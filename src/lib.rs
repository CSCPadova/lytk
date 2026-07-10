//! # lytk — fast symbolic-music conversion & augmentation
//!
//! lytk converts between **LilyPond**, **MusicXML/MXL**, **MIDI** and **ABC**
//! through a shared Internal Representation (IR), applies composable transforms
//! (transpose, invert, retrograde, language change), and emits ML
//! representations (note-array, event-sequence, piano-roll). It ships as a Rust
//! crate plus a CLI and Python bindings (the `_core` extension module).
//!
//! ## Architecture
//! - [`ir`] — the IR: a measure-based [`Score`] (Layer 2) and a recursive
//!   [`MusicDocument`] music tree (Layer 1), bridged by lift/lower.
//! - [`adapters`] — per-format readers/writers ([`ToIrAdapter`]/[`FromIrAdapter`]).
//! - [`transforms`] — composable IR passes.
//! - [`representations`] — note-array / event-sequence / piano-roll encoders.
//!
//! ## Rust quickstart
//! ```no_run
//! use _core::adapters::mxml_to_ir::MxmlToIrAdapter;
//! use _core::adapters::ir_to_ly::IrToLyAdapter;
//! use _core::adapters::{ToIrAdapter, FromIrAdapter};
//!
//! let score = MxmlToIrAdapter::new().convert_file("in.musicxml".as_ref())?;
//! let lilypond = IrToLyAdapter::new().convert(&score)?;
//! # Ok::<(), _core::adapters::AdapterError>(())
//! ```
//!
//! The most-used types are re-exported at the crate root: [`Score`] and
//! [`MusicDocument`].

// pyo3 proc macros wrap return values with `Into::into()` on the error path,
// which clippy flags as a no-op when the error is already `PyErr`.
#![allow(clippy::useless_conversion)]

use std::path::Path;

use pyo3::exceptions::{PyIOError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyBytes, PyModule};

pub mod adapters;
pub mod ir;
mod navigation;
pub mod parser;
pub mod representations;
pub mod transforms;

use adapters::{FromIrAdapter, FromMusicAdapter, ToIrAdapter, ToMusicAdapter};
use ir::interval::Interval;
use ir::language::PitchLanguage;
use ir::pitch::{Alter, Pitch, PitchStep};

// Re-export the core IR types at the crate root for Rust consumers (also used
// internally by the bindings below).
pub use ir::music::MusicDocument;
pub use ir::Score;

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
}

#[pymethods]
impl PyScore {
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
    fn iter_parts(&self) -> Vec<navigation::PyPart> {
        self.inner
            .parts()
            .iter()
            .map(|p| navigation::PyPart::from_ir(p))
            .collect()
    }

    /// Serialize the full score IR to a JSON string.
    fn to_json(&self) -> PyResult<String> {
        serde_json::to_string_pretty(&self.inner).map_err(|e| PyValueError::new_err(e.to_string()))
    }

    /// Deserialize a score from a JSON string.
    #[staticmethod]
    fn from_json(json: &str) -> PyResult<Self> {
        let score: Score =
            serde_json::from_str(json).map_err(|e| PyValueError::new_err(e.to_string()))?;
        Ok(PyScore { inner: score })
    }

    /// Serialize the score IR to a Python dict.
    fn to_dict<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let json_str =
            serde_json::to_string(&self.inner).map_err(|e| PyValueError::new_err(e.to_string()))?;
        let json_mod = PyModule::import_bound(py, "json")?;
        json_mod.call_method1("loads", (json_str,))
    }

    /// Deserialize a score from a Python dict.
    #[staticmethod]
    fn from_dict(dict: &Bound<'_, PyAny>) -> PyResult<Self> {
        let py = dict.py();
        let json_mod = PyModule::import_bound(py, "json")?;
        let json_str: String = json_mod.call_method1("dumps", (dict,))?.extract()?;
        let score: Score =
            serde_json::from_str(&json_str).map_err(|e| PyValueError::new_err(e.to_string()))?;
        Ok(PyScore { inner: score })
    }

    /// Lift this measure-based :class:`Score` to a Layer-1 :class:`MusicDocument`
    /// (the form the ML representations consume).
    fn to_music_document(&self) -> PyMusicDocument {
        PyMusicDocument {
            inner: ir::lift::lift_to_music(&self.inner),
        }
    }

    /// The score's notes as ``(onset, duration, pitch, velocity)`` tuples in time
    /// steps (``resolution`` = steps per quarter note). A lightweight, numpy-free
    /// way to iterate notes directly, without going through
    /// :func:`to_note_array` or hand-walking :meth:`to_dict`.
    #[pyo3(signature = (resolution = representations::note_array::DEFAULT_RESOLUTION))]
    fn notes(&self, resolution: u16) -> Vec<(u32, u32, u8, u8)> {
        let doc = ir::lift::lift_to_music(&self.inner);
        representations::to_note_array(&doc, resolution)
            .notes
            .iter()
            .map(|n| (n.onset, n.duration, n.pitch, n.velocity))
            .collect()
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
}

