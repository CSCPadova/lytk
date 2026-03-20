//! Direction, dynamics, wedge, pedal, harmony, and figured bass parsing.
//!
//! Converts musicxml crate types to IR direction/harmony/figured-bass types.

use crate::ir::articulation::DynamicMark;
use crate::ir::articulation::Wedge;
use crate::ir::direction::*;
use crate::ir::duration::Duration;
use crate::ir::harmony::{ChordDegree, ChordPitch, Figure, FiguredBass, Harmony};
use crate::ir::Placement;

use super::note::note_type_value_to_str;

use musicxml::datatypes as mdt;
use musicxml::elements as mxml;

// ---------------------------------------------------------------------------
// Harmony / chord symbol conversion
// ---------------------------------------------------------------------------

pub(super) fn convert_harmony_elem(harm: &mxml::Harmony) -> Option<Harmony> {
    let sub = harm.content.harmony.first()?;

    let root = sub.root.as_ref()?;
    let root_step = step_to_str(&root.content.root_step.content);
    let root_alter: f64 = root
        .content
        .root_alter
        .as_ref()
        .map(|a| a.content.0 as f64)
        .unwrap_or(0.0);

    let kind = kind_value_to_str(&sub.kind.content);

    let bass = sub.bass.as_ref().map(|b| {
        let step = step_to_str(&b.content.bass_step.content);
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
            let degree_type = degree_type_to_str(&d.content.degree_type.content);
            ChordDegree {
                value,
                alter,
                degree_type,
            }
        })
        .collect();

    let offset: i32 = harm
        .content
        .offset
        .as_ref()
        .map(|o| o.content.0)
        .unwrap_or(0);

    Some(Harmony {
        root: ChordPitch {
            step: root_step,
            alter: root_alter,
        },
        kind,
        bass,
        degrees,
        offset,
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
            let prefix = f.content.prefix.as_ref().map(|p| p.content.clone());
            let suffix = f.content.suffix.as_ref().map(|s| s.content.clone());
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
        offset: 0,
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
                    if let Some(first_mark) = dyn_elem.content.first() {
                        let sign = dynamics_type_to_str(first_mark);
                        if !sign.is_empty() {
                            ir_dir.dynamic = Some(DynamicMark {
                                sign: sign.to_string(),
                                placement: ir_dir.placement,
                            });
                            break;
                        }
                    }
                }
            }
            mxml::DirectionTypeContents::Wedge(wedge) => {
                let wedge_type = wedge_type_to_str(&wedge.attributes.r#type);
                if !wedge_type.is_empty() {
                    ir_dir.wedge = Some(Wedge {
                        wedge_type: wedge_type.to_string(),
                        placement: ir_dir.placement,
                    });
                }
            }
            mxml::DirectionTypeContents::Words(words_vec) => {
                if let Some(words) = words_vec.first() {
                    let text = words.content.clone();
                    if !text.is_empty() {
                        ir_dir.text = Some(TextDirection {
                            text,
                            placement: ir_dir.placement,
                            font_style: words
                                .attributes
                                .font_style
                                .as_ref()
                                .map(|s| font_style_to_str(s).to_string()),
                            font_weight: words
                                .attributes
                                .font_weight
                                .as_ref()
                                .map(|w| font_weight_to_str(w).to_string()),
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
                    let beat_unit = note_type_value_to_str(&bb.beat_unit.content).to_string();
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
                let shift_type = match os.attributes.r#type {
                    mdt::UpDownStopContinue::Up => "up",
                    mdt::UpDownStopContinue::Down => "down",
                    mdt::UpDownStopContinue::Stop => "stop",
                    mdt::UpDownStopContinue::Continue => "continue",
                };
                let size = os.attributes.size.as_ref().map(|s| s.0 as i8).unwrap_or(8);
                ir_dir.octave_shift = Some(OctaveShift {
                    shift_type: shift_type.to_string(),
                    size,
                });
            }
            mxml::DirectionTypeContents::Pedal(pedal) => {
                let pedal_type = pedal_type_to_str(&pedal.attributes.r#type);
                let line = pedal.attributes.line == Some(mdt::YesNo::Yes);
                if !pedal_type.is_empty() {
                    ir_dir.pedal = Some(PedalEvent {
                        pedal_type: pedal_type.to_string(),
                        line,
                    });
                }
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
        if sound.attributes.dacapo == Some(mdt::YesNo::Yes) {
            ir_dir.da_capo = Some("D.C.".to_string());
        }
        if let Some(ref ds) = sound.attributes.dalsegno {
            if !ds.0.is_empty() {
                ir_dir.dal_segno = Some("D.S.".to_string());
            }
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
                        beat_unit: Some("quarter".to_string()),
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

fn step_to_str(step: &mdt::Step) -> String {
    match step {
        mdt::Step::C => "C",
        mdt::Step::D => "D",
        mdt::Step::E => "E",
        mdt::Step::F => "F",
        mdt::Step::G => "G",
        mdt::Step::A => "A",
        mdt::Step::B => "B",
    }
    .to_string()
}

fn kind_value_to_str(kv: &mdt::KindValue) -> String {
    match kv {
        mdt::KindValue::Augmented => "augmented",
        mdt::KindValue::AugmentedSeventh => "augmented-seventh",
        mdt::KindValue::Diminished => "diminished",
        mdt::KindValue::DiminishedSeventh => "diminished-seventh",
        mdt::KindValue::Dominant => "dominant",
        mdt::KindValue::Dominant11th => "dominant-11th",
        mdt::KindValue::Dominant13th => "dominant-13th",
        mdt::KindValue::DominantNinth => "dominant-ninth",
        mdt::KindValue::French => "French",
        mdt::KindValue::German => "German",
        mdt::KindValue::HalfDiminished => "half-diminished",
        mdt::KindValue::Italian => "Italian",
        mdt::KindValue::Major => "major",
        mdt::KindValue::Major11th => "major-11th",
        mdt::KindValue::Major13th => "major-13th",
        mdt::KindValue::MajorMinor => "major-minor",
        mdt::KindValue::MajorNinth => "major-ninth",
        mdt::KindValue::MajorSeventh => "major-seventh",
        mdt::KindValue::MajorSixth => "major-sixth",
        mdt::KindValue::Minor => "minor",
        mdt::KindValue::Minor11th => "minor-11th",
        mdt::KindValue::Minor13th => "minor-13th",
        mdt::KindValue::MinorNinth => "minor-ninth",
        mdt::KindValue::MinorSeventh => "minor-seventh",
        mdt::KindValue::MinorSixth => "minor-sixth",
        mdt::KindValue::Neapolitan => "Neapolitan",
        mdt::KindValue::None => "none",
        mdt::KindValue::Other => "other",
        mdt::KindValue::Pedal => "pedal",
        mdt::KindValue::Power => "power",
        mdt::KindValue::SuspendedFourth => "suspended-fourth",
        mdt::KindValue::SuspendedSecond => "suspended-second",
        mdt::KindValue::Tristan => "Tristan",
    }
    .to_string()
}

fn degree_type_to_str(dt: &mdt::DegreeTypeValue) -> String {
    match dt {
        mdt::DegreeTypeValue::Add => "add",
        mdt::DegreeTypeValue::Alter => "alter",
        mdt::DegreeTypeValue::Subtract => "subtract",
    }
    .to_string()
}

fn dynamics_type_to_str(dt: &mxml::DynamicsType) -> &'static str {
    match dt {
        mxml::DynamicsType::P(_) => "p",
        mxml::DynamicsType::Pp(_) => "pp",
        mxml::DynamicsType::Ppp(_) => "ppp",
        mxml::DynamicsType::Pppp(_) => "pppp",
        mxml::DynamicsType::Ppppp(_) => "ppppp",
        mxml::DynamicsType::Pppppp(_) => "pppppp",
        mxml::DynamicsType::F(_) => "f",
        mxml::DynamicsType::Ff(_) => "ff",
        mxml::DynamicsType::Fff(_) => "fff",
        mxml::DynamicsType::Ffff(_) => "ffff",
        mxml::DynamicsType::Fffff(_) => "fffff",
        mxml::DynamicsType::Ffffff(_) => "ffffff",
        mxml::DynamicsType::Mp(_) => "mp",
        mxml::DynamicsType::Mf(_) => "mf",
        mxml::DynamicsType::Sf(_) => "sf",
        mxml::DynamicsType::Sfp(_) => "sfp",
        mxml::DynamicsType::Sfpp(_) => "sfpp",
        mxml::DynamicsType::Fp(_) => "fp",
        mxml::DynamicsType::Rf(_) => "rf",
        mxml::DynamicsType::Rfz(_) => "rfz",
        mxml::DynamicsType::Sfz(_) => "sfz",
        mxml::DynamicsType::Sffz(_) => "sffz",
        mxml::DynamicsType::Fz(_) => "fz",
        mxml::DynamicsType::N(_) => "n",
        mxml::DynamicsType::Pf(_) => "pf",
        mxml::DynamicsType::Sfzp(_) => "sfzp",
        mxml::DynamicsType::OtherDynamics(_) => "",
    }
}

fn wedge_type_to_str(wt: &mdt::WedgeType) -> &'static str {
    match wt {
        mdt::WedgeType::Crescendo => "crescendo",
        mdt::WedgeType::Diminuendo => "diminuendo",
        mdt::WedgeType::Stop => "stop",
        mdt::WedgeType::Continue => "continue",
    }
}

fn pedal_type_to_str(pt: &mdt::PedalType) -> &'static str {
    match pt {
        mdt::PedalType::Start => "start",
        mdt::PedalType::Stop => "stop",
        mdt::PedalType::Sostenuto => "sostenuto",
        mdt::PedalType::Change => "change",
        mdt::PedalType::Continue => "continue",
        mdt::PedalType::Discontinue => "discontinue",
        mdt::PedalType::Resume => "resume",
    }
}

fn font_style_to_str(fs: &mdt::FontStyle) -> &'static str {
    match fs {
        mdt::FontStyle::Normal => "normal",
        mdt::FontStyle::Italic => "italic",
    }
}

fn font_weight_to_str(fw: &mdt::FontWeight) -> &'static str {
    match fw {
        mdt::FontWeight::Normal => "normal",
        mdt::FontWeight::Bold => "bold",
    }
}
