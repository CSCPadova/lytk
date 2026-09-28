//! PyO3 bindings — the `lytk._core` extension module.
//!
//! Every function and method here that reads, writes or transforms music runs
//! inside [`guard`], so a Rust panic reaches Python as `lytk.InternalError`.

use std::path::{Path, PathBuf};

use std::borrow::Cow;

use pyo3::exceptions::{PyIOError, PyValueError};
use pyo3::prelude::*;
use pyo3::sync::GILOnceCell;
use pyo3::types::{PyBytes, PyDict, PyModule, PyTuple, PyType};

use crate::adapters::{FromIrAdapter, FromMusicAdapter, ToIrAdapter};
use crate::diagnostics::Diagnostic;
use crate::ir::interval::Interval;
use crate::ir::language::{PitchLanguage, PitchMode};
use crate::ir::music::MusicDocument;
use crate::ir::pitch::{Alter, Pitch, PitchStep};
use crate::ir::Score;
use crate::{adapters, ir, navigation, representations, transforms};

// ---------------------------------------------------------------------------
// Errors and the panic firewall
// ---------------------------------------------------------------------------

/// `create_exception!` in pyo3 0.22 checks a `gil-refs` feature that this
/// crate does not declare.
#[allow(unexpected_cfgs)]
mod errors {
    use pyo3::create_exception;

    create_exception!(
        lytk,
        LytkError,
        pyo3::exceptions::PyException,
        "Base class of the errors lytk raises itself."
    );
    create_exception!(
        lytk,
        InternalError,
        LytkError,
        "A bug in lytk: the Rust code panicked. The input is not to blame; please \
         report it at https://github.com/CSCPadova/lytk/issues."
    );
}
use errors::{InternalError, LytkError};

/// `ParseError(LytkError, ValueError)` and `LilyPondSyntaxError(ParseError)`:
/// classes with two bases, which `create_exception!` cannot make.
static PARSE_ERROR: GILOnceCell<Py<PyType>> = GILOnceCell::new();
static SYNTAX_ERROR: GILOnceCell<Py<PyType>> = GILOnceCell::new();

fn new_exception(
    py: Python<'_>,
    name: &str,
    bases: Vec<Bound<'_, PyType>>,
    doc: &str,
) -> PyResult<Py<PyType>> {
    let namespace = PyDict::new_bound(py);
    namespace.set_item("__module__", "lytk")?;
    namespace.set_item("__doc__", doc)?;
    let class =
        py.get_type_bound::<PyType>()
            .call1((name, PyTuple::new_bound(py, bases), namespace))?;
    Ok(class.downcast_into::<PyType>()?.unbind())
}

fn parse_error_type(py: Python<'_>) -> PyResult<&Bound<'_, PyType>> {
    let class = PARSE_ERROR.get_or_try_init(py, || {
        new_exception(
            py,
            "ParseError",
            vec![
                py.get_type_bound::<LytkError>(),
                py.get_type_bound::<PyValueError>(),
            ],
            "The input could not be read: malformed, not the format expected, or \
             past the reader's bounds. A ValueError too.",
        )
    })?;
    Ok(class.bind(py))
}

fn syntax_error_type(py: Python<'_>) -> PyResult<&Bound<'_, PyType>> {
    let class = SYNTAX_ERROR.get_or_try_init(py, || {
        new_exception(
            py,
            "LilyPondSyntaxError",
            vec![parse_error_type(py)?.clone()],
            "LilyPond read with strict=True has errors. Its ``diagnostics`` \
             attribute lists them, with the warnings.",
        )
    })?;
    Ok(class.bind(py))
}

/// A `ParseError`: the input could not be read.
fn parse_error(message: String) -> PyErr {
    Python::with_gil(|py| match parse_error_type(py) {
        Ok(class) => PyErr::from_type_bound(class.clone(), message),
        Err(e) => e,
    })
}

/// A reader's failure: I/O stays `OSError`, anything else is a `ParseError`.
fn read_err(e: adapters::AdapterError) -> PyErr {
    match e {
        adapters::AdapterError::Io(_) => PyIOError::new_err(e.to_string()),
        _ => parse_error(e.to_string()),
    }
}

/// How many `guard` calls are active, on any thread: the LilyPond reader walks
/// on a thread of its own, so a panic can happen away from the caller's.
static GUARDED: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
/// The last guarded panic: message and source location.
static LAST_PANIC: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);

fn last_panic() -> std::sync::MutexGuard<'static, Option<String>> {
    LAST_PANIC
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn panic_message(payload: &(dyn std::any::Any + Send)) -> String {
    payload
        .downcast_ref::<&str>()
        .map(|s| (*s).to_string())
        .or_else(|| payload.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "unknown panic".to_string())
}

/// Install (once) a panic hook that records a panic raised inside [`guard`]
/// instead of printing it to stderr; any other panic keeps the default hook.
fn install_panic_hook() {
    static HOOK: std::sync::Once = std::sync::Once::new();
    HOOK.call_once(|| {
        let default = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            if GUARDED.load(std::sync::atomic::Ordering::SeqCst) == 0 {
                default(info);
                return;
            }
            let location = info
                .location()
                .map(|l| format!(" (at {}:{})", l.file(), l.line()))
                .unwrap_or_default();
            let message = format!("{}{location}", panic_message(info.payload()));
            *last_panic() = Some(message);
        }));
    });
}

/// Run a binding's body so that a Rust panic becomes `lytk.InternalError`
/// instead of reaching Python as a `PanicException` (a `BaseException` that
/// `except Exception` does not catch) with its message on stderr.
fn guard<T>(f: impl FnOnce() -> PyResult<T>) -> PyResult<T> {
    use std::sync::atomic::Ordering::SeqCst;
    GUARDED.fetch_add(1, SeqCst);
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(f));
    GUARDED.fetch_sub(1, SeqCst);
    result.unwrap_or_else(|payload| {
        let detail = last_panic()
            .take()
            .unwrap_or_else(|| panic_message(&*payload));
        Err(InternalError::new_err(format!(
            "lytk panicked: {detail}. This is a bug in lytk, not in the input; please \
             report it at https://github.com/CSCPadova/lytk/issues"
        )))
    })
}

/// Deserialize a hand-supplied IR (`from_json`, `from_dict`), refusing values
/// no reader produces and the IR divides by: a 0 time-signature denominator
/// or tuplet term, or a negative duration.
fn ir_from_json<T: serde::de::DeserializeOwned>(json: &str) -> PyResult<T> {
    let value: serde_json::Value =
        serde_json::from_str(json).map_err(|e| parse_error(e.to_string()))?;
    check_ir_values(&value).map_err(parse_error)?;
    serde_json::from_value(value).map_err(|e| parse_error(e.to_string()))
}

fn check_ir_values(value: &serde_json::Value) -> Result<(), String> {
    use serde_json::Value;
    // A duration's `[numerator, denominator]`: the readers keep both far below
    // 2^40 (multipliers are bounded at 2^24), and the arithmetic relies on it.
    const DURATION_TERM_MAX: i64 = 1 << 40;
    let ratio = |v: &Value| Some((v.get(0)?.as_i64()?, v.get(1)?.as_i64()?));
    let bad = |what: String| Err(format!("invalid IR: {what}"));
    match value {
        Value::Object(map) => {
            for (key, v) in map {
                match key.as_str() {
                    "beat_type" | "tuplet_actual" | "tuplet_normal" | "actual" | "normal"
                        if v.as_u64() == Some(0) =>
                    {
                        return bad(format!("{key} is 0"));
                    }
                    "base" => match ratio(v) {
                        Some((n, d))
                            if (0..=DURATION_TERM_MAX).contains(&n)
                                && (1..=DURATION_TERM_MAX).contains(&d) => {}
                        Some(_) => return bad(format!("duration {v} out of range")),
                        None => check_ir_values(v)?,
                    },
                    "lilypond_version"
                        if v.as_str().is_some_and(|s| {
                            s.parse::<adapters::ly_to_ir::LilyPondVersion>().is_err()
                        }) =>
                    {
                        return bad(format!("lilypond_version {v} is not a LilyPond version"));
                    }
                    // The LilyPond reader's bound (its regression tests climb to 22).
                    "octave" if v.as_i64().is_some_and(|o| !(-128..=127).contains(&o)) => {
                        return bad(format!("octave {v} out of range -128..=127"));
                    }
                    // A pitch's alteration is a `[n, d]` ratio, a chord root's a number.
                    "alter" => {
                        let ok = match ratio(v) {
                            Some((n, d)) => d > 0 && n.abs() <= 4 * d,
                            None => v.as_f64().is_none_or(|x| x.abs() <= 4.0),
                        };
                        if !ok {
                            return bad(format!("alteration {v} out of range"));
                        }
                    }
                    "beats"
                        if v.as_str().is_some_and(|b| {
                            b.split('+').count() > 64
                                || b.split('+')
                                    .any(|n| n.trim().parse::<u32>().is_ok_and(|n| n > 10_000))
                        }) =>
                    {
                        return bad(format!("time signature beats {v} out of range"));
                    }
                    _ => check_ir_values(v)?,
                }
            }
            Ok(())
        }
        Value::Array(items) => items.iter().try_for_each(check_ir_values),
        _ => Ok(()),
    }
}

/// Panic on purpose, inside the firewall: lets the test suite check that a
/// panic becomes `InternalError` without printing anything. Not public API.
#[pyfunction]
fn _panic_for_tests(message: &str) -> PyResult<()> {
    guard(|| panic!("{message}"))
}

// ---------------------------------------------------------------------------
// Diagnostics
// ---------------------------------------------------------------------------

/// One finding of the LilyPond reader: ``severity`` is ``"error"`` (LilyPond
/// rejects the input) or ``"warning"`` (lytk does not read it, or cannot
/// represent it); ``code`` is a stable identifier such as
/// ``"missing-token"``. ``line`` and ``column`` count from 1 (the column in
/// characters); ``text[start:end]`` is the span.
#[pyclass(name = "Diagnostic", module = "lytk", frozen, get_all, eq, hash)]
#[derive(Clone, PartialEq, Eq, Hash)]
struct PyDiagnostic {
    severity: String,
    code: String,
    message: String,
    line: usize,
    column: usize,
    start: usize,
    end: usize,
}