#[pymethods]
impl PyMusicDocument {
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
    fn notes(&self, resolution: u16) -> Vec<(u32, u32, u8, u8)> {
        representations::to_note_array(&self.inner, resolution)
            .notes
            .iter()
            .map(|n| (n.onset, n.duration, n.pitch, n.velocity))
            .collect()
    }

    /// Serialize the music document to a JSON string.
    fn to_json(&self) -> PyResult<String> {
        serde_json::to_string_pretty(&self.inner).map_err(|e| PyValueError::new_err(e.to_string()))
    }

    /// Deserialize a music document from a JSON string.
    #[staticmethod]
    fn from_json(json: &str) -> PyResult<Self> {
        let doc: MusicDocument =
            serde_json::from_str(json).map_err(|e| PyValueError::new_err(e.to_string()))?;
        Ok(PyMusicDocument { inner: doc })
    }

    /// Convert this music document to a measure-based :class:`Score`.
    fn to_score(&self) -> PyScore {
        PyScore {
            inner: ir::lower::lower_to_score(&self.inner),
        }
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

/// Map an adapter error to the appropriate Python exception: an I/O failure
/// becomes `IOError`, every parse/validation failure becomes `ValueError`.
///
/// Previously the file readers mapped *all* adapter errors to `IOError`, so a
/// malformed-but-readable file looked the same as a missing one — a batch loader
/// wrapping reads in `except IOError` would silently swallow corrupt files.
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
    let adapter = adapters::mxml_to_ir::MxmlToIrAdapter::new();
    let score = py
        .allow_threads(|| adapter.convert_file(Path::new(path)))
        .map_err(adapter_err)?;
    Ok(PyScore { inner: score })
}

/// Parse a MusicXML string into a :class:`Score`.
#[pyfunction]
fn from_musicxml_string(xml: &str) -> PyResult<PyScore> {
    let adapter = adapters::mxml_to_ir::MxmlToIrAdapter::new();
    let score = adapter
        .convert_str(xml)
        .map_err(|e| PyValueError::new_err(e.to_string()))?;
    Ok(PyScore { inner: score })
}

/// Resolve a pitch-language name, raising a `ValueError` for unknown names.
fn parse_language(name: &str) -> PyResult<PitchLanguage> {
    PitchLanguage::from_str_loose(name)
        .ok_or_else(|| PyValueError::new_err(format!("unknown language: {name}")))
}

/// Parse a LilyPond (``.ly``) file into a :class:`Score`.
#[pyfunction]
#[pyo3(signature = (path, *, language=None))]
fn from_lilypond(py: Python<'_>, path: &str, language: Option<&str>) -> PyResult<PyScore> {
    let mut adapter = adapters::ly_to_ir::LyToIrAdapter::new();
    if let Some(lang_str) = language {
        adapter = adapter.with_language(parse_language(lang_str)?);
    }
    let score = py
        .allow_threads(|| adapter.convert_file(Path::new(path)))
        .map_err(adapter_err)?;
    Ok(PyScore { inner: score })
}

/// Parse a LilyPond string into a :class:`Score`.
#[pyfunction]
#[pyo3(signature = (text, *, language=None))]
fn from_lilypond_string(text: &str, language: Option<&str>) -> PyResult<PyScore> {
    let mut adapter = adapters::ly_to_ir::LyToIrAdapter::new();
    if let Some(lang_str) = language {
        adapter = adapter.with_language(parse_language(lang_str)?);
    }
    let score = adapter
        .convert_str(text)
        .map_err(|e| PyValueError::new_err(e.to_string()))?;
    Ok(PyScore { inner: score })
}

/// Parse a LilyPond file into a :class:`MusicDocument` (Layer 1 Music tree).
///
/// This preserves structural information like contexts and simultaneous blocks.
#[pyfunction]
#[pyo3(signature = (path, *, language=None))]
fn from_lilypond_music(
    py: Python<'_>,
    path: &str,
    language: Option<&str>,
) -> PyResult<PyMusicDocument> {
    let mut adapter = adapters::ly_to_ir::LyToIrAdapter::new();
    if let Some(lang_str) = language {
        adapter = adapter.with_language(parse_language(lang_str)?);
    }
    let doc = py
        .allow_threads(|| adapter.convert_file_to_music(Path::new(path)))
        .map_err(adapter_err)?;
    Ok(PyMusicDocument { inner: doc })
}

/// Parse a LilyPond string into a :class:`MusicDocument` (Layer 1 Music tree).
#[pyfunction]
#[pyo3(signature = (text, *, language=None))]
fn from_lilypond_music_string(text: &str, language: Option<&str>) -> PyResult<PyMusicDocument> {
    let mut adapter = adapters::ly_to_ir::LyToIrAdapter::new();
    if let Some(lang_str) = language {
        adapter = adapter.with_language(parse_language(lang_str)?);
    }
    let doc = adapter
        .convert_str_to_music(text)
        .map_err(|e| PyValueError::new_err(e.to_string()))?;
    Ok(PyMusicDocument { inner: doc })
}

/// Emit a :class:`MusicDocument` as a LilyPond string.  If *path* is given the
/// result is also written to that file.
#[pyfunction]
#[pyo3(signature = (doc, path=None))]
fn to_lilypond_music(doc: &PyMusicDocument, path: Option<&str>) -> PyResult<String> {
    let adapter = adapters::ir_to_ly::IrToLyAdapter::new();
    let output = adapter
        .convert_music(&doc.inner)
        .map_err(|e| PyValueError::new_err(e.to_string()))?;
    if let Some(p) = path {
        std::fs::write(p, &output).map_err(|e| PyIOError::new_err(e.to_string()))?;
    }
    Ok(output)
}

/// Emit a :class:`Score` as a LilyPond string.  If *path* is given the result
/// is also written to that file.
#[pyfunction]
#[pyo3(signature = (score, path=None, *, language=None))]
fn to_lilypond(score: &PyScore, path: Option<&str>, language: Option<&str>) -> PyResult<String> {
    let mut adapter = adapters::ir_to_ly::IrToLyAdapter::new();
    if let Some(lang_str) = language {
        adapter = adapter.with_language(parse_language(lang_str)?);
    } else if let Some(lang) = score.inner.metadata.pitch_language {
        adapter = adapter.with_language(lang);
    }
    let output = adapter
        .convert(&score.inner)
        .map_err(|e| PyValueError::new_err(e.to_string()))?;
    if let Some(p) = path {
        std::fs::write(p, &output).map_err(|e| PyIOError::new_err(e.to_string()))?;
    }
    Ok(output)
}

/// Emit a :class:`Score` as a MusicXML string.  If *path* is given the result
/// is also written to that file.
#[pyfunction]
#[pyo3(signature = (score, path=None))]
fn to_musicxml(py: Python<'_>, score: &PyScore, path: Option<&str>) -> PyResult<String> {
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
    let opts = adapters::ly_flatten::FlattenOpts {
        include_paths: include_paths
            .unwrap_or_default()
            .into_iter()
            .map(std::path::PathBuf::from)
            .collect(),
        add_markers,
    };
    let text = adapters::ly_flatten::flatten(Path::new(input), opts)
        .map_err(|e| PyValueError::new_err(e.to_string()))?;
    if let Some(p) = output {
        std::fs::write(p, &text).map_err(|e| PyIOError::new_err(e.to_string()))?;
    }
    Ok(text)
}

/// Parse an ABC notation (``.abc``) file into a :class:`Score`.
#[pyfunction]
fn from_abc(path: &str) -> PyResult<PyScore> {
    let adapter = adapters::abc_to_ir::AbcToIrAdapter::new();
    let score = adapter.convert_file(Path::new(path)).map_err(adapter_err)?;
    Ok(PyScore { inner: score })
}

/// Parse an ABC notation string into a :class:`Score`.
#[pyfunction]
fn from_abc_string(text: &str) -> PyResult<PyScore> {
    let adapter = adapters::abc_to_ir::AbcToIrAdapter::new();
    let score = adapter
        .convert_str(text)
        .map_err(|e| PyValueError::new_err(e.to_string()))?;
    Ok(PyScore { inner: score })
}

/// Emit a :class:`Score` as an ABC notation string.  If *path* is given the
/// result is also written to that file.  (ABC emission goes through the Layer-1
/// Music tree, so the score is lifted internally.)
#[pyfunction]
#[pyo3(signature = (score, path=None))]
fn to_abc(score: &PyScore, path: Option<&str>) -> PyResult<String> {
    let doc = ir::lift::lift_to_music(&score.inner);
    let adapter = adapters::ir_to_abc::IrToAbcAdapter::new();
    let output = adapter
        .convert_music(&doc)
        .map_err(|e| PyValueError::new_err(e.to_string()))?;
    if let Some(p) = path {
        std::fs::write(p, &output).map_err(|e| PyIOError::new_err(e.to_string()))?;
    }
    Ok(output)
}

/// Parse a Standard MIDI File into a :class:`Score`.
#[pyfunction]
fn from_midi(path: &str) -> PyResult<PyScore> {
    let bytes = std::fs::read(path).map_err(|e| PyIOError::new_err(e.to_string()))?;
    let adapter = adapters::midi_to_ir::MidiToIrAdapter::new();
    let score = adapter.convert_bytes(&bytes).map_err(adapter_err)?;
    Ok(PyScore { inner: score })
}

/// Parse a Standard MIDI File from in-memory ``bytes`` into a :class:`Score`
/// (no temp file needed — for archives, HTTP responses, dataset buffers).
#[pyfunction]
fn from_midi_bytes(data: &[u8]) -> PyResult<PyScore> {
    let adapter = adapters::midi_to_ir::MidiToIrAdapter::new();
    let score = adapter.convert_bytes(data).map_err(adapter_err)?;
    Ok(PyScore { inner: score })
}

/// Parse MusicXML or compressed MXL from in-memory ``bytes`` into a
/// :class:`Score` (auto-detects `.mxl` vs plain XML; no temp file needed).
#[pyfunction]
fn from_musicxml_bytes(data: &[u8]) -> PyResult<PyScore> {
    let adapter = adapters::mxml_to_ir::MxmlToIrAdapter::new();
    let score = adapter.convert_bytes(data).map_err(adapter_err)?;
    Ok(PyScore { inner: score })
}

/// Write a :class:`Score` to a Standard MIDI File.
#[pyfunction]
fn to_midi(score: &PyScore, path: &str) -> PyResult<()> {
    let adapter = adapters::ir_to_midi::IrToMidiAdapter::new();
    adapter
        .write(&score.inner, Path::new(path))
        .map_err(adapter_err)?;
    Ok(())
}

/// Serialize a :class:`Score` to Standard MIDI File ``bytes`` (the in-memory
/// counterpart of :func:`to_midi`, which writes to a path).
#[pyfunction]
fn to_midi_bytes<'py>(py: Python<'py>, score: &PyScore) -> PyResult<Bound<'py, PyBytes>> {
    let adapter = adapters::ir_to_midi::IrToMidiAdapter::new();
    let bytes = adapter.convert_bytes(&score.inner).map_err(adapter_err)?;
    Ok(PyBytes::new_bound(py, &bytes))
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
        return Ok(PyScore { inner }.into_py(py));
    }
    if let Ok(d) = music.extract::<PyRef<PyMusicDocument>>() {
        let doc: &MusicDocument = &d.inner;
        let inner = py.allow_threads(|| on_music(doc));
        return Ok(PyMusicDocument { inner }.into_py(py));
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
    dispatch_transform(
        py,
        music,
        |s| transforms::transpose::transpose(s, semitones),
        |d| transforms::transpose::transpose_music(d, semitones),
    )
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
    let iv = Interval::from_name(interval).map_err(PyValueError::new_err)?;
    dispatch_transform(
        py,
        music,
        |s| transforms::transpose::transpose_interval(s, iv),
        |d| transforms::transpose::transpose_interval_music(d, iv),
    )
}

