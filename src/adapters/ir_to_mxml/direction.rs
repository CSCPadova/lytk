//! Direction, dynamics, wedge, pedal, harmony, and figured bass → musicxml element construction.

use super::helpers::format_float;
use super::IrToMxmlAdapter;
use crate::ir::articulation::Placement;
use crate::ir::direction::Direction;
use crate::ir::harmony::{FiguredBass, Harmony};
use crate::ir::note::Note;

use musicxml::datatypes as mdt;
use musicxml::elements as mxml;

impl IrToMxmlAdapter {
    /// Build `Direction` elements for dynamics, wedges, and text directions
    /// attached directly to a [`Note`] (populated by the LY->IR path).
    pub(super) fn build_note_direction_elements(&self, note: &Note) -> Vec<mxml::Direction> {
        let mut dirs = Vec::new();
        for dyn_mark in &note.dynamics {
            let dir = Direction {
                dynamic: Some(dyn_mark.clone()),
                placement: Placement::Below,
                ..Direction::default()
            };
            dirs.push(self.build_direction(&dir));
        }
        for wedge in &note.wedges {
            let dir = Direction {
                wedge: Some(wedge.clone()),
                placement: Placement::Below,
                ..Direction::default()
            };
            dirs.push(self.build_direction(&dir));
        }
        for td in &note.text_directions {
            let dir = Direction {
                text: Some(td.clone()),
                placement: td.placement,
                ..Direction::default()
            };
            dirs.push(self.build_direction(&dir));
        }
        dirs
    }