type DiagnosticFields = (String, String, String, usize, usize, usize, usize);

#[pymethods]
impl PyDiagnostic {
    #[new]
    fn new(
        severity: String,
        code: String,
        message: String,
        line: usize,
        column: usize,
        start: usize,
        end: usize,
    ) -> Self {
        Self {
            severity,
            code,
            message,
            line,
            column,
            start,
            end,
        }
    }

    /// Pickling (an exception carrying diagnostics crosses process pools).
    fn __reduce__<'py>(slf: &Bound<'py, Self>) -> (Bound<'py, PyType>, DiagnosticFields) {
        let d = slf.get();
        (
            slf.get_type(),
            (
                d.severity.clone(),
                d.code.clone(),
                d.message.clone(),
                d.line,
                d.column,
                d.start,
                d.end,
            ),
        )
    }

    /// ``3:12: error: missing `}` [missing-token]``
    fn __str__(&self) -> String {
        format!(
            "{}:{}: {}: {} [{}]",
            self.line, self.column, self.severity, self.message, self.code
        )
    }

    fn __repr__(&self) -> String {
        format!("<Diagnostic {}>", self.__str__())
    }
}

/// The character offset into `text` of each byte offset in `offsets`
/// (floored to a character boundary), counted in one pass over `text`.
fn char_offsets<'t>(
    text: &'t str,
    offsets: impl IntoIterator<Item = usize>,
) -> impl Fn(usize) -> usize + 't {
    let boundary = move |b: usize| {
        let mut b = b.min(text.len());
        while !text.is_char_boundary(b) {
            b -= 1;
        }
        b
    };
    let mut bytes: Vec<usize> = offsets.into_iter().map(boundary).collect();
    bytes.sort_unstable();
    bytes.dedup();
    let (mut chars, mut count, mut at) = (Vec::with_capacity(bytes.len()), 0, 0);
    for &b in &bytes {
        count += text[at..b].chars().count();
        at = b;
        chars.push(count);
    }
    move |b| chars[bytes.binary_search(&boundary(b)).unwrap_or(0)]
}

/// Diagnostics for Python, their byte ranges turned into character offsets
/// into `text`.
fn py_diagnostics(text: &str, diagnostics: &[Diagnostic]) -> Vec<PyDiagnostic> {
    let char_at = char_offsets(text, diagnostics.iter().flat_map(|d| [d.start, d.end]));
    diagnostics
        .iter()
        .map(|d| PyDiagnostic {
            severity: d.severity.as_str().to_string(),
            code: d.code.to_string(),
            message: d.message.clone(),
            line: d.line,
            column: d.column,
            start: char_at(d.start),
            end: char_at(d.end),
        })
        .collect()
}

/// With `strict`, an error among `diagnostics` fails the reading: a
/// `LilyPondSyntaxError` that carries them all.
fn check_strict(strict: bool, diagnostics: &[PyDiagnostic]) -> PyResult<()> {
    let mut errors = diagnostics.iter().filter(|d| d.severity == "error");
    let (Some(first), true) = (errors.next(), strict) else {
        return Ok(());
    };
    let more = match errors.count() {
        0 => String::new(),
        1 => " (and 1 more error)".to_string(),
        n => format!(" (and {n} more errors)"),
    };
    Python::with_gil(|py| {
        let class = syntax_error_type(py)?.clone();
        let err = PyErr::from_type_bound(class, format!("{}{more}", first.__str__()));
        err.value_bound(py)
            .setattr("diagnostics", diagnostics.to_vec().into_py(py))?;
        Err(err)
    })
}

/// A field of a LilyPond ``\header`` block, from :func:`header_fields`:
/// ``text[start:end]`` is the whole ``key = value``, and ``score`` the index
/// of the ``\score`` block it is in (in file order), or *None*.
#[pyclass(name = "HeaderField", module = "lytk", frozen, get_all, eq, hash)]
#[derive(Clone, PartialEq, Eq, Hash)]
struct PyHeaderField {
    key: String,
    value: String,
    start: usize,
    end: usize,
    score: Option<usize>,
}

#[pymethods]
impl PyHeaderField {
    fn __repr__(&self) -> String {
        format!(
            "HeaderField({:?}, {:?}, start={}, end={}, score={:?})",
            self.key, self.value, self.start, self.end, self.score
        )
    }
}

/// Every ``\header`` field of LilyPond text, in source order, from the parse
/// tree alone (nothing is read). Values are text: strings decoded, a
/// ``\markup`` value as its plain words, ``#"…"`` as its string; fields with
/// other values (``##f``) are left out. ``text[f.start:f.end]`` is the whole
/// assignment, so a field can be read and cut.
#[pyfunction]
fn header_fields(py: Python<'_>, text: &str) -> PyResult<Vec<PyHeaderField>> {
    guard(|| {
        let fields = py.allow_threads(|| adapters::ly_to_ir::header_fields(text));
        let char_at = char_offsets(text, fields.iter().flat_map(|f| [f.start, f.end]));
        Ok(fields
            .into_iter()
            .map(|f| PyHeaderField {
                start: char_at(f.start),
                end: char_at(f.end),
                key: f.key,
                value: f.value,
                score: f.score,
            })
            .collect())
    })
}

/// The header fields of `meta` as a dict: the ones the IR names, then the
/// others sorted by key.
fn header_dict<'py>(
    py: Python<'py>,
    meta: &ir::score::ScoreMetadata,
) -> PyResult<Bound<'py, PyDict>> {
    let dict = PyDict::new_bound(py);
    let named = [
        ("title", &meta.title),
        ("subtitle", &meta.subtitle),
        ("composer", &meta.composer),
        ("arranger", &meta.arranger),
        ("lyricist", &meta.lyricist),
    ];
    for (key, value) in named {
        if let Some(value) = value {
            dict.set_item(key, value)?;
        }
    }
    let mut extra: Vec<(&String, &String)> = meta.extra.iter().collect();
    extra.sort();
    for (key, value) in extra {
        dict.set_item(key, value)?;
    }
    Ok(dict)
}

/// Read LilyPond, from a file or text, releasing the GIL: the text and the
/// reading. With *include_paths*, includes are followed (relative to a
/// file's directory, then the paths).
fn read_lilypond(
    py: Python<'_>,
    source: Result<&str, &str>,
    language: Option<&str>,
    include_paths: Option<Vec<String>>,
) -> PyResult<(String, adapters::ly_to_ir::LyReading)> {
    let mut adapter = adapters::ly_to_ir::LyToIrAdapter::new();
    if let Some(lang_str) = language {
        adapter = adapter.with_language(parse_language(lang_str)?);
    }
    if let Some(paths) = include_paths {
        adapter = adapter.with_include_paths(paths.into_iter().map(PathBuf::from).collect());
    }
    py.allow_threads(|| {
        let (text, base_dir) = match source {
            Ok(path) => (
                Cow::Owned(adapters::ly_to_ir::read_source(Path::new(path))?),
                Path::new(path).parent(),
            ),
            Err(text) => (Cow::Borrowed(text), None),
        };
        let reading = adapter.read_text(&text, base_dir)?;
        Ok((text.into_owned(), reading))
    })
    .map_err(read_err)
}

/// The first movement of a LilyPond reading, with its diagnostics.
fn first_movement(
    text: &str,
    reading: adapters::ly_to_ir::LyReading,
    strict: bool,
) -> PyResult<(Score, Vec<PyDiagnostic>)> {
    let (score, diagnostics) = reading.into_first();
    let diagnostics = py_diagnostics(text, &diagnostics);
    check_strict(strict, &diagnostics)?;
    Ok((score, diagnostics))
}

/// Check LilyPond text and return its :class:`Diagnostic` list, errors and
/// warnings in source order. By default only the syntax, from the parse tree
/// (fast, nothing is read); with ``semantic=True`` also what a reading
/// reports: invalid durations and ratios, unknown commands, input it does not
/// read. Input too large to read is an error here, not an exception.
/// *include_paths* follows includes as the readers do (see
/// :func:`from_lilypond`); an included file that cannot be read raises
/// ``OSError``.
#[pyfunction]
#[pyo3(signature = (text, *, semantic=false, include_paths=None))]
fn check_lilypond(
    py: Python<'_>,
    text: &str,
    semantic: bool,
    include_paths: Option<Vec<String>>,
) -> PyResult<Vec<PyDiagnostic>> {
    guard(|| {
        let mut adapter = adapters::ly_to_ir::LyToIrAdapter::new();
        if let Some(paths) = include_paths {
            adapter = adapter.with_include_paths(paths.into_iter().map(PathBuf::from).collect());
        }
        let diagnostics = py
            .allow_threads(|| adapter.check_str(text, semantic))
            .map_err(read_err)?;
        Ok(py_diagnostics(text, &diagnostics))
    })
}

// ---------------------------------------------------------------------------
// LilyPond versions
// ---------------------------------------------------------------------------

/// A LilyPond version, as ``\\version`` states it, compared numerically:
/// ``LilyPondVersion("2.24") == LilyPondVersion("2.24.0")``. Accepted as
/// LilyPond 2.24 accepts it: ``major.minor.patch`` with an optional fourth
/// part (kept, not compared), or ``major.minor`` with an even minor (a stable
/// series). Any other string raises :class:`ParseError`. ``str()`` gives
/// ``"2.24.0"``.
#[pyclass(name = "LilyPondVersion", module = "lytk", frozen)]
#[derive(Clone)]
struct PyLilyPondVersion(adapters::ly_to_ir::LilyPondVersion);

#[pymethods]
impl PyLilyPondVersion {
    #[new]
    fn new(text: &str) -> PyResult<Self> {
        text.parse().map(Self).map_err(parse_error)
    }

    #[getter]
    fn major(&self) -> u32 {
        self.0.major
    }

    #[getter]
    fn minor(&self) -> u32 {
        self.0.minor
    }

