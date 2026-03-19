//! Annotations — musical markings attached to notes in the Music tree.
//!
//! Annotations unify the various marking types (articulations, dynamics,
//! slurs, ties, pedal, etc.) that in the Layer 2 IR are scattered across
//! `Note` fields and `Measure.directions`. In the Music tree (Layer 1),
//! all markings attach directly to the note/chord they belong to.

use super::articulation::{
    Articulation, DynamicMark, Fermata, LyricSyllable, Ornament, Placement, StartStop, Technical,
    Wedge,
};
use super::direction::{OctaveShift, TextDirection};
use super::note::ArpeggioType;
use serde::{Deserialize, Serialize};

/// A musical annotation attached to a note or chord in the Music tree.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Annotation {
    /// Articulation marking (staccato, accent, tenuto, etc.)
    Articulation(Articulation),
    /// Ornament (trill, mordent, turn, etc.)
    Ornament(Ornament),
    /// Technical indication (fingering, string number, etc.)
    Technical(Technical),
    /// Dynamic marking (pp, mf, ff, etc.)
    Dynamic(DynamicMark),
    /// Crescendo/decrescendo hairpin
    Wedge(Wedge),

    // Spanning notations
    /// Start of a slur
    SlurStart { number: u8, placement: Placement },
    /// End of a slur
    SlurStop { number: u8 },
    /// Start of a tie
    TieStart,
    /// End of a tie
    TieStop,
    /// Start of a beam group
    BeamStart,
    /// End of a beam group
    BeamStop,

    // Note-level markings
    /// Fermata
    Fermata(Fermata),
    /// Arpeggio on a chord
    Arpeggio(ArpeggioType),
    /// Glissando
    Glissando(StartStop),
    /// Single-note tremolo (1-4 marks)
    Tremolo { marks: u8 },

    // Pedal
    /// Sustain pedal down
    PedalStart,
    /// Sustain pedal up
    PedalStop,
    /// Sustain pedal change (release + press)
    PedalChange,

    // Text/expression
    /// Text direction (dolce, pizz., etc.)
    Text(TextDirection),
    /// Fingering
    Fingering(String),
    /// Lyric syllable
    Lyric(LyricSyllable),

    // Octave shift
    /// Ottava indication
    OctaveShift(OctaveShift),
}
