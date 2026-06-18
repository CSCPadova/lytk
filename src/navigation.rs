//! Read-only **structured note navigation** for a Layer-2 [`Score`], exposed to
//! Python as typed, walkable objects.
//!
//! Complements the flat `Score.notes()` tuples and the `to_dict()` JSON blob
//! with a typed `Part → Measure → Voice → Note/Rest/Chord` tree (each note
//! carrying a [`Pitch`]). Callers can iterate measures and voices directly
//! instead of re-deriving structure from a dict:
//!
//! ```python
//! for part in score.iter_parts():
//!     for measure in part.measures:
//!         for voice in measure.voices:
//!             for el in voice.elements:
//!                 ...  # Note / Rest / Chord
//! ```
//!
//! Every wrapper owns a clone of its IR node, so the objects outlive the parent
//! score and never alias mutable state — this view is strictly read-only.

use pyo3::prelude::*;

use crate::ir::duration::Duration;
use crate::ir::measure::Measure;
use crate::ir::note::{Chord, Note, Rest, VoiceElement};
use crate::ir::part::Part;
use crate::ir::pitch::Pitch;
use crate::ir::voice::Voice;

/// A duration in quarter-note lengths (a `1/4` note = `1.0`).
fn quarter_length(d: &Duration) -> f64 {
    let f = d.actual_duration();
    (*f.numer() as f64 / *f.denom() as f64) * 4.0
}

/// A duration as an exact `(numerator, denominator)` fraction of a whole note.
fn duration_fraction(d: &Duration) -> (i64, i64) {
    let f = d.actual_duration();
    (*f.numer(), *f.denom())
}

/// Integer-semitone part of a chromatic alteration (1 = sharp, -1 = flat).
fn alter_int(p: &Pitch) -> i32 {
    *p.alter.numer() / *p.alter.denom()
}

// ---------------------------------------------------------------------------
// Pitch
// ---------------------------------------------------------------------------

/// A note's pitch: diatonic step, chromatic alteration, octave, MIDI number.
#[pyclass(name = "Pitch", module = "lytk._core", frozen)]
#[derive(Clone)]
pub struct PyPitch {
    pub(crate) inner: Pitch,
}

#[pymethods]
impl PyPitch {
    /// Diatonic step name, e.g. ``"C"``.
    #[getter]
    fn step(&self) -> String {
        self.inner.step.name().to_string()
    }

    /// Chromatic alteration in whole semitones (1 = sharp, -1 = flat).
    #[getter]
    fn alter(&self) -> i32 {
        alter_int(&self.inner)
    }

    /// Octave number (middle C = octave 4).
    #[getter]
    fn octave(&self) -> i32 {
        self.inner.octave
    }

    /// MIDI note number (middle C = 60).
    #[getter]
    fn midi(&self) -> i32 {
        self.inner.midi_number()
    }

    /// Scientific pitch name with accidentals, e.g. ``"C#4"`` / ``"Eb3"``.
    #[getter]
    fn name(&self) -> String {
        let alter = alter_int(&self.inner);
        let acc = if alter > 0 {
            "#".repeat(alter as usize)
        } else if alter < 0 {
            "b".repeat((-alter) as usize)
        } else {
            String::new()
        };
        format!("{}{}{}", self.inner.step.name(), acc, self.inner.octave)
    }

    fn __repr__(&self) -> String {
        format!("<Pitch {}>", self.name())
    }
}

// ---------------------------------------------------------------------------
// Note / Rest / Chord
// ---------------------------------------------------------------------------

/// A single pitched note.
#[pyclass(name = "Note", module = "lytk._core", frozen)]
#[derive(Clone)]
pub struct PyNote {
    pub(crate) inner: Note,
}

#[pymethods]
impl PyNote {
    /// The note's :class:`Pitch`.
    #[getter]
    fn pitch(&self) -> PyPitch {
        PyPitch {
            inner: self.inner.pitch,
        }
    }

    /// MIDI note number (shortcut for ``note.pitch.midi``).
    #[getter]
    fn midi(&self) -> i32 {
        self.inner.pitch.midi_number()
    }

    /// Duration in quarter-note lengths.
    #[getter]
    fn duration(&self) -> f64 {
        quarter_length(&self.inner.duration)
    }

    /// Exact duration as a ``(numerator, denominator)`` fraction of a whole note.
    #[getter]
    fn duration_fraction(&self) -> (i64, i64) {
        duration_fraction(&self.inner.duration)
    }

    /// 1-based voice number.
    #[getter]
    fn voice(&self) -> u8 {
        self.inner.voice
    }

    /// 1-based staff number.
    #[getter]
    fn staff(&self) -> u8 {
        self.inner.staff
    }

    /// Whether this is a grace note.
    #[getter]
    fn is_grace(&self) -> bool {
        self.inner.is_grace
    }

    /// Tie events on this note, as ``"start"`` / ``"stop"`` / ``"continue"``.
    #[getter]
    fn ties(&self) -> Vec<String> {
        self.inner
            .ties
            .iter()
            .map(|t| start_stop(&t.tie_type).to_string())
            .collect()
    }