    #[getter]
    fn patch(&self) -> u32 {
        self.0.patch
    }

    /// The fourth part (``"foo"`` in ``2.25.3.foo``), if any.
    #[getter]
    fn extra(&self) -> Option<String> {
        self.0.extra.clone()
    }

    fn __str__(&self) -> String {
        self.0.to_string()
    }

    fn __repr__(&self, py: Python<'_>) -> PyResult<String> {
        let text = pyo3::types::PyString::new_bound(py, &self.0.to_string());
        Ok(format!("LilyPondVersion({})", text.repr()?))
    }

    fn __richcmp__(&self, other: PyRef<'_, Self>, op: pyo3::basic::CompareOp) -> bool {
        op.matches(self.0.cmp(&other.0))
    }

    fn __hash__(&self) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        self.0.hash(&mut hasher);
        hasher.finish()
    }

    fn __reduce__<'py>(slf: &Bound<'py, Self>) -> (Bound<'py, PyType>, (String,)) {
        (slf.get_type(), (slf.get().0.to_string(),))
    }
}

/// A version argument: a :class:`LilyPondVersion`, or a string to parse.
fn version_arg(version: &Bound<'_, PyAny>) -> PyResult<adapters::ly_to_ir::LilyPondVersion> {
    if let Ok(v) = version.downcast::<PyLilyPondVersion>() {
        return Ok(v.get().0.clone());
    }
    version.extract::<String>()?.parse().map_err(parse_error)
}

/// The version stored in `meta`, read from a ``\\version``.
fn meta_version(meta: &ir::score::ScoreMetadata) -> Option<PyLilyPondVersion> {
    let version = meta.lilypond_version.as_deref()?.parse().ok()?;
    Some(PyLilyPondVersion(version))
}

/// The version the first ``\\version`` statement of LilyPond text states,
/// from the parse tree (a commented-out one does not count): *None* when
/// there is none, or when it is not a valid version (:func:`check_lilypond`
/// reports it as ``invalid-version``).
#[pyfunction]
fn lilypond_version(py: Python<'_>, text: &str) -> PyResult<Option<PyLilyPondVersion>> {
    guard(|| {
        let version = py.allow_threads(|| adapters::ly_to_ir::lilypond_version(text));
        Ok(version.map(PyLilyPondVersion))
    })
}

/// *text* with every ``\\version`` statement stating *version* (a
/// :class:`LilyPondVersion` or a string), or with one added at the top when
/// there is none.
#[pyfunction]
fn set_lilypond_version(
    py: Python<'_>,
    text: &str,
    version: &Bound<'_, PyAny>,
) -> PyResult<String> {
    let version = version_arg(version)?;
    guard(|| {
        py.allow_threads(|| adapters::ly_to_ir::set_lilypond_version(text, &version))
            .map_err(read_err)
    })
}

/// *text* without its ``\\version`` statements; a statement alone on its
/// line takes the line with it.
#[pyfunction]
fn strip_lilypond_version(py: Python<'_>, text: &str) -> PyResult<String> {
    guard(|| {
        py.allow_threads(|| adapters::ly_to_ir::strip_lilypond_version(text))
            .map_err(read_err)
    })
}

// ---------------------------------------------------------------------------
// Tokens
// ---------------------------------------------------------------------------

/// A token of LilyPond text, from :func:`tokenize`: ``kind`` is one of
/// ``"comment"``, ``"string"``, ``"scheme"``, ``"command"``, ``"symbol"``,
/// ``"number"``, ``"fraction"``, ``"punctuation"`` and ``"error"`` (text the
/// grammar cannot tokenize); ``text[start:end]`` is the token (character
/// offsets); ``line`` and ``column`` count from 1.
#[pyclass(name = "Token", module = "lytk", frozen, get_all, eq, hash)]
#[derive(Clone, PartialEq, Eq, Hash)]
struct PyToken {
    kind: &'static str,
    text: String,
    start: usize,
    end: usize,
    line: usize,
    column: usize,
}

#[pymethods]
impl PyToken {
    fn __repr__(&self) -> String {
        format!(
            "Token({:?}, {:?}, start={}, end={}, line={}, column={})",
            self.kind, self.text, self.start, self.end, self.line, self.column
        )
    }
}

/// The tokens of LilyPond text, in order, from the parse tree: strings,
/// embedded Scheme expressions and comments whole. Whitespace is no token;
/// every other character is in exactly one, so the tokens of broken input
/// cover it too (as ``"error"`` tokens where needed).
#[pyfunction]
fn tokenize(py: Python<'_>, text: &str) -> PyResult<Vec<PyToken>> {
    guard(|| {
        let tokens = py
            .allow_threads(|| adapters::ly_to_ir::tokenize(text))
            .map_err(read_err)?;
        let char_at = char_offsets(text, tokens.iter().flat_map(|t| [t.start, t.end]));
        Ok(tokens
            .into_iter()
            .map(|t| PyToken {
                kind: t.kind.as_str(),
                text: text[t.start..t.end].to_string(),
                start: char_at(t.start),
                end: char_at(t.end),
                line: t.line,
                column: t.column,
            })
            .collect())
    })
}

/// *text* without its LilyPond comments (``% …``, ``%{ … %}``): a block
/// comment between two tokens becomes a space, a line comment leaves its
/// line break. Comments inside embedded Scheme stay.
#[pyfunction]
fn strip_comments(py: Python<'_>, text: &str) -> PyResult<String> {
    guard(|| {
        py.allow_threads(|| adapters::ly_to_ir::strip_comments(text))
            .map_err(read_err)
    })
}

/// Counts of LilyPond source text, from its tokens (:func:`tokenize`):
/// ``bytes`` (UTF-8), ``lines``, ``tokens``, ``comments``, ``scheme``
/// (embedded Scheme expressions) and ``error_tokens`` (text the grammar
/// cannot tokenize).
#[pyfunction]
fn source_stats<'py>(py: Python<'py>, text: &str) -> PyResult<Bound<'py, PyDict>> {
    guard(|| {
        let s = py
            .allow_threads(|| adapters::ly_to_ir::source_stats(text))
            .map_err(read_err)?;
        let dict = PyDict::new_bound(py);
        for (key, value) in [
            ("bytes", s.bytes),
            ("lines", s.lines),
            ("tokens", s.tokens),
            ("comments", s.comments),
            ("scheme", s.scheme),
            ("error_tokens", s.error_tokens),
        ] {
            dict.set_item(key, value)?;
        }
        Ok(dict)
    })
}

// ---------------------------------------------------------------------------
// Statistics
// ---------------------------------------------------------------------------

/// A score's metadata and counts, as ``lytk info --json`` prints them:
/// ``title``, ``subtitle``, ``composer``, ``arranger``, ``lyricist``,
/// ``language``, ``lilypond_version`` (strings or *None*); ``part_count``,
/// ``note_count`` (sounding notes: chord members and grace notes each
/// count), ``voice_count`` (distinct voices of each part, summed),
/// ``bar_count``, ``duration_quarters`` (each bar as long as its longest
/// voice), ``lyric_count`` (syllables), ``chord_symbol_count``,
/// ``grace_note_count``; and ``parts``, one dict per part (``id``, ``name``,
/// ``abbreviation``, ``measures``, ``staves``, ``midi_program``,
/// ``midi_instrument``, ``voices``, ``notes``).
#[pyfunction]
fn info<'py>(py: Python<'py>, score: &PyScore) -> PyResult<Bound<'py, PyDict>> {
    guard(|| {
        let s = &score.inner;
        let counts = ir::stats::score_info(s);
        let meta = &s.metadata;
        let dict = PyDict::new_bound(py);
        let language = meta.pitch_language.map(|l| l.as_str().to_string());
        for (key, value) in [
            ("title", &meta.title),
            ("subtitle", &meta.subtitle),
            ("composer", &meta.composer),
            ("arranger", &meta.arranger),
            ("lyricist", &meta.lyricist),
            ("language", &language),
            ("lilypond_version", &meta.lilypond_version),
        ] {
            dict.set_item(key, value)?;
        }
        let quarters = counts.duration_quarters;
        dict.set_item("part_count", counts.parts.len())?;
        dict.set_item("note_count", counts.note_count)?;
        dict.set_item("voice_count", counts.voice_count)?;
        dict.set_item("bar_count", counts.bar_count)?;
        dict.set_item(
            "duration_quarters",
            *quarters.numer() as f64 / *quarters.denom() as f64,
        )?;
        dict.set_item("lyric_count", counts.lyric_count)?;
        dict.set_item("chord_symbol_count", counts.chord_symbol_count)?;
        dict.set_item("grace_note_count", counts.grace_note_count)?;
        let parts = s
            .parts()
            .into_iter()
            .zip(&counts.parts)
            .map(|(part, c)| {
                let d = PyDict::new_bound(py);
                d.set_item("id", &part.part_id)?;
                d.set_item("name", &part.name)?;
                d.set_item("abbreviation", &part.abbreviation)?;
                d.set_item("measures", c.measures)?;
                d.set_item("staves", part.staves)?;
                d.set_item("midi_program", part.midi_program)?;
                d.set_item("midi_instrument", &part.midi_instrument)?;
                d.set_item("voices", c.voices)?;
                d.set_item("notes", c.notes)?;
                Ok(d)
            })
            .collect::<PyResult<Vec<_>>>()?;
        dict.set_item("parts", parts)?;
        Ok(dict)
    })
}

// ---------------------------------------------------------------------------
// PyScore — opaque wrapper for the IR Score
// ---------------------------------------------------------------------------

/// An opaque handle to a parsed music score.
///
/// Provides read-only access to metadata and serialisation helpers.
/// All heavy processing (parsing, transforms, emission) happens through
/// the module-level functions.
#[pyclass(name = "Score")]
#[derive(Clone)]
struct PyScore {
    inner: Score,
    diagnostics: Vec<PyDiagnostic>,
}

impl From<Score> for PyScore {
    fn from(inner: Score) -> Self {
        Self {
            inner,
            diagnostics: Vec::new(),
        }
    }
}

