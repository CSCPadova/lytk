//! State types used during the lowering pass.

use super::super::annotation::Annotation;
use super::super::direction::*;
use super::super::duration::{Duration, Frac};
use super::super::harmony::{FiguredBass, Harmony};
use super::super::measure::*;
use super::super::music::ContextType;
use super::super::pitch::Pitch;
use super::super::score::*;

/// A timed event: something that happens at a specific moment.
#[derive(Debug, Clone)]
pub(super) enum TimedEvent {
    Note {
        pitch: Pitch,
        duration: Duration,
        annotations: Vec<Annotation>,
        voice: u8,
    },
    Chord {
        pitches: Vec<(Pitch, Vec<Annotation>)>,
        duration: Duration,
        annotations: Vec<Annotation>,
        voice: u8,
    },
    Rest {
        duration: Duration,
        is_measure_rest: bool,
        voice: u8,
    },
    Skip {
        duration: Duration,
        voice: u8,
    },
    TimeSignature(TimeSignature),
    KeySignature(KeySignature),
    Clef(Clef),
    Direction(Box<Direction>),
    Barline(Barline),
    FiguredBass(FiguredBass),
    Harmony(Harmony),
}

/// Collects events for a single staff with their absolute time offsets.
#[derive(Debug)]
pub(super) struct StaffBuilder {
    pub(super) name: String,
    #[allow(dead_code)]
    pub(super) staff_number: u8,
    pub(super) events: Vec<(Frac, TimedEvent)>,
}

impl StaffBuilder {
    pub(super) fn new(name: String, staff_number: u8) -> Self {
        Self {
            name,
            staff_number,
            events: Vec::new(),
        }
    }

    pub(super) fn push(&mut self, time: Frac, event: TimedEvent) {
        self.events.push((time, event));
    }
}

/// Tracks the structure being built from the Music tree.
pub(super) struct LowerState {
    /// Stack of staff builders — one per Staff context encountered.
    pub(super) staves: Vec<StaffBuilder>,
    /// Groups: (context_type, name, staff_indices)
    pub(super) groups: Vec<(ContextType, Option<String>, Vec<usize>)>,
    /// Current time offset for sequential processing.
    pub(super) time: Frac,
    /// Current voice number (incremented in Simultaneous).
    pub(super) voice: u8,
    /// Current time signature (for measure splitting).
    pub(super) current_time_sig: Frac,
    /// Current staff index we're adding events to.
    pub(super) current_staff: Option<usize>,
    /// Metadata collected from the document.
    pub(super) metadata: ScoreMetadata,
}

impl LowerState {
    pub(super) fn new(metadata: ScoreMetadata) -> Self {
        Self {
            staves: Vec::new(),
            groups: Vec::new(),
            time: Frac::from_integer(0),
            voice: 1,
            current_time_sig: Frac::new(1, 1), // default 4/4
            current_staff: None,
            metadata,
        }
    }

    pub(super) fn ensure_staff(&mut self) -> usize {
        if let Some(idx) = self.current_staff {
            idx
        } else {
            let idx = self.staves.len();
            self.staves
                .push(StaffBuilder::new(String::new(), idx as u8 + 1));
            self.current_staff = Some(idx);
            idx
        }
    }

    pub(super) fn push_event(&mut self, event: TimedEvent) {
        let idx = self.ensure_staff();
        let time = self.time;
        self.staves[idx].push(time, event);
    }
}
