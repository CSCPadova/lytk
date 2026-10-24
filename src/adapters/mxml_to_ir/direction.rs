//! Direction, dynamics, wedge, pedal, harmony, and figured bass parsing.
//!
//! Converts musicxml crate types to IR direction/harmony/figured-bass types.

use crate::ir::articulation::{DynamicMark, DynamicType, Wedge, WedgeType};
use crate::ir::direction::*;
use crate::ir::duration::{Duration, Frac};
use crate::ir::harmony::{ChordDegree, ChordPitch, Figure, FiguredBass, Harmony};
use crate::ir::Placement;

use super::note::{convert_step, note_type_value};

use crate::ir::direction::{FontStyle, FontWeight, OctaveShiftType, PedalType};
use crate::ir::duration::NoteType;
use crate::ir::harmony::{ChordKind, DegreeType};
use musicxml::datatypes as mdt;
use musicxml::elements as mxml;

// ---------------------------------------------------------------------------
// Harmony / chord symbol conversion
// ---------------------------------------------------------------------------

/// The harmony, its `offset` the `<offset>` from where it is written
/// (`divisions` per quarter note).
pub(super) fn convert_harmony_elem(harm: &mxml::Harmony, divisions: i64) -> Option<Harmony> {
    let sub = harm.content.harmony.first()?;

    let root = sub.root.as_ref()?;
    let root_step = convert_step(&root.content.root_step.content);
    let root_alter: f64 = root
        .content
        .root_alter
        .as_ref()
        .map(|a| a.content.0 as f64)
        .unwrap_or(0.0);

    let kind = chord_kind(&sub.kind.content);

    let bass = sub.bass.as_ref().map(|b| {
        let step = convert_step(&b.content.bass_step.content);
        let alter: f64 = b
            .content
            .bass_alter
            .as_ref()
            .map(|a| a.content.0 as f64)
            .unwrap_or(0.0);
        ChordPitch { step, alter }
    });

    let degrees: Vec<ChordDegree> = sub
        .degree
        .iter()
        .map(|d| {
            let value = d.content.degree_value.content.0 as u8;
            let alter = d.content.degree_alter.content.0 as f64;
            let degree_type = degree_type(&d.content.degree_type.content);
            ChordDegree {
                value,
                alter,
                degree_type,
            }
        })
        .collect();

    let offset = harm
        .content
        .offset
        .as_ref()
        .map_or(Frac::from_integer(0), |o| {
            Frac::new(i64::from(o.content.0), 4 * divisions.max(1))
        });

    // Functional-harmony Roman numeral (deprecated <function> element, still
    // common). Supplements the chord symbol.
    let function = sub.function.as_ref().map(|f| f.content.clone());

    Some(Harmony {
        root: ChordPitch {
            step: root_step,
            alter: root_alter,
        },
        kind,
        bass,
        degrees,
        offset,
        function,
    })
}

// ---------------------------------------------------------------------------
// Figured bass conversion
// ---------------------------------------------------------------------------

pub(super) fn convert_figured_bass_elem(fb: &mxml::FiguredBass, divisions: i64) -> FiguredBass {
    let figures: Vec<Figure> = fb
        .content
        .figure
        .iter()
        .map(|f| {
            let number: Option<u8> = f
                .content
                .figure_number
                .as_ref()
                .and_then(|n| n.content.parse().ok());
            let prefix = f.content.prefix.as_ref().map(|p| p.content.as_str().into());
            let suffix = f.content.suffix.as_ref().map(|s| s.content.as_str().into());
            Figure {
                number,
                prefix,
                suffix,
            }
        })
        .collect();

    let duration = fb
        .content
        .duration
        .as_ref()
        .map(|d| {
            let dur_val = d.content.0 as i64;
            if dur_val > 0 {
                Duration::from_divisions(dur_val, divisions, 0)
            } else {
                Duration::default()
            }
        })
        .unwrap_or_default();

    let parentheses = fb.attributes.parentheses == Some(mdt::YesNo::Yes);

    FiguredBass {
        figures,
        duration,
        parentheses,
        offset: Frac::from_integer(0),
    }
}

// ---------------------------------------------------------------------------
// Direction conversion
// ---------------------------------------------------------------------------