#[pymethods]
impl PyScore {
    /// What reading LilyPond reported, as :class:`Diagnostic` objects (empty
    /// for other sources). Not part of :meth:`to_dict` or :meth:`to_json`.
    #[getter]
    fn diagnostics(&self) -> Vec<PyDiagnostic> {
        self.diagnostics.clone()
    }

    /// Score title (from the MusicXML or LilyPond header).
    #[getter]
    fn title(&self) -> Option<String> {
        self.inner.metadata.title.clone()
    }

    /// Composer name.
    #[getter]
    fn composer(&self) -> Option<String> {
        self.inner.metadata.composer.clone()
    }

    /// Subtitle.
    #[getter]
    fn subtitle(&self) -> Option<String> {
        self.inner.metadata.subtitle.clone()
    }

    /// Arranger name.
    #[getter]
    fn arranger(&self) -> Option<String> {
        self.inner.metadata.arranger.clone()
    }

    /// Lyricist (LilyPond ``poet``/``lyricist``, MusicXML ``lyricist`` creator).
    #[getter]
    fn lyricist(&self) -> Option<String> {
        self.inner.metadata.lyricist.clone()
    }

    /// The :class:`LilyPondVersion` the source's ``\\version`` states, for a
    /// score read from LilyPond that states a valid one; else *None*.
    #[getter]
    fn lilypond_version(&self) -> Option<PyLilyPondVersion> {
        meta_version(&self.inner.metadata)
    }

    /// Every header field as a dict of strings: ``title``, ``subtitle``,
    /// ``composer``, ``arranger`` and ``lyricist`` (LilyPond's ``poet``) when
    /// set, then the others (``copyright``, ``opus``, ``texidoc``, …) by key.
    #[getter]
    fn header<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyDict>> {
        header_dict(py, &self.inner.metadata)
    }

    /// Active LilyPond pitch language (e.g. ``"nederlands"``), or *None*.
    #[getter]
    fn language(&self) -> Option<String> {
        self.inner
            .metadata
            .pitch_language
            .map(|l| l.as_str().to_string())
    }

    /// Number of instrument parts.
    #[getter]
    fn num_parts(&self) -> usize {
        self.inner.parts().len()
    }

    /// Part names (or IDs when unnamed) as a list of strings.
    #[getter]
    fn parts(&self) -> Vec<String> {
        self.inner
            .parts()
            .iter()
            .map(|p| {
                if p.name.is_empty() {
                    p.part_id.clone()
                } else {
                    p.name.clone()
                }
            })
            .collect()
    }

    /// Structured note navigation: the score's parts as typed :class:`Part`
    /// objects, walkable as ``part.measures → measure.voices → voice.elements``
    /// (each element a :class:`Note` / :class:`Rest` / :class:`Chord`). A
    /// structure-preserving complement to the flat :meth:`notes` tuples.
    fn iter_parts(&self) -> PyResult<Vec<navigation::PyPart>> {
        guard(|| {
            Ok({
                self.inner
                    .parts()
                    .iter()
                    .map(|p| navigation::PyPart::from_ir(p))
                    .collect()
            })
        })
    }

    /// Serialize the full score IR to a JSON string.
    fn to_json(&self) -> PyResult<String> {
        guard(|| {
            serde_json::to_string_pretty(&self.inner)
                .map_err(|e| PyValueError::new_err(e.to_string()))
        })
    }

    /// Deserialize a score from a JSON string.
    #[staticmethod]
    fn from_json(json: &str) -> PyResult<Self> {
        guard(|| Ok(PyScore::from(ir_from_json::<Score>(json)?)))
    }

    /// Serialize the score IR to a Python dict.
    fn to_dict<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        guard(|| {
            let json_str = serde_json::to_string(&self.inner)
                .map_err(|e| PyValueError::new_err(e.to_string()))?;
            let json_mod = PyModule::import_bound(py, "json")?;
            json_mod.call_method1("loads", (json_str,))
        })
    }

    /// Deserialize a score from a Python dict.
    #[staticmethod]
    fn from_dict(dict: &Bound<'_, PyAny>) -> PyResult<Self> {
        guard(|| {
            let py = dict.py();
            let json_mod = PyModule::import_bound(py, "json")?;
            let json_str: String = json_mod.call_method1("dumps", (dict,))?.extract()?;
            Ok(PyScore::from(ir_from_json::<Score>(&json_str)?))
        })
    }

    /// Lift this measure-based :class:`Score` to a Layer-1 :class:`MusicDocument`
    /// (the form the ML representations consume).
    fn to_music_document(&self) -> PyResult<PyMusicDocument> {
        guard(|| Ok(PyMusicDocument::from(ir::lift::lift_to_music(&self.inner))))
    }

    /// The score's notes as ``(onset, duration, pitch, velocity)`` tuples in time
    /// steps (``resolution`` = steps per quarter note). A lightweight, numpy-free
    /// way to iterate notes directly, without going through
    /// :func:`to_note_array` or hand-walking :meth:`to_dict`.
    #[pyo3(signature = (resolution = representations::note_array::DEFAULT_RESOLUTION))]
    fn notes(&self, resolution: u16) -> PyResult<Vec<(u32, u32, u8, u8)>> {
        guard(|| {
            Ok({
                let doc = ir::lift::lift_to_music(&self.inner);
                representations::to_note_array(&doc, resolution)
                    .notes
                    .iter()
                    .map(|n| (n.onset, n.duration, n.pitch, n.velocity))
                    .collect()
            })
        })
    }

    fn __repr__(&self) -> String {
        format!("{}", self.inner)
    }

    fn __str__(&self) -> String {
        format!("{}", self.inner)
    }

    fn __eq__(&self, other: &PyScore) -> bool {
        self.inner == other.inner
    }
}

// ---------------------------------------------------------------------------
// PyMusicDocument — opaque wrapper for the Music tree (Layer 1)
// ---------------------------------------------------------------------------

/// An opaque handle to a parsed music document (Layer 1 IR).
///
/// The Music tree preserves structural information (contexts, sequential/
/// simultaneous blocks, variables) that is lost when converting to the
/// measure-based Score (Layer 2) representation.
#[pyclass(name = "MusicDocument")]
#[derive(Clone)]
struct PyMusicDocument {
    inner: MusicDocument,
    diagnostics: Vec<PyDiagnostic>,
}

impl From<MusicDocument> for PyMusicDocument {
    fn from(inner: MusicDocument) -> Self {
        Self {
            inner,
            diagnostics: Vec::new(),
        }
    }
}

#[pymethods]
impl PyMusicDocument {
    /// What reading LilyPond reported, as :class:`Diagnostic` objects (empty
    /// for other sources). Not part of :meth:`to_json`.
    #[getter]
    fn diagnostics(&self) -> Vec<PyDiagnostic> {
        self.diagnostics.clone()
    }

    /// Score title (from the header).
    #[getter]
    fn title(&self) -> Option<String> {
        self.inner.metadata.title.clone()
    }

    /// Composer name.
    #[getter]
    fn composer(&self) -> Option<String> {
        self.inner.metadata.composer.clone()
    }

    /// Subtitle.
    #[getter]
    fn subtitle(&self) -> Option<String> {
        self.inner.metadata.subtitle.clone()
    }

    /// Arranger name.
    #[getter]
    fn arranger(&self) -> Option<String> {
        self.inner.metadata.arranger.clone()
    }

    /// Lyricist (LilyPond ``poet``/``lyricist``).
    #[getter]
    fn lyricist(&self) -> Option<String> {
        self.inner.metadata.lyricist.clone()
    }

    /// The :class:`LilyPondVersion` the source's ``\\version`` states (see
    /// :attr:`Score.lilypond_version`).
    #[getter]
    fn lilypond_version(&self) -> Option<PyLilyPondVersion> {
        meta_version(&self.inner.metadata)
    }

    /// Every header field as a dict of strings (see :attr:`Score.header`).
    #[getter]
    fn header<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyDict>> {
        header_dict(py, &self.inner.metadata)
    }

    /// Active LilyPond pitch language (e.g. ``"nederlands"``), or *None*.
    #[getter]
    fn language(&self) -> Option<String> {
        self.inner
            .metadata
            .pitch_language
            .map(|l| l.as_str().to_string())
    }

    /// The document's notes as ``(onset, duration, pitch, velocity)`` tuples in
    /// time steps (``resolution`` = steps per quarter note) — a lightweight,
    /// numpy-free way to iterate notes directly.
    #[pyo3(signature = (resolution = representations::note_array::DEFAULT_RESOLUTION))]
    fn notes(&self, resolution: u16) -> PyResult<Vec<(u32, u32, u8, u8)>> {
        guard(|| {
            Ok({
                representations::to_note_array(&self.inner, resolution)
                    .notes
                    .iter()
                    .map(|n| (n.onset, n.duration, n.pitch, n.velocity))
                    .collect()
            })
        })
    }

    /// Serialize the music document to a JSON string.
    fn to_json(&self) -> PyResult<String> {
        guard(|| {
            serde_json::to_string_pretty(&self.inner)
                .map_err(|e| PyValueError::new_err(e.to_string()))
        })
    }

    /// Deserialize a music document from a JSON string.
    #[staticmethod]
    fn from_json(json: &str) -> PyResult<Self> {
        guard(|| Ok(PyMusicDocument::from(ir_from_json::<MusicDocument>(json)?)))
    }

    /// Convert this music document to a measure-based :class:`Score`.
    fn to_score(&self) -> PyResult<PyScore> {
        guard(|| Ok(PyScore::from(ir::lower::lower_to_score(&self.inner))))
    }

    fn __repr__(&self) -> String {
        format!("MusicDocument(title={:?})", self.inner.metadata.title)
    }

    fn __eq__(&self, other: &PyMusicDocument) -> bool {
        self.inner == other.inner
    }
}

// ---------------------------------------------------------------------------
// Adapter functions
// ---------------------------------------------------------------------------