    pub(super) fn build_direction(&self, direction: &Direction) -> mxml::Direction {
        let placement = match direction.placement {
            Placement::Above => Some(mdt::AboveBelow::Above),
            Placement::Below => Some(mdt::AboveBelow::Below),
            Placement::Unspecified => None,
        };

        let mut direction_types: Vec<mxml::DirectionType> = Vec::new();

        // Dynamics
        if let Some(dyn_mark) = &direction.dynamic {
            let dyn_type = str_to_dynamics_type(&dyn_mark.sign);
            direction_types.push(mxml::DirectionType {
                attributes: mxml::DirectionTypeAttributes::default(),
                content: mxml::DirectionTypeContents::Dynamics(vec![mxml::Dynamics {
                    attributes: mxml::DynamicsAttributes::default(),
                    content: vec![dyn_type],
                }]),
            });
        }

        // Wedge
        if let Some(wedge) = &direction.wedge {
            let wedge_type = match wedge.wedge_type.as_str() {
                "crescendo" => mdt::WedgeType::Crescendo,
                "diminuendo" => mdt::WedgeType::Diminuendo,
                "stop" => mdt::WedgeType::Stop,
                _ => mdt::WedgeType::Crescendo,
            };
            direction_types.push(mxml::DirectionType {
                attributes: mxml::DirectionTypeAttributes::default(),
                content: mxml::DirectionTypeContents::Wedge(mxml::Wedge {
                    attributes: mxml::WedgeAttributes {
                        r#type: wedge_type,
                        color: None,
                        dash_length: None,
                        default_x: None,
                        default_y: None,
                        id: None,
                        line_type: None,
                        niente: None,
                        number: None,
                        relative_x: None,
                        relative_y: None,
                        space_length: None,
                        spread: None,
                    },
                    content: (),
                }),
            });
        }

        // Text direction (words)
        if let Some(text_dir) = &direction.text {
            let mut words_attrs = mxml::WordsAttributes::default();
            if let Some(ref fs) = text_dir.font_style {
                words_attrs.font_style = match fs.as_str() {
                    "italic" => Some(mdt::FontStyle::Italic),
                    "normal" => Some(mdt::FontStyle::Normal),
                    _ => None,
                };
            }
            if let Some(ref fw) = text_dir.font_weight {
                words_attrs.font_weight = match fw.as_str() {
                    "bold" => Some(mdt::FontWeight::Bold),
                    "normal" => Some(mdt::FontWeight::Normal),
                    _ => None,
                };
            }
            direction_types.push(mxml::DirectionType {
                attributes: mxml::DirectionTypeAttributes::default(),
                content: mxml::DirectionTypeContents::Words(vec![mxml::Words {
                    attributes: words_attrs,
                    content: text_dir.text.clone(),
                }]),
            });
        }

        // Rehearsal mark
        if let Some(reh) = &direction.rehearsal {
            direction_types.push(mxml::DirectionType {
                attributes: mxml::DirectionTypeAttributes::default(),
                content: mxml::DirectionTypeContents::Rehearsal(vec![mxml::Rehearsal {
                    attributes: mxml::RehearsalAttributes::default(),
                    content: reh.text.clone(),
                }]),
            });
        }

        // Octave shift
        if let Some(os) = &direction.octave_shift {
            let shift_type = match os.shift_type.as_str() {
                "up" => mdt::UpDownStopContinue::Up,
                "down" => mdt::UpDownStopContinue::Down,
                "stop" => mdt::UpDownStopContinue::Stop,
                "continue" => mdt::UpDownStopContinue::Continue,
                _ => mdt::UpDownStopContinue::Up,
            };
            direction_types.push(mxml::DirectionType {
                attributes: mxml::DirectionTypeAttributes::default(),
                content: mxml::DirectionTypeContents::OctaveShift(mxml::OctaveShift {
                    attributes: mxml::OctaveShiftAttributes {
                        r#type: shift_type,
                        size: Some(mdt::PositiveInteger(os.size as u32)),
                        ..Default::default()
                    },
                    content: (),
                }),
            });
        }

        // Pedal
        if let Some(ped) = &direction.pedal {
            let pedal_type = match ped.pedal_type.as_str() {
                "start" => mdt::PedalType::Start,
                "stop" => mdt::PedalType::Stop,
                "sostenuto" => mdt::PedalType::Sostenuto,
                "change" => mdt::PedalType::Change,
                "continue" => mdt::PedalType::Continue,
                "resume" => mdt::PedalType::Resume,
                _ => mdt::PedalType::Start,
            };
            direction_types.push(mxml::DirectionType {
                attributes: mxml::DirectionTypeAttributes::default(),
                content: mxml::DirectionTypeContents::Pedal(mxml::Pedal {
                    attributes: mxml::PedalAttributes {
                        r#type: pedal_type,
                        line: if ped.line {
                            Some(mdt::YesNo::Yes)
                        } else {
                            None
                        },
                        abbreviated: None,
                        color: None,
                        default_x: None,
                        default_y: None,
                        font_family: None,
                        font_size: None,
                        font_style: None,
                        font_weight: None,
                        id: None,
                        number: None,
                        relative_x: None,
                        relative_y: None,
                        sign: None,
                    },
                    content: (),
                }),
            });
        }

        // Tempo (text label + metronome)
        if let Some(tempo) = &direction.tempo {
            if let Some(ref label) = tempo.text {
                direction_types.push(mxml::DirectionType {
                    attributes: mxml::DirectionTypeAttributes::default(),
                    content: mxml::DirectionTypeContents::Words(vec![mxml::Words {
                        attributes: mxml::WordsAttributes::default(),
                        content: label.clone(),
                    }]),
                });
            }
            if let (Some(beat_unit), Some(per_min)) = (&tempo.beat_unit, tempo.per_minute) {
                let beat_unit_val = match beat_unit.as_str() {
                    "whole" => mdt::NoteTypeValue::Whole,
                    "half" => mdt::NoteTypeValue::Half,
                    "quarter" => mdt::NoteTypeValue::Quarter,
                    "eighth" => mdt::NoteTypeValue::Eighth,
                    "16th" => mdt::NoteTypeValue::Sixteenth,
                    _ => mdt::NoteTypeValue::Quarter,
                };
                let beat_unit_dots: Vec<mxml::BeatUnitDot> = (0..tempo.dots)
                    .map(|_| mxml::BeatUnitDot {
                        attributes: (),
                        content: (),
                    })
                    .collect();
                direction_types.push(mxml::DirectionType {
                    attributes: mxml::DirectionTypeAttributes::default(),
                    content: mxml::DirectionTypeContents::Metronome(mxml::Metronome {
                        attributes: mxml::MetronomeAttributes::default(),
                        content: mxml::MetronomeContents::BeatBased(mxml::BeatBased {
                            beat_unit: mxml::BeatUnit {
                                attributes: (),
                                content: beat_unit_val,
                            },
                            beat_unit_dot: beat_unit_dots,
                            beat_unit_tied: vec![],
                            equals: mxml::BeatEquation::BPM(mxml::PerMinute {
                                attributes: mxml::PerMinuteAttributes::default(),
                                content: format_float(per_min),
                            }),
                        }),
                    }),
                });
            }
        }

        // Coda
        if direction.coda {
            direction_types.push(mxml::DirectionType {
                attributes: mxml::DirectionTypeAttributes::default(),
                content: mxml::DirectionTypeContents::Coda(vec![mxml::Coda {
                    attributes: mxml::CodaAttributes::default(),
                    content: (),
                }]),
            });
        }

        // Segno
        if direction.segno {
            direction_types.push(mxml::DirectionType {
                attributes: mxml::DirectionTypeAttributes::default(),
                content: mxml::DirectionTypeContents::Segno(vec![mxml::Segno {
                    attributes: mxml::SegnoAttributes::default(),
                    content: (),
                }]),
            });
        }

        // Da capo text
        if let Some(text) = &direction.da_capo {
            direction_types.push(mxml::DirectionType {
                attributes: mxml::DirectionTypeAttributes::default(),
                content: mxml::DirectionTypeContents::Words(vec![mxml::Words {
                    attributes: mxml::WordsAttributes::default(),
                    content: text.clone(),
                }]),
            });
        }

        // Dal segno text
        if let Some(text) = &direction.dal_segno {
            direction_types.push(mxml::DirectionType {
                attributes: mxml::DirectionTypeAttributes::default(),
                content: mxml::DirectionTypeContents::Words(vec![mxml::Words {
                    attributes: mxml::WordsAttributes::default(),
                    content: text.clone(),
                }]),
            });
        }

        // Sound element
        let tempo_bpm = direction.tempo.as_ref().and_then(|t| t.per_minute);
        let has_dacapo = direction.da_capo.is_some();
        let has_dalsegno = direction.dal_segno.is_some();
        let sound = if tempo_bpm.is_some() || has_dacapo || has_dalsegno {
            let mut sound_attrs = mxml::SoundAttributes::default();
            if let Some(bpm) = tempo_bpm {
                sound_attrs.tempo = Some(mdt::NonNegativeDecimal(bpm));
            }
            if has_dacapo {
                sound_attrs.dacapo = Some(mdt::YesNo::Yes);
            }
            if has_dalsegno {
                sound_attrs.dalsegno = Some(mdt::Token(String::from("yes")));
            }
            Some(mxml::Sound {
                attributes: sound_attrs,
                content: mxml::SoundContents {
                    instrument_change: vec![],
                    midi_device: vec![],
                    midi_instrument: vec![],
                    play: vec![],
                    swing: None,
                    offset: None,
                },
            })
        } else if let Some(ref ic) = direction.instrument_change {
            // Instrument change (separate sound with MIDI instrument child)
            let sound_attrs = mxml::SoundAttributes::default();
            let midi_instrument = mxml::MidiInstrument {
                attributes: mxml::MidiInstrumentAttributes {
                    id: mdt::IdRef(ic.instrument_id.clone()),
                },
                content: mxml::MidiInstrumentContents {
                    midi_channel: None,
                    midi_name: ic.instrument_name.as_ref().map(|n| mxml::MidiName {
                        attributes: (),
                        content: n.clone(),
                    }),
                    midi_program: None,
                    midi_bank: None,
                    midi_unpitched: None,
                    volume: None,
                    pan: None,
                    elevation: None,
                },
            };
            Some(mxml::Sound {
                attributes: sound_attrs,
                content: mxml::SoundContents {
                    instrument_change: vec![],
                    midi_device: vec![],
                    midi_instrument: vec![midi_instrument],
                    play: vec![],
                    swing: None,
                    offset: None,
                },
            })
        } else {
            None
        };

        mxml::Direction {
            attributes: mxml::DirectionAttributes {
                placement,
                ..Default::default()
            },
            content: mxml::DirectionContents {
                direction_type: direction_types,
                offset: None,
                footnote: None,
                level: None,
                voice: None,
                staff: None,
                sound,
                listening: None,
            },
        }
    }