    /// Articulation names (e.g. ``"staccato"``, ``"accent"``).
    #[getter]
    fn articulations(&self) -> Vec<String> {
        self.inner
            .articulations
            .iter()
            .map(|a| a.name.clone())
            .collect()
    }

    /// Lyric syllables attached to this note.
    #[getter]
    fn lyrics(&self) -> Vec<String> {
        self.inner.lyrics.iter().map(|l| l.text.clone()).collect()
    }

    fn __repr__(&self) -> String {
        format!(
            "<Note {} dur={}>",
            PyPitch {
                inner: self.inner.pitch
            }
            .name(),
            self.duration()
        )
    }
}

/// A rest (silence) or spacer.
#[pyclass(name = "Rest", module = "lytk._core", frozen)]
#[derive(Clone)]
pub struct PyRest {
    pub(crate) inner: Rest,
}

#[pymethods]
impl PyRest {
    #[getter]
    fn duration(&self) -> f64 {
        quarter_length(&self.inner.duration)
    }

    #[getter]
    fn duration_fraction(&self) -> (i64, i64) {
        duration_fraction(&self.inner.duration)
    }

    #[getter]
    fn voice(&self) -> u8 {
        self.inner.voice
    }

    #[getter]
    fn staff(&self) -> u8 {
        self.inner.staff
    }

    /// Whether this is a whole-measure rest.
    #[getter]
    fn is_measure_rest(&self) -> bool {
        self.inner.is_measure_rest
    }

    /// Whether this is an invisible spacer (LilyPond ``s``).
    #[getter]
    fn is_spacer(&self) -> bool {
        self.inner.is_spacer
    }

    fn __repr__(&self) -> String {
        format!("<Rest dur={}>", self.duration())
    }
}

/// A chord — several notes sounding together with a shared duration.
#[pyclass(name = "Chord", module = "lytk._core", frozen)]
#[derive(Clone)]
pub struct PyChord {
    pub(crate) inner: Chord,
}

#[pymethods]
impl PyChord {
    #[getter]
    fn duration(&self) -> f64 {
        quarter_length(&self.inner.duration)
    }

    #[getter]
    fn duration_fraction(&self) -> (i64, i64) {
        duration_fraction(&self.inner.duration)
    }

    #[getter]
    fn voice(&self) -> u8 {
        self.inner.voice
    }

    #[getter]
    fn staff(&self) -> u8 {
        self.inner.staff
    }

    /// The chord's notes, as :class:`Note` objects.
    #[getter]
    fn notes(&self) -> Vec<PyNote> {
        self.inner
            .notes
            .iter()
            .map(|n| PyNote { inner: n.clone() })
            .collect()
    }

    fn __len__(&self) -> usize {
        self.inner.notes.len()
    }

    fn __repr__(&self) -> String {
        let names: Vec<String> = self
            .inner
            .notes
            .iter()
            .map(|n| PyPitch { inner: n.pitch }.name())
            .collect();
        format!("<Chord [{}] dur={}>", names.join(", "), self.duration())
    }
}

/// Convert a voice element into its Python wrapper object.
fn element_to_py(py: Python<'_>, el: &VoiceElement) -> PyObject {
    match el {
        VoiceElement::Note(n) => PyNote {
            inner: (**n).clone(),
        }
        .into_py(py),
        VoiceElement::Rest(r) => PyRest { inner: r.clone() }.into_py(py),
        VoiceElement::Chord(c) => PyChord { inner: c.clone() }.into_py(py),
    }
}