/// A writer's failure: I/O becomes `OSError`, anything else `ValueError`.
/// Readers use [`read_err`].
fn adapter_err(e: adapters::AdapterError) -> PyErr {
    let msg = e.to_string();
    match e {
        adapters::AdapterError::Io(_) => PyIOError::new_err(msg),
        _ => PyValueError::new_err(msg),
    }
}

/// Parse a MusicXML (``.xml``, ``.musicxml``) or compressed MXL file into a
/// :class:`Score`.
#[pyfunction]
fn from_musicxml(py: Python<'_>, path: &str) -> PyResult<PyScore> {
    guard(|| {
        let adapter = adapters::mxml_to_ir::MxmlToIrAdapter::new();
        let score = py
            .allow_threads(|| adapter.convert_file(Path::new(path)))
            .map_err(read_err)?;
        Ok(PyScore::from(score))
    })
}

/// Parse a MusicXML string into a :class:`Score`.
#[pyfunction]
fn from_musicxml_string(xml: &str) -> PyResult<PyScore> {
    guard(|| {
        let adapter = adapters::mxml_to_ir::MxmlToIrAdapter::new();
        let score = adapter.convert_str(xml).map_err(read_err)?;
        Ok(PyScore::from(score))
    })
}

/// Resolve a pitch-language name, raising a `ValueError` for unknown names.
fn parse_language(name: &str) -> PyResult<PitchLanguage> {
    PitchLanguage::from_str_loose(name)
        .ok_or_else(|| PyValueError::new_err(format!("unknown language: {name}")))
}

/// Parse a LilyPond (``.ly``) file into a :class:`Score` (its first movement).
///
/// ``strict=True`` raises :class:`LilyPondSyntaxError` if the reading reports
/// an error; the diagnostics are in :attr:`Score.diagnostics` either way.
/// With *include_paths* (a list of directories, possibly empty),
/// ``\\include``\ s are followed: relative to the file's directory (for a
/// string reader, only the paths), then the paths. An include not found is an
/// ``ignored-include`` warning; a diagnostic in an included file is reported
/// at its ``\\include``. Without it, includes are not read.
#[pyfunction]
#[pyo3(signature = (path, *, language=None, strict=false, include_paths=None))]
fn from_lilypond(
    py: Python<'_>,
    path: &str,
    language: Option<&str>,
    strict: bool,
    include_paths: Option<Vec<String>>,
) -> PyResult<PyScore> {
    guard(|| {
        let (text, reading) = read_lilypond(py, Ok(path), language, include_paths)?;
        let (score, diagnostics) = first_movement(&text, reading, strict)?;
        Ok(PyScore {
            inner: score,
            diagnostics,
        })
    })
}

/// Parse every movement of a LilyPond file: one :class:`Score` per ``\\score``
/// block and per top-level music expression, in order, as LilyPond makes a
/// score of each. Each score carries the file's diagnostics.
#[pyfunction]
#[pyo3(signature = (path, *, language=None, strict=false, include_paths=None))]
fn from_lilypond_movements(
    py: Python<'_>,
    path: &str,
    language: Option<&str>,
    strict: bool,
    include_paths: Option<Vec<String>>,
) -> PyResult<Vec<PyScore>> {
    guard(|| {
        let (text, reading) = read_lilypond(py, Ok(path), language, include_paths)?;
        let diagnostics = py_diagnostics(&text, &reading.diagnostics);
        check_strict(strict, &diagnostics)?;
        Ok(reading
            .scores
            .into_iter()
            .map(|inner| PyScore {
                inner,
                diagnostics: diagnostics.clone(),
            })
            .collect())
    })
}

/// Parse a LilyPond string into a :class:`Score` (its first movement); see
/// :func:`from_lilypond`.
#[pyfunction]
#[pyo3(signature = (text, *, language=None, strict=false, include_paths=None))]
fn from_lilypond_string(
    py: Python<'_>,
    text: &str,
    language: Option<&str>,
    strict: bool,
    include_paths: Option<Vec<String>>,
) -> PyResult<PyScore> {
    guard(|| {
        let (text, reading) = read_lilypond(py, Err(text), language, include_paths)?;
        let (score, diagnostics) = first_movement(&text, reading, strict)?;
        Ok(PyScore {
            inner: score,
            diagnostics,
        })
    })
}

/// Parse a LilyPond file into a :class:`MusicDocument` (Layer 1 Music tree).
///
/// This preserves structural information like contexts and simultaneous
/// blocks. ``strict`` as in :func:`from_lilypond`.
#[pyfunction]
#[pyo3(signature = (path, *, language=None, strict=false, include_paths=None))]
fn from_lilypond_music(
    py: Python<'_>,
    path: &str,
    language: Option<&str>,
    strict: bool,
    include_paths: Option<Vec<String>>,
) -> PyResult<PyMusicDocument> {
    guard(|| {
        let (text, reading) = read_lilypond(py, Ok(path), language, include_paths)?;
        let (score, diagnostics) = first_movement(&text, reading, strict)?;
        Ok(PyMusicDocument {
            inner: ir::lift::lift_to_music(&score),
            diagnostics,
        })
    })
}

/// Parse every movement of a LilyPond file into a :class:`MusicDocument`
/// (see :func:`from_lilypond_movements`). Each carries the file's
/// diagnostics.
#[pyfunction]
#[pyo3(signature = (path, *, language=None, strict=false, include_paths=None))]
fn from_lilypond_music_movements(
    py: Python<'_>,
    path: &str,
    language: Option<&str>,
    strict: bool,
    include_paths: Option<Vec<String>>,
) -> PyResult<Vec<PyMusicDocument>> {
    guard(|| {
        let (text, reading) = read_lilypond(py, Ok(path), language, include_paths)?;
        let diagnostics = py_diagnostics(&text, &reading.diagnostics);
        check_strict(strict, &diagnostics)?;
        Ok(reading
            .scores
            .iter()
            .map(|score| PyMusicDocument {
                inner: ir::lift::lift_to_music(score),
                diagnostics: diagnostics.clone(),
            })
            .collect())
    })
}

/// Parse a LilyPond string into a :class:`MusicDocument` (Layer 1 Music
/// tree); see :func:`from_lilypond_music`.
#[pyfunction]
#[pyo3(signature = (text, *, language=None, strict=false, include_paths=None))]
fn from_lilypond_music_string(
    py: Python<'_>,
    text: &str,
    language: Option<&str>,
    strict: bool,
    include_paths: Option<Vec<String>>,
) -> PyResult<PyMusicDocument> {
    guard(|| {
        let (text, reading) = read_lilypond(py, Err(text), language, include_paths)?;
        let (score, diagnostics) = first_movement(&text, reading, strict)?;
        Ok(PyMusicDocument {
            inner: ir::lift::lift_to_music(&score),
            diagnostics,
        })
    })
}

/// Emit a :class:`MusicDocument` as a LilyPond string.  If *path* is given the
/// result is also written to that file. *version* (a :class:`LilyPondVersion`
/// or a string) is the ``\\version`` written; by default ``2.24.0``.
#[pyfunction]
#[pyo3(signature = (doc, path=None, *, version=None))]
fn to_lilypond_music(
    doc: &PyMusicDocument,
    path: Option<&str>,
    version: Option<&Bound<'_, PyAny>>,
) -> PyResult<String> {
    let version = version.map(version_arg).transpose()?;
    guard(|| {
        let mut adapter = adapters::ir_to_ly::IrToLyAdapter::new();
        if let Some(v) = &version {
            adapter = adapter.with_version(&v.to_string());
        }
        let output = adapter
            .convert_music(&doc.inner)
            .map_err(|e| PyValueError::new_err(e.to_string()))?;
        if let Some(p) = path {
            std::fs::write(p, &output).map_err(|e| PyIOError::new_err(e.to_string()))?;
        }
        Ok(output)
    })
}

/// Emit a :class:`Score` as a LilyPond string.  If *path* is given the result
/// is also written to that file.
///
/// *relative* chooses the pitch entry: ``True`` for ``\\relative`` octave marks,
/// ``False`` for absolute ones, ``None`` (default) for the score's own.
/// *version* (a :class:`LilyPondVersion` or a string) is the ``\\version``
/// written; by default ``2.24.0``, the syntax lytk writes, whatever version
/// the score was read from.
#[pyfunction]
#[pyo3(signature = (score, path=None, *, language=None, relative=None, version=None))]
fn to_lilypond(
    score: &PyScore,
    path: Option<&str>,
    language: Option<&str>,
    relative: Option<bool>,
    version: Option<&Bound<'_, PyAny>>,
) -> PyResult<String> {
    let version = version.map(version_arg).transpose()?;
    guard(|| {
        let mut adapter = adapters::ir_to_ly::IrToLyAdapter::new();
        if let Some(v) = &version {
            adapter = adapter.with_version(&v.to_string());
        }
        if let Some(lang_str) = language {
            adapter = adapter.with_language(parse_language(lang_str)?);
        } else if let Some(lang) = score.inner.metadata.pitch_language {
            adapter = adapter.with_language(lang);
        }
        let output = match relative {
            None => adapter.convert(&score.inner),
            Some(relative) => {
                let mut score = score.inner.clone();
                score.metadata.pitch_mode = if relative {
                    PitchMode::Relative
                } else {
                    PitchMode::Absolute
                };
                adapter.convert(&score)
            }
        }
        .map_err(|e| PyValueError::new_err(e.to_string()))?;
        if let Some(p) = path {
            std::fs::write(p, &output).map_err(|e| PyIOError::new_err(e.to_string()))?;
        }
        Ok(output)
    })
}

/// Emit a :class:`Score` as a MusicXML string.  If *path* is given the result
/// is also written to that file.
#[pyfunction]
#[pyo3(signature = (score, path=None))]
fn to_musicxml(py: Python<'_>, score: &PyScore, path: Option<&str>) -> PyResult<String> {
    guard(|| {
        let adapter = adapters::ir_to_mxml::IrToMxmlAdapter::new();
        let output = py
            .allow_threads(|| adapter.convert(&score.inner))
            .map_err(|e| PyValueError::new_err(e.to_string()))?;
        if let Some(p) = path {
            if p.to_ascii_lowercase().ends_with(".mxl") {
                // Compressed MXL, not plain XML in a misnamed file.
                adapter
                    .write(&score.inner, Path::new(p))
                    .map_err(|e| PyIOError::new_err(e.to_string()))?;
            } else {
                std::fs::write(p, &output).map_err(|e| PyIOError::new_err(e.to_string()))?;
            }
        }
        Ok(output)
    })
}

