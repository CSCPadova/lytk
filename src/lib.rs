// pyo3 proc macros wrap return values with `Into::into()` on the error path,
// which clippy flags as a no-op when the error is already `PyErr`.
#![allow(clippy::useless_conversion)]

use std::path::Path;

use pyo3::exceptions::{PyIOError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::PyModule;

pub mod adapters;
pub mod ir;
pub mod parser;
pub mod transforms;

use adapters::{FromIrAdapter, ToIrAdapter};
use ir::language::PitchLanguage;
use ir::pitch::{Alter, Pitch, PitchStep};
use ir::Score;

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

    /// Serialize the full score IR to a JSON string.
    fn to_json(&self) -> PyResult<String> {
        serde_json::to_string_pretty(&self.inner)
            .map_err(|e| PyValueError::new_err(e.to_string()))
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
        let json_str = serde_json::to_string(&self.inner)
            .map_err(|e| PyValueError::new_err(e.to_string()))?;
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
// Adapter functions
// ---------------------------------------------------------------------------

/// Parse a MusicXML (``.xml``, ``.musicxml``) or compressed MXL file into a
/// :class:`Score`.
#[pyfunction]
fn from_musicxml(path: &str) -> PyResult<PyScore> {
    let adapter = adapters::mxml_to_ir::MxmlToIrAdapter::new();
    let score = adapter
        .convert_file(Path::new(path))
        .map_err(|e| PyIOError::new_err(e.to_string()))?;
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

/// Parse a LilyPond (``.ly``) file into a :class:`Score`.
#[pyfunction]
#[pyo3(signature = (path, *, language=None))]
fn from_lilypond(path: &str, language: Option<&str>) -> PyResult<PyScore> {
    let mut adapter = adapters::ly_to_ir::LyToIrAdapter::new();
    if let Some(lang_str) = language {
        let lang = PitchLanguage::from_str_loose(lang_str)
            .ok_or_else(|| PyValueError::new_err(format!("unknown language: {lang_str}")))?;
        adapter = adapter.with_language(lang);
    }
    let score = adapter
        .convert_file(Path::new(path))
        .map_err(|e| PyIOError::new_err(e.to_string()))?;
    Ok(PyScore { inner: score })
}

/// Parse a LilyPond string into a :class:`Score`.
#[pyfunction]
#[pyo3(signature = (text, *, language=None))]
fn from_lilypond_string(text: &str, language: Option<&str>) -> PyResult<PyScore> {
    let mut adapter = adapters::ly_to_ir::LyToIrAdapter::new();
    if let Some(lang_str) = language {
        let lang = PitchLanguage::from_str_loose(lang_str)
            .ok_or_else(|| PyValueError::new_err(format!("unknown language: {lang_str}")))?;
        adapter = adapter.with_language(lang);
    }
    let score = adapter
        .convert_str(text)
        .map_err(|e| PyValueError::new_err(e.to_string()))?;
    Ok(PyScore { inner: score })
}

/// Emit a :class:`Score` as a LilyPond string.  If *path* is given the result
/// is also written to that file.
#[pyfunction]
#[pyo3(signature = (score, path=None, *, language=None))]
fn to_lilypond(score: &PyScore, path: Option<&str>, language: Option<&str>) -> PyResult<String> {
    let mut adapter = adapters::ir_to_ly::IrToLyAdapter::new();
    if let Some(lang_str) = language {
        let lang = PitchLanguage::from_str_loose(lang_str)
            .ok_or_else(|| PyValueError::new_err(format!("unknown language: {lang_str}")))?;
        adapter = adapter.with_language(lang);
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
fn to_musicxml(score: &PyScore, path: Option<&str>) -> PyResult<String> {
    let adapter = adapters::ir_to_mxml::IrToMxmlAdapter::new();
    let output = adapter
        .convert(&score.inner)
        .map_err(|e| PyValueError::new_err(e.to_string()))?;
    if let Some(p) = path {
        std::fs::write(p, &output).map_err(|e| PyIOError::new_err(e.to_string()))?;
    }
    Ok(output)
}

/// Parse a Standard MIDI File into a :class:`Score`.
#[cfg(feature = "midi")]
#[pyfunction]
fn from_midi(path: &str) -> PyResult<PyScore> {
    let bytes = std::fs::read(path).map_err(|e| PyIOError::new_err(e.to_string()))?;
    let adapter = adapters::midi_to_ir::MidiToIrAdapter::new();
    let score = adapter
        .convert_bytes(&bytes)
        .map_err(|e| PyIOError::new_err(e.to_string()))?;
    Ok(PyScore { inner: score })
}

/// Write a :class:`Score` to a Standard MIDI File.
#[cfg(feature = "midi")]
#[pyfunction]
fn to_midi(score: &PyScore, path: &str) -> PyResult<()> {
    let adapter = adapters::ir_to_midi::IrToMidiAdapter::new();
    adapter
        .write(&score.inner, Path::new(path))
        .map_err(|e| PyIOError::new_err(e.to_string()))?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Transform functions
// ---------------------------------------------------------------------------

/// Transpose all pitches by *semitones* (positive = up, negative = down).
/// Returns a new :class:`Score`; the original is not modified.
#[pyfunction]
fn transpose(score: &PyScore, semitones: i32) -> PyScore {
    PyScore {
        inner: transforms::transpose::transpose(&score.inner, semitones),
    }
}

/// Change the LilyPond pitch language (e.g. ``"english"``, ``"deutsch"``).
/// Returns a new :class:`Score`.
#[pyfunction]
fn change_language(score: &PyScore, language: &str) -> PyResult<PyScore> {
    let lang = PitchLanguage::from_str_loose(language)
        .ok_or_else(|| PyValueError::new_err(format!("unknown language: {language}")))?;
    Ok(PyScore {
        inner: transforms::language::change_language(&score.inner, lang),
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
#[pyo3(signature = (score, *, step="C", alter=0, octave=4))]
fn invert(score: &PyScore, step: &str, alter: i32, octave: i32) -> PyResult<PyScore> {
    let s = PitchStep::from_name(step)
        .ok_or_else(|| PyValueError::new_err(format!("invalid step: {step}")))?;
    let axis = Pitch::with_alter(s, Alter::from_integer(alter), octave);
    Ok(PyScore {
        inner: transforms::invert::invert(&score.inner, axis),
    })
}

/// Reverse note order within each voice.  Returns a new :class:`Score`.
#[pyfunction]
fn retrograde(score: &PyScore) -> PyScore {
    PyScore {
        inner: transforms::retrograde::retrograde(&score.inner),
    }
}

// ---------------------------------------------------------------------------
// Module registration
// ---------------------------------------------------------------------------

/// The compiled Rust extension module (``lytk._core``).
#[pymodule]
fn _core(m: &Bound<'_, PyModule>) -> PyResult<()> {
    // Score class
    m.add_class::<PyScore>()?;

    // Adapter functions
    m.add_function(wrap_pyfunction!(from_musicxml, m)?)?;
    m.add_function(wrap_pyfunction!(from_musicxml_string, m)?)?;
    m.add_function(wrap_pyfunction!(from_lilypond, m)?)?;
    m.add_function(wrap_pyfunction!(from_lilypond_string, m)?)?;
    m.add_function(wrap_pyfunction!(to_lilypond, m)?)?;
    m.add_function(wrap_pyfunction!(to_musicxml, m)?)?;

    #[cfg(feature = "midi")]
    {
        m.add_function(wrap_pyfunction!(from_midi, m)?)?;
        m.add_function(wrap_pyfunction!(to_midi, m)?)?;
    }

    // Transform functions
    m.add_function(wrap_pyfunction!(transpose, m)?)?;
    m.add_function(wrap_pyfunction!(change_language, m)?)?;
    m.add_function(wrap_pyfunction!(invert, m)?)?;
    m.add_function(wrap_pyfunction!(retrograde, m)?)?;

    Ok(())
}