/// Transpose so the piece's tonic becomes *key* (e.g. ``"D"``, ``"Bb"``,
/// ``"F#"``), choosing the nearest direction. Accepts a :class:`Score` or
/// :class:`MusicDocument`; returns the same type.
#[pyfunction]
fn transpose_to_key(py: Python<'_>, music: &Bound<'_, PyAny>, key: &str) -> PyResult<PyObject> {
    let tonic = _core_parse_tonic(key).map_err(PyValueError::new_err)?;
    dispatch_transform(
        py,
        music,
        |s| transforms::transpose::transpose_to_key(s, tonic),
        |d| transforms::transpose::transpose_to_key_music(d, tonic),
    )
}

use crate::ir::pitch::parse_tonic as _core_parse_tonic;

/// Change the LilyPond pitch language (e.g. ``"english"``, ``"deutsch"``).
/// Accepts a :class:`Score` or :class:`MusicDocument`; returns the same type.
#[pyfunction]
fn change_language(py: Python<'_>, music: &Bound<'_, PyAny>, language: &str) -> PyResult<PyObject> {
    let lang = parse_language(language)?;
    dispatch_transform(
        py,
        music,
        |s| transforms::language::change_language(s, lang),
        |d| transforms::language::change_language_music(d, lang),
    )
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
    let s = PitchStep::from_name(step)
        .ok_or_else(|| PyValueError::new_err(format!("invalid step: {step}")))?;
    let axis = Pitch::with_alter(s, Alter::from_integer(alter), octave);
    dispatch_transform(
        py,
        music,
        |s| transforms::invert::invert(s, axis),
        |d| transforms::invert::invert_music(d, axis),
    )
}