/// Recursively expand ``\include`` directives in a LilyPond file, returning the
/// flattened source.  If *output* is given the result is also written there.
///
/// *include_paths* are extra directories searched for includes; pass
/// ``add_markers=False`` to suppress the ``% === BEGIN/END INCLUDE ===``
/// comments.
#[pyfunction]
#[pyo3(signature = (input, output=None, *, include_paths=None, add_markers=true))]
fn flatten(
    input: &str,
    output: Option<&str>,
    include_paths: Option<Vec<String>>,
    add_markers: bool,
) -> PyResult<String> {
    guard(|| {
        let opts = adapters::ly_flatten::FlattenOpts {
            include_paths: include_paths
                .unwrap_or_default()
                .into_iter()
                .map(std::path::PathBuf::from)
                .collect(),
            add_markers,
            ..Default::default()
        };
        let text = adapters::ly_flatten::flatten(Path::new(input), opts).map_err(flatten_error)?;
        if let Some(p) = output {
            std::fs::write(p, &text).map_err(|e| PyIOError::new_err(e.to_string()))?;
        }
        Ok(text)
    })
}

/// Expand the ``\\include`` directives of LilyPond *text*: relative ones
/// against *base_dir* (when given), then *include_paths*. Includes are found
/// on the parse tree (anywhere in a line, never in a comment or string).
/// Raises :class:`ParseError` for a file not found (LilyPond's own, such as
/// ``english.ly``, stay as they are), a circular include, or an expansion
/// past the bounds; ``OSError`` for a file that cannot be read.
#[pyfunction]
#[pyo3(signature = (text, *, base_dir=None, include_paths=None, add_markers=true))]
fn flatten_string(
    py: Python<'_>,
    text: &str,
    base_dir: Option<&str>,
    include_paths: Option<Vec<String>>,
    add_markers: bool,
) -> PyResult<String> {
    guard(|| {
        let opts = adapters::ly_flatten::FlattenOpts {
            include_paths: include_paths
                .unwrap_or_default()
                .into_iter()
                .map(PathBuf::from)
                .collect(),
            add_markers,
            ..Default::default()
        };
        py.allow_threads(|| adapters::ly_flatten::flatten_str(text, base_dir.map(Path::new), opts))
            .map_err(flatten_error)
    })
}

/// A flatten failure: ``OSError`` for a file that cannot be read, else
/// :class:`ParseError`.
fn flatten_error(e: adapters::ly_flatten::FlattenError) -> PyErr {
    match e {
        adapters::ly_flatten::FlattenError::Io { .. } => PyIOError::new_err(e.to_string()),
        e => parse_error(e.to_string()),
    }
}

/// Parse an ABC notation (``.abc``) file into a :class:`Score`.
#[pyfunction]
fn from_abc(path: &str) -> PyResult<PyScore> {
    guard(|| {
        let adapter = adapters::abc_to_ir::AbcToIrAdapter::new();
        let score = adapter.convert_file(Path::new(path)).map_err(read_err)?;
        Ok(PyScore::from(score))
    })
}

/// Parse every tune of an ABC file: one :class:`Score` per ``X:`` tune (the
/// text before the first ``X:`` applies to all of them).
#[pyfunction]
fn from_abc_tunes(py: Python<'_>, path: &str) -> PyResult<Vec<PyScore>> {
    guard(|| {
        let adapter = adapters::abc_to_ir::AbcToIrAdapter::new();
        let scores = py
            .allow_threads(|| adapter.convert_file_tunes(Path::new(path)))
            .map_err(read_err)?;
        Ok(scores.into_iter().map(PyScore::from).collect())
    })
}

/// Parse an ABC notation string into a :class:`Score`.
#[pyfunction]
fn from_abc_string(text: &str) -> PyResult<PyScore> {
    guard(|| {
        let adapter = adapters::abc_to_ir::AbcToIrAdapter::new();
        let score = adapter.convert_str(text).map_err(read_err)?;
        Ok(PyScore::from(score))
    })
}

/// Emit a :class:`Score` as an ABC notation string.  If *path* is given the
/// result is also written to that file.  (ABC emission goes through the Layer-1
/// Music tree, so the score is lifted internally.)
#[pyfunction]
#[pyo3(signature = (score, path=None))]
fn to_abc(score: &PyScore, path: Option<&str>) -> PyResult<String> {
    guard(|| {
        let doc = ir::lift::lift_to_music(&score.inner);
        let adapter = adapters::ir_to_abc::IrToAbcAdapter::new();
        let output = adapter
            .convert_music(&doc)
            .map_err(|e| PyValueError::new_err(e.to_string()))?;
        if let Some(p) = path {
            std::fs::write(p, &output).map_err(|e| PyIOError::new_err(e.to_string()))?;
        }
        Ok(output)
    })
}

/// Parse a Humdrum (``**kern``) file into a :class:`Score`.
#[pyfunction]
fn from_humdrum(py: Python<'_>, path: &str) -> PyResult<PyScore> {
    guard(|| {
        let adapter = adapters::humdrum_to_ir::HumdrumToIrAdapter::new();
        let score = py
            .allow_threads(|| adapter.convert_file(Path::new(path)))
            .map_err(read_err)?;
        Ok(PyScore::from(score))
    })
}

/// Parse a Humdrum (``**kern``) string into a :class:`Score`.
#[pyfunction]
fn from_humdrum_string(text: &str) -> PyResult<PyScore> {
    guard(|| {
        let adapter = adapters::humdrum_to_ir::HumdrumToIrAdapter::new();
        let score = adapter.convert_str(text).map_err(read_err)?;
        Ok(PyScore::from(score))
    })
}

/// Emit a :class:`Score` as a Humdrum ``**kern`` string.  If *path* is given
/// the result is also written to that file.
#[pyfunction]
#[pyo3(signature = (score, path=None))]
fn to_humdrum(py: Python<'_>, score: &PyScore, path: Option<&str>) -> PyResult<String> {
    guard(|| {
        let adapter = adapters::ir_to_humdrum::IrToHumdrumAdapter::new();
        let output = py
            .allow_threads(|| adapter.convert(&score.inner))
            .map_err(|e| PyValueError::new_err(e.to_string()))?;
        if let Some(p) = path {
            std::fs::write(p, &output).map_err(|e| PyIOError::new_err(e.to_string()))?;
        }
        Ok(output)
    })
}

/// Parse a Standard MIDI File into a :class:`Score`. ``quantize`` (4, 8, 16
/// or 32) is the shortest plain note value a played file is snapped to;
/// ``swing`` reads swung eighths as straight ones marked "Swing" (``True``),
/// never (``False``), or when a played file swings (``None``).
#[pyfunction]
#[pyo3(signature = (path, *, quantize=None, swing=None))]
fn from_midi(path: &str, quantize: Option<u32>, swing: Option<bool>) -> PyResult<PyScore> {
    guard(|| {
        let bytes = std::fs::read(path).map_err(|e| PyIOError::new_err(e.to_string()))?;
        from_midi_bytes(&bytes, quantize, swing)
    })
}

/// Parse a Standard MIDI File from in-memory ``bytes`` into a :class:`Score`
/// (no temp file needed — for archives, HTTP responses, dataset buffers).
#[pyfunction]
#[pyo3(signature = (data, *, quantize=None, swing=None))]
fn from_midi_bytes(data: &[u8], quantize: Option<u32>, swing: Option<bool>) -> PyResult<PyScore> {
    guard(|| {
        let adapter = adapters::midi_to_ir::MidiToIrAdapter::new()
            .with_quantize(quantize)
            .with_swing(swing);
        let score = adapter.convert_bytes(data).map_err(read_err)?;
        Ok(PyScore::from(score))
    })
}

/// Parse MusicXML or compressed MXL from in-memory ``bytes`` into a
/// :class:`Score` (auto-detects `.mxl` vs plain XML; no temp file needed).
#[pyfunction]
fn from_musicxml_bytes(data: &[u8]) -> PyResult<PyScore> {
    guard(|| {
        let adapter = adapters::mxml_to_ir::MxmlToIrAdapter::new();
        let score = adapter.convert_bytes(data).map_err(read_err)?;
        Ok(PyScore::from(score))
    })
}

/// Write a :class:`Score` to a Standard MIDI File. Repeats are played out
/// with their endings unless ``unfold_repeats=False``.
#[pyfunction]
#[pyo3(signature = (score, path, *, unfold_repeats=true))]
fn to_midi(score: &PyScore, path: &str, unfold_repeats: bool) -> PyResult<()> {
    guard(|| {
        let adapter =
            adapters::ir_to_midi::IrToMidiAdapter::new().with_unfold_repeats(unfold_repeats);
        adapter
            .write(&score.inner, Path::new(path))
            .map_err(adapter_err)?;
        Ok(())
    })
}

/// Serialize a :class:`Score` to compressed MusicXML (``.mxl``) ``bytes`` — a
/// ZIP archive, the in-memory counterpart of ``to_musicxml(score, "x.mxl")``.
#[pyfunction]
fn to_mxl_bytes<'py>(py: Python<'py>, score: &PyScore) -> PyResult<Bound<'py, PyBytes>> {
    guard(|| {
        let adapter = adapters::ir_to_mxml::IrToMxmlAdapter::new();
        let bytes = py
            .allow_threads(|| adapter.convert_mxl_bytes(&score.inner))
            .map_err(adapter_err)?;
        Ok(PyBytes::new_bound(py, &bytes))
    })
}