    pub(super) fn build_harmony(&self, harmony: &Harmony) -> mxml::Harmony {
        // Root
        let root_step = mxml::RootStep {
            attributes: mxml::RootStepAttributes::default(),
            content: str_to_step(&harmony.root.step),
        };
        let root_alter = if harmony.root.alter != 0.0 {
            Some(mxml::RootAlter {
                attributes: mxml::RootAlterAttributes::default(),
                content: mdt::Semitones(harmony.root.alter as i16),
            })
        } else {
            None
        };

        let kind = mxml::Kind {
            attributes: mxml::KindAttributes::default(),
            content: str_to_kind_value(&harmony.kind),
        };

        // Bass
        let bass = harmony.bass.as_ref().map(|b| {
            let bass_step = mxml::BassStep {
                attributes: mxml::BassStepAttributes::default(),
                content: str_to_step(&b.step),
            };
            let bass_alter = if b.alter != 0.0 {
                Some(mxml::BassAlter {
                    attributes: mxml::BassAlterAttributes::default(),
                    content: mdt::Semitones(b.alter as i16),
                })
            } else {
                None
            };
            mxml::Bass {
                attributes: mxml::BassAttributes::default(),
                content: mxml::BassContents {
                    bass_separator: None,
                    bass_step,
                    bass_alter,
                },
            }
        });

        // Degrees
        let degree: Vec<mxml::Degree> = harmony
            .degrees
            .iter()
            .map(|deg| {
                let degree_type_val = match deg.degree_type.as_str() {
                    "add" => mdt::DegreeTypeValue::Add,
                    "alter" => mdt::DegreeTypeValue::Alter,
                    "subtract" => mdt::DegreeTypeValue::Subtract,
                    _ => mdt::DegreeTypeValue::Alter,
                };
                mxml::Degree {
                    attributes: mxml::DegreeAttributes::default(),
                    content: mxml::DegreeContents {
                        degree_value: mxml::DegreeValue {
                            attributes: mxml::DegreeValueAttributes::default(),
                            content: mdt::PositiveInteger(deg.value as u32),
                        },
                        degree_alter: mxml::DegreeAlter {
                            attributes: mxml::DegreeAlterAttributes::default(),
                            content: mdt::Semitones(deg.alter as i16),
                        },
                        degree_type: mxml::DegreeType {
                            attributes: mxml::DegreeTypeAttributes::default(),
                            content: degree_type_val,
                        },
                    },
                }
            })
            .collect();

        // Offset
        let offset = if harmony.offset != 0 {
            Some(mxml::Offset {
                attributes: mxml::OffsetAttributes::default(),
                content: mdt::Divisions(harmony.offset),
            })
        } else {
            None
        };

        mxml::Harmony {
            attributes: mxml::HarmonyAttributes::default(),
            content: mxml::HarmonyContents {
                harmony: vec![mxml::HarmonySubcontents {
                    root: Some(mxml::Root {
                        attributes: (),
                        content: mxml::RootContents {
                            root_step,
                            root_alter,
                        },
                    }),
                    numeral: None,
                    function: None,
                    kind,
                    inversion: None,
                    bass,
                    degree,
                }],
                frame: None,
                offset,
                footnote: None,
                level: None,
                staff: None,
            },
        }
    }