/// Reverse the music in time. Accepts a :class:`Score` or
/// :class:`MusicDocument`; returns the same type.
#[pyfunction]
fn retrograde(py: Python<'_>, music: &Bound<'_, PyAny>) -> PyResult<PyObject> {
    dispatch_transform(
        py,
        music,
        transforms::retrograde::retrograde,
        transforms::retrograde::retrograde_music,
    )
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
) -> Bound<'py, PyArray2<i32>> {
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
}

/// Decode a note-based array of shape ``(N, 4)`` back into a
/// :class:`MusicDocument`.
#[pyfunction]
#[pyo3(signature = (array, resolution=representations::note_array::DEFAULT_RESOLUTION))]
fn from_note_array(array: PyReadonlyArray2<i32>, resolution: u16) -> PyResult<PyMusicDocument> {
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
    Ok(PyMusicDocument {
        inner: representations::from_note_array(&na),
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
) -> Bound<'py, PyArray1<i64>> {
    let arr = representations::to_note_array(&doc.inner, resolution);
    let opts = EventOptions {
        max_time_shift,
        velocity_bins,
        encode_velocity,
    };
    let seq = representations::to_event_sequence(&arr, &opts);
    let codes: Vec<i64> = seq.codes.iter().map(|&c| c as i64).collect();
    Array1::from(codes).into_pyarray_bound(py)
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
) -> PyMusicDocument {
    let codes: Vec<u32> = array.as_array().iter().map(|&c| c.max(0) as u32).collect();
    let seq = EventSequence {
        codes,
        resolution,
        max_time_shift,
        velocity_bins,
        encode_velocity,
    };
    let na = representations::from_event_sequence(&seq);
    PyMusicDocument {
        inner: representations::from_note_array(&na),
    }
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
) -> Bound<'py, PyArray2<u8>> {
    let arr = representations::to_note_array(&doc.inner, resolution);
    let pr = representations::to_piano_roll(&arr, encode_velocity);
    let t = pr.num_steps as usize;
    // pr.data is already row-major (t, 128).
    let data = Array2::from_shape_vec((t, PITCH_COUNT), pr.data)
        .expect("piano-roll data length matches T*128");
    data.into_pyarray_bound(py)
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
    Ok(PyMusicDocument {
        inner: representations::from_note_array(&na),
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
}

// ---------------------------------------------------------------------------
// Module registration
// ---------------------------------------------------------------------------

/// The compiled Rust extension module (``lytk._core``).
#[pymodule]
fn _core(m: &Bound<'_, PyModule>) -> PyResult<()> {
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
    m.add_function(wrap_pyfunction!(from_lilypond_music, m)?)?;
    m.add_function(wrap_pyfunction!(from_lilypond_music_string, m)?)?;
    m.add_function(wrap_pyfunction!(to_lilypond, m)?)?;
    m.add_function(wrap_pyfunction!(to_lilypond_music, m)?)?;
    m.add_function(wrap_pyfunction!(flatten, m)?)?;
    m.add_function(wrap_pyfunction!(to_musicxml, m)?)?;
    m.add_function(wrap_pyfunction!(from_abc, m)?)?;
    m.add_function(wrap_pyfunction!(from_abc_string, m)?)?;
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