/// Serialize a :class:`Score` to Standard MIDI File ``bytes`` (the in-memory
/// counterpart of :func:`to_midi`, which writes to a path).
#[pyfunction]
#[pyo3(signature = (score, *, unfold_repeats=true))]
fn to_midi_bytes<'py>(
    py: Python<'py>,
    score: &PyScore,
    unfold_repeats: bool,
) -> PyResult<Bound<'py, PyBytes>> {
    guard(|| {
        let adapter =
            adapters::ir_to_midi::IrToMidiAdapter::new().with_unfold_repeats(unfold_repeats);
        let bytes = adapter.convert_bytes(&score.inner).map_err(adapter_err)?;
        Ok(PyBytes::new_bound(py, &bytes))
    })
}

// ---------------------------------------------------------------------------
// Transform functions
// ---------------------------------------------------------------------------

/// Dispatch a transform over either a :class:`Score` (Layer 2) or a
/// :class:`MusicDocument` (Layer 1), returning the same type that came in —
/// Layer-1 pipelines keep contexts/relative structure without a lossy
/// round-trip through the measure-based Score.
fn dispatch_transform(
    py: Python<'_>,
    music: &Bound<'_, PyAny>,
    on_score: impl Fn(&Score) -> Score + Sync,
    on_music: impl Fn(&MusicDocument) -> MusicDocument + Sync,
) -> PyResult<PyObject> {
    if let Ok(s) = music.extract::<PyRef<PyScore>>() {
        let score: &Score = &s.inner;
        let inner = py.allow_threads(|| on_score(score));
        return Ok(PyScore::from(inner).into_py(py));
    }
    if let Ok(d) = music.extract::<PyRef<PyMusicDocument>>() {
        let doc: &MusicDocument = &d.inner;
        let inner = py.allow_threads(|| on_music(doc));
        return Ok(PyMusicDocument::from(inner).into_py(py));
    }
    Err(PyValueError::new_err(
        "expected a Score or MusicDocument as first argument",
    ))
}

/// Transpose all pitches by *semitones* (positive = up, negative = down).
/// Accepts a :class:`Score` or :class:`MusicDocument` and returns a new value
/// of the same type; the original is not modified.
#[pyfunction]
fn transpose(py: Python<'_>, music: &Bound<'_, PyAny>, semitones: i32) -> PyResult<PyObject> {
    guard(|| {
        if !(-127..=127).contains(&semitones) {
            return Err(PyValueError::new_err(format!(
                "transpose by {semitones} semitones: at most 127 either way"
            )));
        }
        dispatch_transform(
            py,
            music,
            |s| transforms::transpose::transpose(s, semitones),
            |d| transforms::transpose::transpose_music(d, semitones),
        )
    })
}

/// Transpose all pitches by a named diatonic *interval* (e.g. ``"M3"``, ``"m3"``,
/// ``"P5"``, ``"A4"``, ``"-m2"``), preserving correct enharmonic spelling.
/// Accepts a :class:`Score` or :class:`MusicDocument`; returns the same type.
/// Raises :class:`ValueError` on an invalid name.
#[pyfunction]
fn transpose_interval(
    py: Python<'_>,
    music: &Bound<'_, PyAny>,
    interval: &str,
) -> PyResult<PyObject> {
    guard(|| {
        let iv = Interval::from_name(interval).map_err(PyValueError::new_err)?;
        dispatch_transform(
            py,
            music,
            |s| transforms::transpose::transpose_interval(s, iv),
            |d| transforms::transpose::transpose_interval_music(d, iv),
        )
    })
}

/// Transpose so the piece's tonic becomes *key* (e.g. ``"D"``, ``"Bb"``,
/// ``"F#"``), choosing the nearest direction. Accepts a :class:`Score` or
/// :class:`MusicDocument`; returns the same type.
#[pyfunction]
fn transpose_to_key(py: Python<'_>, music: &Bound<'_, PyAny>, key: &str) -> PyResult<PyObject> {
    guard(|| {
        let tonic = _core_parse_tonic(key).map_err(PyValueError::new_err)?;
        dispatch_transform(
            py,
            music,
            |s| transforms::transpose::transpose_to_key(s, tonic),
            |d| transforms::transpose::transpose_to_key_music(d, tonic),
        )
    })
}

use crate::ir::pitch::parse_tonic as _core_parse_tonic;

/// Change the LilyPond pitch language (e.g. ``"english"``, ``"deutsch"``).
/// Accepts a :class:`Score` or :class:`MusicDocument`; returns the same type.
#[pyfunction]
fn change_language(py: Python<'_>, music: &Bound<'_, PyAny>, language: &str) -> PyResult<PyObject> {
    guard(|| {
        let lang = parse_language(language)?;
        dispatch_transform(
            py,
            music,
            |s| transforms::language::change_language(s, lang),
            |d| transforms::language::change_language_music(d, lang),
        )
    })
}

/// Invert intervals around an axis pitch.  Returns a new :class:`Score`.
///
/// Parameters
/// ----------
/// step : str
///     Diatonic step name, e.g. ``"C"``, ``"D"``.
/// alter : int
///     Chromatic alteration in semitones (0 = natural, 1 = sharp, -1 = flat).
/// octave : int
///     Octave number (middle C = 4).
#[pyfunction]
#[pyo3(signature = (music, *, step="C", alter=0, octave=4))]
fn invert(
    py: Python<'_>,
    music: &Bound<'_, PyAny>,
    step: &str,
    alter: i32,
    octave: i32,
) -> PyResult<PyObject> {
    guard(|| {
        let s = PitchStep::from_name(step)
            .ok_or_else(|| PyValueError::new_err(format!("invalid step: {step}")))?;
        if !(-128..=127).contains(&octave) || !(-4..=4).contains(&alter) {
            return Err(PyValueError::new_err(format!(
                "invert axis out of range: octave {octave} (-128..=127), alter {alter} (-4..=4)"
            )));
        }
        let axis = Pitch::with_alter(s, Alter::from_integer(alter), octave);
        dispatch_transform(
            py,
            music,
            |s| transforms::invert::invert(s, axis),
            |d| transforms::invert::invert_music(d, axis),
        )
    })
}

/// Reverse the music in time. Accepts a :class:`Score` or
/// :class:`MusicDocument`; returns the same type.
#[pyfunction]
fn retrograde(py: Python<'_>, music: &Bound<'_, PyAny>) -> PyResult<PyObject> {
    guard(|| {
        dispatch_transform(
            py,
            music,
            transforms::retrograde::retrograde,
            transforms::retrograde::retrograde_music,
        )
    })
}

// ---------------------------------------------------------------------------
// ML representations (Epic D) — numpy interop
// ---------------------------------------------------------------------------

use numpy::ndarray::{Array1, Array2};
use numpy::{IntoPyArray, PyArray1, PyArray2, PyReadonlyArray1, PyReadonlyArray2};
use representations::event_sequence::{EventOptions, EventSequence};
use representations::note_array::{NoteArray, NoteRow};
use representations::piano_roll::{PianoRoll, PITCH_COUNT};

/// Encode a :class:`MusicDocument` as a note-based array of shape ``(N, 4)``
/// with integer columns ``(onset, duration, pitch, velocity)`` in time steps
/// (``resolution`` = steps per quarter note).
#[pyfunction]
#[pyo3(signature = (doc, resolution=representations::note_array::DEFAULT_RESOLUTION))]
fn to_note_array<'py>(
    py: Python<'py>,
    doc: &PyMusicDocument,
    resolution: u16,
) -> PyResult<Bound<'py, PyArray2<i32>>> {
    guard(|| {
        Ok({
            let arr = representations::to_note_array(&doc.inner, resolution);
            let n = arr.notes.len();
            let mut data = Array2::<i32>::zeros((n, 4));
            for (i, row) in arr.notes.iter().enumerate() {
                data[[i, 0]] = row.onset as i32;
                data[[i, 1]] = row.duration as i32;
                data[[i, 2]] = row.pitch as i32;
                data[[i, 3]] = row.velocity as i32;
            }
            data.into_pyarray_bound(py)
        })
    })
}

/// Decode a note-based array of shape ``(N, 4)`` back into a
/// :class:`MusicDocument`.
#[pyfunction]
#[pyo3(signature = (array, resolution=representations::note_array::DEFAULT_RESOLUTION))]
fn from_note_array(array: PyReadonlyArray2<i32>, resolution: u16) -> PyResult<PyMusicDocument> {
    guard(|| {
        let view = array.as_array();
        if view.ncols() != 4 {
            return Err(PyValueError::new_err(
                "note array must have shape (N, 4): (onset, duration, pitch, velocity)",
            ));
        }
        let notes = view
            .rows()
            .into_iter()
            .map(|r| NoteRow {
                onset: r[0].max(0) as u32,
                duration: r[1].max(0) as u32,
                pitch: r[2].clamp(0, 127) as u8,
                velocity: r[3].clamp(0, 127) as u8,
            })
            .collect();
        let na = NoteArray { resolution, notes };
        Ok(PyMusicDocument::from(representations::from_note_array(&na)))
    })
}

/// Encode a :class:`MusicDocument` as an event sequence (1-D array of event
/// codes). See :mod:`lytk` docs for the vocabulary layout.
#[pyfunction]
#[pyo3(signature = (
    doc,
    resolution=representations::note_array::DEFAULT_RESOLUTION,
    max_time_shift=representations::DEFAULT_MAX_TIME_SHIFT,
    velocity_bins=representations::DEFAULT_VELOCITY_BINS,
    encode_velocity=true,
))]
fn to_event_sequence<'py>(
    py: Python<'py>,
    doc: &PyMusicDocument,
    resolution: u16,
    max_time_shift: u32,
    velocity_bins: u8,
    encode_velocity: bool,
) -> PyResult<Bound<'py, PyArray1<i64>>> {
    guard(|| {
        Ok({
            let arr = representations::to_note_array(&doc.inner, resolution);
            let opts = EventOptions {
                max_time_shift,
                velocity_bins,
                encode_velocity,
            };
            let seq = representations::to_event_sequence(&arr, &opts);
            let codes: Vec<i64> = seq.codes.iter().map(|&c| c as i64).collect();
            Array1::from(codes).into_pyarray_bound(py)
        })
    })
}