    pub(super) fn build_figured_bass(&self, fb: &FiguredBass) -> mxml::FiguredBass {
        let figures: Vec<mxml::Figure> = fb
            .figures
            .iter()
            .map(|fig| {
                let prefix = fig.prefix.as_ref().map(|p| mxml::Prefix {
                    attributes: mxml::PrefixAttributes::default(),
                    content: p.clone(),
                });
                let figure_number = fig.number.map(|n| mxml::FigureNumber {
                    attributes: mxml::FigureNumberAttributes::default(),
                    content: n.to_string(),
                });
                let suffix = fig.suffix.as_ref().map(|s| mxml::Suffix {
                    attributes: mxml::SuffixAttributes::default(),
                    content: s.clone(),
                });
                mxml::Figure {
                    attributes: (),
                    content: mxml::FigureContents {
                        prefix,
                        figure_number,
                        suffix,
                        extend: None,
                        footnote: None,
                        level: None,
                    },
                }
            })
            .collect();

        let dur_val = self.duration_to_divisions(&fb.duration).max(1) as u32;

        mxml::FiguredBass {
            attributes: mxml::FiguredBassAttributes {
                parentheses: if fb.parentheses {
                    Some(mdt::YesNo::Yes)
                } else {
                    None
                },
                ..Default::default()
            },
            content: mxml::FiguredBassContents {
                figure: figures,
                duration: Some(mxml::Duration {
                    attributes: (),
                    content: mdt::PositiveDivisions(dur_val),
                }),
                footnote: None,
                level: None,
            },
        }
    }
}