pub(super) fn convert_direction(dir: &mxml::Direction) -> Option<Direction> {
    let mut ir_dir = Direction::default();

    if let Some(ref p) = dir.attributes.placement {
        ir_dir.placement = match p {
            mdt::AboveBelow::Above => Placement::Above,
            mdt::AboveBelow::Below => Placement::Below,
        };
    }

    for dir_type in &dir.content.direction_type {
        match &dir_type.content {
            mxml::DirectionTypeContents::Dynamics(dynamics_vec) => {
                // Take the first dynamic element's first mark.
                for dyn_elem in dynamics_vec {
                    if let Some(sign) = dyn_elem.content.first().and_then(dynamic_type) {
                        ir_dir.dynamic = Some(DynamicMark {
                            sign,
                            placement: ir_dir.placement,
                        });
                        break;
                    }
                }
            }
            mxml::DirectionTypeContents::Wedge(wedge) => {
                ir_dir.wedge = Some(Wedge {
                    wedge_type: wedge_type(&wedge.attributes.r#type),
                    placement: ir_dir.placement,
                });
            }
            mxml::DirectionTypeContents::Words(words_vec) => {
                if let Some(words) = words_vec.first() {
                    let text = words.content.clone();
                    if !text.is_empty() {
                        ir_dir.text = Some(TextDirection {
                            text,
                            placement: ir_dir.placement,
                            font_style: words.attributes.font_style.as_ref().map(font_style),
                            font_weight: words.attributes.font_weight.as_ref().map(font_weight),
                        });
                    }
                }
            }
            mxml::DirectionTypeContents::Rehearsal(rehearsal_vec) => {
                if let Some(rehearsal) = rehearsal_vec.first() {
                    ir_dir.rehearsal = Some(RehearsalMark {
                        text: rehearsal.content.clone(),
                    });
                }
            }
            mxml::DirectionTypeContents::Metronome(metronome) => {
                if let mxml::MetronomeContents::BeatBased(bb) = &metronome.content {
                    let beat_unit = note_type_value(&bb.beat_unit.content);
                    let dots = bb.beat_unit_dot.len() as u8;
                    let per_minute = match &bb.equals {
                        mxml::BeatEquation::BPM(pm) => pm.content.parse::<f64>().ok(),
                        mxml::BeatEquation::Beats(_) => None,
                    };
                    ir_dir.tempo = Some(TempoDirection {
                        text: None,
                        beat_unit: Some(beat_unit),
                        per_minute,
                        dots,
                        placement: ir_dir.placement,
                    });
                }
            }
            mxml::DirectionTypeContents::OctaveShift(os) => {
                // MusicXML names where the notes are printed (an 8va's go
                // down); the IR, where they sound (up).
                let shift_type = match os.attributes.r#type {
                    mdt::UpDownStopContinue::Down => OctaveShiftType::Up,
                    mdt::UpDownStopContinue::Up => OctaveShiftType::Down,
                    mdt::UpDownStopContinue::Stop => OctaveShiftType::Stop,
                    mdt::UpDownStopContinue::Continue => OctaveShiftType::Continue,
                };
                let size = os.attributes.size.as_ref().map(|s| s.0 as i8).unwrap_or(8);
                ir_dir.octave_shift = Some(OctaveShift { shift_type, size });
            }
            mxml::DirectionTypeContents::Pedal(pedal) => {
                ir_dir.pedal = Some(PedalEvent {
                    pedal_type: pedal_type(&pedal.attributes.r#type),
                    line: pedal.attributes.line == Some(mdt::YesNo::Yes),
                });
            }
            mxml::DirectionTypeContents::Coda(_) => {
                ir_dir.coda = true;
            }
            mxml::DirectionTypeContents::Segno(_) => {
                ir_dir.segno = true;
            }
            _ => {}
        }
    }

    // If we have both a tempo (from <metronome>) and a text direction (from
    // <words>), merge the text into the tempo so that LilyPond emits e.g.
    // \tempo "Allegro" 4 = 120  instead of a separate \markup.
    if ir_dir.tempo.is_some() && ir_dir.text.is_some() {
        if let (Some(tempo), Some(text_dir)) = (ir_dir.tempo.as_mut(), ir_dir.text.take()) {
            tempo.text = Some(text_dir.text);
        }
    }

    // Check <sound> element for dacapo/dalsegno and tempo attributes.
    if let Some(ref sound) = dir.content.sound {
        // The direction's words are the jump's text (`D.C. al Fine`), kept
        // once: as plain text too, every writer wrote it twice.
        let mut jump =
            |default: &str| Some(ir_dir.text.take().map_or(default.to_string(), |t| t.text));
        if sound.attributes.dacapo == Some(mdt::YesNo::Yes) {
            ir_dir.da_capo = jump("D.C.");
        } else if sound
            .attributes
            .dalsegno
            .as_ref()
            .is_some_and(|ds| !ds.0.is_empty())
        {
            ir_dir.dal_segno = jump("D.S.");
        }
        // <sound tempo="120"> as fallback BPM when no <metronome> was given,
        // or to fill in a missing per_minute value.
        if let Some(ref tempo_val) = sound.attributes.tempo {
            let bpm = tempo_val.0;
            if bpm > 0.0 {
                if let Some(ref mut tempo) = ir_dir.tempo {
                    // Fill in BPM if the metronome element didn't have one.
                    if tempo.per_minute.is_none() {
                        tempo.per_minute = Some(bpm);
                    }
                } else {
                    // No <metronome> at all — create a tempo from <sound tempo>.
                    let text = ir_dir.text.take().map(|t| t.text);
                    ir_dir.tempo = Some(TempoDirection {
                        text,
                        beat_unit: Some(NoteType::Quarter),
                        per_minute: Some(bpm),
                        dots: 0,
                        placement: ir_dir.placement,
                    });
                }
            }
        }
    }

    // Check if any content was parsed.
    if ir_dir.dynamic.is_none()
        && ir_dir.wedge.is_none()
        && ir_dir.text.is_none()
        && ir_dir.rehearsal.is_none()
        && ir_dir.tempo.is_none()
        && ir_dir.octave_shift.is_none()
        && ir_dir.pedal.is_none()
        && !ir_dir.coda
        && !ir_dir.segno
        && ir_dir.da_capo.is_none()
        && ir_dir.dal_segno.is_none()
    {
        return None;
    }

    Some(ir_dir)
}

// ---------------------------------------------------------------------------
// Type conversion helpers
// ---------------------------------------------------------------------------

/// The chord kind, by its MusicXML name.
fn chord_kind(kv: &mdt::KindValue) -> ChordKind {
    ChordKind::from_name(&musicxml_internal::DatatypeSerializer::serialize(kv))
        .unwrap_or(ChordKind::Other)
}

fn degree_type(dt: &mdt::DegreeTypeValue) -> DegreeType {
    match dt {
        mdt::DegreeTypeValue::Add => DegreeType::Add,
        mdt::DegreeTypeValue::Alter => DegreeType::Alter,
        mdt::DegreeTypeValue::Subtract => DegreeType::Subtract,
    }
}

/// A `<dynamics>` mark's dynamic (none for `<other-dynamics>`).
fn dynamic_type(dt: &mxml::DynamicsType) -> Option<DynamicType> {
    Some(match dt {
        mxml::DynamicsType::P(_) => DynamicType::P,
        mxml::DynamicsType::Pp(_) => DynamicType::Pp,
        mxml::DynamicsType::Ppp(_) => DynamicType::Ppp,
        mxml::DynamicsType::Pppp(_) => DynamicType::Pppp,
        mxml::DynamicsType::Ppppp(_) => DynamicType::Ppppp,
        mxml::DynamicsType::Pppppp(_) => DynamicType::Pppppp,
        mxml::DynamicsType::F(_) => DynamicType::F,
        mxml::DynamicsType::Ff(_) => DynamicType::Ff,
        mxml::DynamicsType::Fff(_) => DynamicType::Fff,
        mxml::DynamicsType::Ffff(_) => DynamicType::Ffff,
        mxml::DynamicsType::Fffff(_) => DynamicType::Fffff,
        mxml::DynamicsType::Ffffff(_) => DynamicType::Ffffff,
        mxml::DynamicsType::Mp(_) => DynamicType::Mp,
        mxml::DynamicsType::Mf(_) => DynamicType::Mf,
        mxml::DynamicsType::Sf(_) => DynamicType::Sf,
        mxml::DynamicsType::Sfp(_) => DynamicType::Sfp,
        mxml::DynamicsType::Sfpp(_) => DynamicType::Sfpp,
        mxml::DynamicsType::Fp(_) => DynamicType::Fp,
        mxml::DynamicsType::Rf(_) => DynamicType::Rf,
        mxml::DynamicsType::Rfz(_) => DynamicType::Rfz,
        mxml::DynamicsType::Sfz(_) => DynamicType::Sfz,
        mxml::DynamicsType::Sffz(_) => DynamicType::Sffz,
        mxml::DynamicsType::Fz(_) => DynamicType::Fz,
        mxml::DynamicsType::N(_) => DynamicType::N,
        mxml::DynamicsType::Pf(_) => DynamicType::Pf,
        mxml::DynamicsType::Sfzp(_) => DynamicType::Sfzp,
        mxml::DynamicsType::OtherDynamics(_) => return None,
    })
}

fn wedge_type(wt: &mdt::WedgeType) -> WedgeType {
    match wt {
        mdt::WedgeType::Crescendo => WedgeType::Crescendo,
        mdt::WedgeType::Diminuendo => WedgeType::Diminuendo,
        mdt::WedgeType::Stop => WedgeType::Stop,
        mdt::WedgeType::Continue => WedgeType::Continue,
    }
}

fn pedal_type(pt: &mdt::PedalType) -> PedalType {
    match pt {
        mdt::PedalType::Start => PedalType::Start,
        mdt::PedalType::Stop => PedalType::Stop,
        mdt::PedalType::Sostenuto => PedalType::Sostenuto,
        mdt::PedalType::Change => PedalType::Change,
        mdt::PedalType::Continue => PedalType::Continue,
        mdt::PedalType::Discontinue => PedalType::Discontinue,
        mdt::PedalType::Resume => PedalType::Resume,
    }
}

fn font_style(fs: &mdt::FontStyle) -> FontStyle {
    match fs {
        mdt::FontStyle::Normal => FontStyle::Normal,
        mdt::FontStyle::Italic => FontStyle::Italic,
    }
}

fn font_weight(fw: &mdt::FontWeight) -> FontWeight {
    match fw {
        mdt::FontWeight::Normal => FontWeight::Normal,
        mdt::FontWeight::Bold => FontWeight::Bold,
    }
}