/// Collect every sounding [`Note`] in an element stream (chord members included).
fn collect_notes(elements: &[VoiceElement]) -> Vec<PyNote> {
    let mut out = Vec::new();
    for el in elements {
        match el {
            VoiceElement::Note(n) => out.push(PyNote {
                inner: (**n).clone(),
            }),
            VoiceElement::Chord(c) => {
                out.extend(c.notes.iter().map(|n| PyNote { inner: n.clone() }))
            }
            VoiceElement::Rest(_) => {}
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Voice / Measure / Part
// ---------------------------------------------------------------------------

/// A voice within a measure — one rhythmic stream of notes/rests/chords.
#[pyclass(name = "Voice", module = "lytk._core", frozen)]
#[derive(Clone)]
pub struct PyVoice {
    pub(crate) inner: Voice,
}

#[pymethods]
impl PyVoice {
    /// 1-based voice number.
    #[getter]
    fn number(&self) -> u8 {
        self.inner.number
    }

    /// The voice's elements, as a list of :class:`Note` / :class:`Rest` /
    /// :class:`Chord` objects.
    #[getter]
    fn elements(&self, py: Python<'_>) -> Vec<PyObject> {
        self.inner
            .elements
            .iter()
            .map(|e| element_to_py(py, e))
            .collect()
    }

    /// Every sounding note in the voice (chord members flattened).
    #[getter]
    fn notes(&self) -> Vec<PyNote> {
        collect_notes(&self.inner.elements)
    }

    fn __len__(&self) -> usize {
        self.inner.elements.len()
    }

    fn __repr__(&self) -> String {
        format!(
            "<Voice {} elements={}>",
            self.inner.number,
            self.inner.elements.len()
        )
    }
}

/// A single measure / bar.
#[pyclass(name = "Measure", module = "lytk._core", frozen)]
#[derive(Clone)]
pub struct PyMeasure {
    pub(crate) inner: Measure,
}

#[pymethods]
impl PyMeasure {
    /// 1-based measure number.
    #[getter]
    fn number(&self) -> u32 {
        self.inner.number
    }

    /// Original non-numeric label (e.g. ``"3A"``), or *None*.
    #[getter]
    fn number_label(&self) -> Option<String> {
        self.inner.number_label.clone()
    }

    /// Whether this is an implicit measure (anacrusis / pickup).
    #[getter]
    fn implicit(&self) -> bool {
        self.inner.implicit
    }

    /// Whether this measure is senza-misura (free time, no bar length).
    #[getter]
    fn senza_misura(&self) -> bool {
        self.inner.senza_misura
    }

    /// Time signature taking effect here, as ``(beats, beat_type, symbol)``,
    /// or *None* if unchanged from the previous measure.
    #[getter]
    fn time_signature(&self) -> Option<(String, u8, Option<String>)> {
        self.inner
            .attributes
            .as_ref()
            .and_then(|a| a.time.as_ref())
            .map(|t| (t.beats.clone(), t.beat_type, t.symbol.clone()))
    }

    /// Key signature taking effect here, as ``(fifths, mode)``, or *None*.
    #[getter]
    fn key_signature(&self) -> Option<(i8, String)> {
        self.inner
            .attributes
            .as_ref()
            .and_then(|a| a.key.as_ref())
            .map(|k| (k.fifths, k.mode.as_str().to_string()))
    }

    /// The measure's voices.
    #[getter]
    fn voices(&self) -> Vec<PyVoice> {
        self.inner
            .voices
            .iter()
            .map(|v| PyVoice { inner: v.clone() })
            .collect()
    }

    /// Every sounding note across all voices in this measure.
    #[getter]
    fn notes(&self) -> Vec<PyNote> {
        self.inner
            .voices
            .iter()
            .flat_map(|v| collect_notes(&v.elements))
            .collect()
    }

    fn __len__(&self) -> usize {
        self.inner.voices.len()
    }

    fn __repr__(&self) -> String {
        format!(
            "<Measure {} voices={}>",
            self.inner.number,
            self.inner.voices.len()
        )
    }
}

/// A single instrument part — a sequence of measures.
#[pyclass(name = "Part", module = "lytk._core", frozen)]
#[derive(Clone)]
pub struct PyPart {
    pub(crate) inner: Part,
}

impl PyPart {
    /// Build a navigation wrapper from an IR part (crate-internal).
    pub(crate) fn from_ir(part: &Part) -> Self {
        PyPart {
            inner: part.clone(),
        }
    }
}

#[pymethods]
impl PyPart {
    /// Full instrument name (may be empty).
    #[getter]
    fn name(&self) -> String {
        self.inner.name.clone()
    }

    /// Abbreviated instrument name.
    #[getter]
    fn abbreviation(&self) -> String {
        self.inner.abbreviation.clone()
    }

    /// Unique part identifier.
    #[getter]
    fn part_id(&self) -> String {
        self.inner.part_id.clone()
    }

    /// MIDI program number.
    #[getter]
    fn midi_program(&self) -> u8 {
        self.inner.midi_program
    }

    /// MIDI channel (0-based).
    #[getter]
    fn midi_channel(&self) -> u8 {
        self.inner.midi_channel
    }

    /// Number of staves (e.g. 2 for piano).
    #[getter]
    fn staves(&self) -> u8 {
        self.inner.staves
    }

    /// The part's measures.
    #[getter]
    fn measures(&self) -> Vec<PyMeasure> {
        self.inner
            .measures
            .iter()
            .map(|m| PyMeasure { inner: m.clone() })
            .collect()
    }

    /// Every sounding note in the part (all measures, voices, chord members).
    #[getter]
    fn notes(&self) -> Vec<PyNote> {
        self.inner
            .measures
            .iter()
            .flat_map(|m| m.voices.iter())
            .flat_map(|v| collect_notes(&v.elements))
            .collect()
    }

    fn __len__(&self) -> usize {
        self.inner.measures.len()
    }

    fn __repr__(&self) -> String {
        let label = if self.inner.name.is_empty() {
            &self.inner.part_id
        } else {
            &self.inner.name
        };
        format!("<Part {:?} measures={}>", label, self.inner.measures.len())
    }
}

/// String label for a tie/slur start-stop value.
fn start_stop(s: &crate::ir::articulation::StartStop) -> &'static str {
    use crate::ir::articulation::StartStop::*;
    match s {
        Start => "start",
        Stop => "stop",
        Continue => "continue",
    }
}

/// Register the navigation classes on the `_core` module.
pub fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<PyPitch>()?;
    m.add_class::<PyNote>()?;
    m.add_class::<PyRest>()?;
    m.add_class::<PyChord>()?;
    m.add_class::<PyVoice>()?;
    m.add_class::<PyMeasure>()?;
    m.add_class::<PyPart>()?;
    Ok(())
}