// ---------------------------------------------------------------------------
// String-to-enum helpers
// ---------------------------------------------------------------------------

fn str_to_step(s: &str) -> mdt::Step {
    match s {
        "C" => mdt::Step::C,
        "D" => mdt::Step::D,
        "E" => mdt::Step::E,
        "F" => mdt::Step::F,
        "G" => mdt::Step::G,
        "A" => mdt::Step::A,
        "B" => mdt::Step::B,
        _ => mdt::Step::C,
    }
}

fn str_to_kind_value(s: &str) -> mdt::KindValue {
    match s {
        "major" => mdt::KindValue::Major,
        "minor" => mdt::KindValue::Minor,
        "augmented" => mdt::KindValue::Augmented,
        "diminished" => mdt::KindValue::Diminished,
        "dominant" => mdt::KindValue::Dominant,
        "major-seventh" => mdt::KindValue::MajorSeventh,
        "minor-seventh" => mdt::KindValue::MinorSeventh,
        "diminished-seventh" => mdt::KindValue::DiminishedSeventh,
        "augmented-seventh" => mdt::KindValue::AugmentedSeventh,
        "half-diminished" => mdt::KindValue::HalfDiminished,
        "major-minor" => mdt::KindValue::MajorMinor,
        "major-sixth" => mdt::KindValue::MajorSixth,
        "minor-sixth" => mdt::KindValue::MinorSixth,
        "dominant-ninth" => mdt::KindValue::DominantNinth,
        "major-ninth" => mdt::KindValue::MajorNinth,
        "minor-ninth" => mdt::KindValue::MinorNinth,
        "dominant-11th" => mdt::KindValue::Dominant11th,
        "major-11th" => mdt::KindValue::Major11th,
        "minor-11th" => mdt::KindValue::Minor11th,
        "dominant-13th" => mdt::KindValue::Dominant13th,
        "major-13th" => mdt::KindValue::Major13th,
        "minor-13th" => mdt::KindValue::Minor13th,
        "suspended-second" => mdt::KindValue::SuspendedSecond,
        "suspended-fourth" => mdt::KindValue::SuspendedFourth,
        "power" => mdt::KindValue::Power,
        "none" => mdt::KindValue::None,
        "other" => mdt::KindValue::Other,
        _ => mdt::KindValue::Other,
    }
}