/// Decode an event-sequence array back into a :class:`MusicDocument`.
#[pyfunction]
#[pyo3(signature = (
    array,
    resolution=representations::note_array::DEFAULT_RESOLUTION,
    max_time_shift=representations::DEFAULT_MAX_TIME_SHIFT,
    velocity_bins=representations::DEFAULT_VELOCITY_BINS,
    encode_velocity=true,
))]
fn from_event_sequence(
    array: PyReadonlyArray1<i64>,
    resolution: u16,
    max_time_shift: u32,
    velocity_bins: u8,
    encode_velocity: bool,
) -> PyResult<PyMusicDocument> {
    guard(|| {
        Ok({
            let codes: Vec<u32> = array.as_array().iter().map(|&c| c.max(0) as u32).collect();
            let seq = EventSequence {
                codes,
                resolution,
                max_time_shift,
                velocity_bins,
                encode_velocity,
            };
            let na = representations::from_event_sequence(&seq);
            PyMusicDocument::from(representations::from_note_array(&na))
        })
    })
}

/// Encode a :class:`MusicDocument` as a piano-roll matrix of shape
/// ``(T, 128)`` (uint8; velocity-valued, or 0/1 when *encode_velocity* is
/// false).
#[pyfunction]
#[pyo3(signature = (doc, resolution=representations::note_array::DEFAULT_RESOLUTION, encode_velocity=true))]
fn to_piano_roll<'py>(
    py: Python<'py>,
    doc: &PyMusicDocument,
    resolution: u16,
    encode_velocity: bool,
) -> PyResult<Bound<'py, PyArray2<u8>>> {
    guard(|| {
        Ok({
            let arr = representations::to_note_array(&doc.inner, resolution);
            let pr = representations::to_piano_roll(&arr, encode_velocity);
            let t = pr.num_steps as usize;
            // pr.data is already row-major (t, 128).
            let data = Array2::from_shape_vec((t, PITCH_COUNT), pr.data)
                .expect("piano-roll data length matches T*128");
            data.into_pyarray_bound(py)
        })
    })
}

/// Decode a piano-roll matrix of shape ``(T, 128)`` back into a
/// :class:`MusicDocument`.
#[pyfunction]
#[pyo3(signature = (array, resolution=representations::note_array::DEFAULT_RESOLUTION, encode_velocity=true))]
fn from_piano_roll(
    array: PyReadonlyArray2<u8>,
    resolution: u16,
    encode_velocity: bool,
) -> PyResult<PyMusicDocument> {
    guard(|| {
        let view = array.as_array();
        if view.ncols() != PITCH_COUNT {
            return Err(PyValueError::new_err("piano roll must have shape (T, 128)"));
        }
        let num_steps = view.nrows() as u32;
        let data: Vec<u8> = view.iter().copied().collect();
        let pr = PianoRoll {
            resolution,
            num_steps,
            encode_velocity,
            data,
        };
        let na = representations::from_piano_roll(&pr);
        Ok(PyMusicDocument::from(representations::from_note_array(&na)))
    })
}

/// Compute objective evaluation metrics for a :class:`MusicDocument`, returning
/// a dict of metric name → value (NaN where undefined, e.g. no notes).
///
/// ``resolution`` is the time steps per quarter note used for the internal
/// note-array; ``measure_resolution`` (steps per measure) governs the
/// measure-based metrics (defaults to ``4 * resolution``, i.e. a 4/4 bar).
#[pyfunction]
#[pyo3(signature = (
    doc,
    resolution=representations::note_array::DEFAULT_RESOLUTION,
    measure_resolution=None,
))]
fn compute_metrics<'py>(
    py: Python<'py>,
    doc: &PyMusicDocument,
    resolution: u16,
    measure_resolution: Option<u32>,
) -> PyResult<Bound<'py, pyo3::types::PyDict>> {
    guard(|| {
        use representations::metrics as mx;
        let arr = representations::to_note_array(&doc.inner, resolution);
        let mr = measure_resolution.unwrap_or(4 * resolution as u32).max(1);

        let dict = pyo3::types::PyDict::new_bound(py);
        dict.set_item("n_pitches_used", mx::n_pitches_used(&arr))?;
        dict.set_item("n_pitch_classes_used", mx::n_pitch_classes_used(&arr))?;
        dict.set_item("pitch_range", mx::pitch_range(&arr))?;
        dict.set_item(
            "pitch_class_histogram",
            mx::pitch_class_histogram(&arr).to_vec(),
        )?;
        dict.set_item("pitch_entropy", mx::pitch_entropy(&arr))?;
        dict.set_item("pitch_class_entropy", mx::pitch_class_entropy(&arr))?;
        dict.set_item("polyphony", mx::polyphony(&arr))?;
        dict.set_item("polyphony_rate", mx::polyphony_rate(&arr, 2))?;
        dict.set_item("empty_beat_rate", mx::empty_beat_rate(&arr))?;
        dict.set_item("scale_consistency", mx::scale_consistency(&arr))?;
        dict.set_item("groove_consistency", mx::groove_consistency(&arr, mr))?;
        Ok(dict)
    })
}

// ---------------------------------------------------------------------------
// Module registration
// ---------------------------------------------------------------------------

/// The compiled Rust extension module (``lytk._core``).
#[pymodule]
fn _core(m: &Bound<'_, PyModule>) -> PyResult<()> {
    install_panic_hook();
    m.add("__version__", env!("CARGO_PKG_VERSION"))?;
    m.add("LytkError", m.py().get_type_bound::<LytkError>())?;
    m.add("InternalError", m.py().get_type_bound::<InternalError>())?;
    m.add("ParseError", parse_error_type(m.py())?)?;
    m.add("LilyPondSyntaxError", syntax_error_type(m.py())?)?;
    m.add_class::<PyDiagnostic>()?;
    m.add_function(wrap_pyfunction!(check_lilypond, m)?)?;
    m.add_class::<PyHeaderField>()?;
    m.add_function(wrap_pyfunction!(header_fields, m)?)?;
    m.add_class::<PyLilyPondVersion>()?;
    m.add_function(wrap_pyfunction!(lilypond_version, m)?)?;
    m.add_function(wrap_pyfunction!(set_lilypond_version, m)?)?;
    m.add_function(wrap_pyfunction!(strip_lilypond_version, m)?)?;
    m.add_class::<PyToken>()?;
    m.add_function(wrap_pyfunction!(tokenize, m)?)?;
    m.add_function(wrap_pyfunction!(strip_comments, m)?)?;
    m.add_function(wrap_pyfunction!(source_stats, m)?)?;
    m.add_function(wrap_pyfunction!(info, m)?)?;
    m.add_function(wrap_pyfunction!(_panic_for_tests, m)?)?;

    // Score class
    m.add_class::<PyScore>()?;
    m.add_class::<PyMusicDocument>()?;

    // Structured note-navigation classes (Part / Measure / Voice / Note / …)
    navigation::register(m)?;

    // Adapter functions
    m.add_function(wrap_pyfunction!(from_musicxml, m)?)?;
    m.add_function(wrap_pyfunction!(from_musicxml_string, m)?)?;
    m.add_function(wrap_pyfunction!(from_musicxml_bytes, m)?)?;
    m.add_function(wrap_pyfunction!(from_lilypond, m)?)?;
    m.add_function(wrap_pyfunction!(from_lilypond_string, m)?)?;
    m.add_function(wrap_pyfunction!(from_lilypond_movements, m)?)?;
    m.add_function(wrap_pyfunction!(from_lilypond_music_movements, m)?)?;
    m.add_function(wrap_pyfunction!(from_lilypond_music, m)?)?;
    m.add_function(wrap_pyfunction!(from_lilypond_music_string, m)?)?;
    m.add_function(wrap_pyfunction!(to_lilypond, m)?)?;
    m.add_function(wrap_pyfunction!(to_lilypond_music, m)?)?;
    m.add_function(wrap_pyfunction!(flatten, m)?)?;
    m.add_function(wrap_pyfunction!(flatten_string, m)?)?;
    m.add_function(wrap_pyfunction!(to_musicxml, m)?)?;
    m.add_function(wrap_pyfunction!(to_mxl_bytes, m)?)?;
    m.add_function(wrap_pyfunction!(from_abc, m)?)?;
    m.add_function(wrap_pyfunction!(from_abc_string, m)?)?;
    m.add_function(wrap_pyfunction!(from_abc_tunes, m)?)?;
    m.add_function(wrap_pyfunction!(from_humdrum, m)?)?;
    m.add_function(wrap_pyfunction!(from_humdrum_string, m)?)?;
    m.add_function(wrap_pyfunction!(to_humdrum, m)?)?;
    m.add_function(wrap_pyfunction!(to_abc, m)?)?;

    m.add_function(wrap_pyfunction!(from_midi, m)?)?;
    m.add_function(wrap_pyfunction!(from_midi_bytes, m)?)?;
    m.add_function(wrap_pyfunction!(to_midi, m)?)?;
    m.add_function(wrap_pyfunction!(to_midi_bytes, m)?)?;

    // Transform functions
    m.add_function(wrap_pyfunction!(transpose, m)?)?;
    m.add_function(wrap_pyfunction!(transpose_interval, m)?)?;
    m.add_function(wrap_pyfunction!(transpose_to_key, m)?)?;
    m.add_function(wrap_pyfunction!(change_language, m)?)?;
    m.add_function(wrap_pyfunction!(invert, m)?)?;
    m.add_function(wrap_pyfunction!(retrograde, m)?)?;

    // ML representations (Epic D)
    m.add_function(wrap_pyfunction!(to_note_array, m)?)?;
    m.add_function(wrap_pyfunction!(from_note_array, m)?)?;
    m.add_function(wrap_pyfunction!(to_event_sequence, m)?)?;
    m.add_function(wrap_pyfunction!(from_event_sequence, m)?)?;
    m.add_function(wrap_pyfunction!(to_piano_roll, m)?)?;
    m.add_function(wrap_pyfunction!(from_piano_roll, m)?)?;
    m.add_function(wrap_pyfunction!(compute_metrics, m)?)?;

    Ok(())
}