fn str_to_dynamics_type(sign: &str) -> mxml::DynamicsType {
    match sign {
        "p" => mxml::DynamicsType::P(mxml::P {
            attributes: (),
            content: (),
        }),
        "pp" => mxml::DynamicsType::Pp(mxml::Pp {
            attributes: (),
            content: (),
        }),
        "ppp" => mxml::DynamicsType::Ppp(mxml::Ppp {
            attributes: (),
            content: (),
        }),
        "pppp" => mxml::DynamicsType::Pppp(mxml::Pppp {
            attributes: (),
            content: (),
        }),
        "ppppp" => mxml::DynamicsType::Ppppp(mxml::Ppppp {
            attributes: (),
            content: (),
        }),
        "pppppp" => mxml::DynamicsType::Pppppp(mxml::Pppppp {
            attributes: (),
            content: (),
        }),
        "f" => mxml::DynamicsType::F(mxml::F {
            attributes: (),
            content: (),
        }),
        "ff" => mxml::DynamicsType::Ff(mxml::Ff {
            attributes: (),
            content: (),
        }),
        "fff" => mxml::DynamicsType::Fff(mxml::Fff {
            attributes: (),
            content: (),
        }),
        "ffff" => mxml::DynamicsType::Ffff(mxml::Ffff {
            attributes: (),
            content: (),
        }),
        "fffff" => mxml::DynamicsType::Fffff(mxml::Fffff {
            attributes: (),
            content: (),
        }),
        "ffffff" => mxml::DynamicsType::Ffffff(mxml::Ffffff {
            attributes: (),
            content: (),
        }),
        "mp" => mxml::DynamicsType::Mp(mxml::Mp {
            attributes: (),
            content: (),
        }),
        "mf" => mxml::DynamicsType::Mf(mxml::Mf {
            attributes: (),
            content: (),
        }),
        "sf" => mxml::DynamicsType::Sf(mxml::Sf {
            attributes: (),
            content: (),
        }),
        "sfp" => mxml::DynamicsType::Sfp(mxml::Sfp {
            attributes: (),
            content: (),
        }),
        "sfpp" => mxml::DynamicsType::Sfpp(mxml::Sfpp {
            attributes: (),
            content: (),
        }),
        "fp" => mxml::DynamicsType::Fp(mxml::Fp {
            attributes: (),
            content: (),
        }),
        "rf" => mxml::DynamicsType::Rf(mxml::Rf {
            attributes: (),
            content: (),
        }),
        "rfz" => mxml::DynamicsType::Rfz(mxml::Rfz {
            attributes: (),
            content: (),
        }),
        "sfz" => mxml::DynamicsType::Sfz(mxml::Sfz {
            attributes: (),
            content: (),
        }),
        "sffz" => mxml::DynamicsType::Sffz(mxml::Sffz {
            attributes: (),
            content: (),
        }),
        "fz" => mxml::DynamicsType::Fz(mxml::Fz {
            attributes: (),
            content: (),
        }),
        "n" => mxml::DynamicsType::N(mxml::N {
            attributes: (),
            content: (),
        }),
        "pf" => mxml::DynamicsType::Pf(mxml::Pf {
            attributes: (),
            content: (),
        }),
        "sfzp" => mxml::DynamicsType::Sfzp(mxml::Sfzp {
            attributes: (),
            content: (),
        }),
        _ => mxml::DynamicsType::OtherDynamics(mxml::OtherDynamics {
            attributes: mxml::OtherDynamicsAttributes::default(),
            content: sign.to_string(),
        }),
    }
}
