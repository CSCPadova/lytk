//! Note, chord, and rest → musicxml element construction.

use super::helpers::{alter_to_accidental_value, start_stop_to_mxml, start_stop_to_mxml_ss};
use super::IrToMxmlAdapter;
use crate::ir::articulation::StartStop;
use crate::ir::note::{ArpeggioType, Chord, Note, Rest};
use crate::ir::pitch::AccidentalDisplay;

use musicxml::datatypes as mdt;
use musicxml::elements as mxml;

impl IrToMxmlAdapter {
    pub(super) fn build_note(
        &self,
        note: &Note,
        voice_num: u8,
        is_chord: bool,
        chord_arpeggio: Option<ArpeggioType>,
        part_staves: u8,
    ) -> mxml::Note {
        // Build pitch
        let step = ir_step_to_mxml(&note.pitch.step);
        let alter_val = *note.pitch.alter.numer() as f64 / *note.pitch.alter.denom() as f64;
        let alter = if alter_val != 0.0 {
            Some(mxml::Alter {
                attributes: (),
                content: mdt::Semitones(alter_val as i16),
            })
        } else {
            None
        };
        let pitch = mxml::Pitch {
            attributes: (),
            content: mxml::PitchContents {
                step: mxml::Step {
                    attributes: (),
                    content: step,
                },
                alter,
                octave: mxml::Octave {
                    attributes: (),
                    content: mdt::Octave(note.pitch.octave as u8),
                },
            },
        };

        // Build note info based on type (grace/cue/normal)
        let chord_tag = if is_chord {
            Some(mxml::Chord {
                attributes: (),
                content: (),
            })
        } else {
            None
        };

        let info = if note.is_grace {
            let mut grace_attrs = mxml::GraceAttributes::default();
            if note.after_grace {
                grace_attrs.steal_time_previous = Some(mdt::Percent(100.0));
            }
            if note.grace_slash {
                grace_attrs.slash = Some(mdt::YesNo::Yes);
            }
            mxml::NoteType::Grace(mxml::GraceInfo {
                grace: mxml::Grace {
                    attributes: grace_attrs,
                    content: (),
                },
                info: mxml::GraceType::Normal(mxml::GraceNormalInfo {
                    chord: chord_tag,
                    audible: mxml::AudibleType::Pitch(pitch),
                    tie: build_ties(&note.ties),
                }),
            })
        } else if note.is_cue {
            mxml::NoteType::Cue(mxml::CueInfo {
                cue: mxml::Cue {
                    attributes: (),
                    content: (),
                },
                chord: chord_tag,
                audible: mxml::AudibleType::Pitch(pitch),
                duration: mxml::Duration {
                    attributes: (),
                    content: mdt::PositiveDivisions(
                        self.duration_to_divisions(&note.duration).max(1) as u32,
                    ),
                },
            })
        } else {
            mxml::NoteType::Normal(mxml::NormalInfo {
                chord: chord_tag,
                audible: mxml::AudibleType::Pitch(pitch),
                duration: mxml::Duration {
                    attributes: (),
                    content: mdt::PositiveDivisions(
                        self.duration_to_divisions(&note.duration).max(1) as u32,
                    ),
                },
                tie: build_ties(&note.ties),
            })
        };

        // Voice
        let voice = Some(mxml::Voice {
            attributes: (),
            content: voice_num.to_string(),
        });

        // Type (quarter, half, etc.)
        let note_type = note
            .duration
            .musicxml_type()
            .and_then(str_to_note_type_value)
            .map(|v| mxml::Type {
                attributes: mxml::TypeAttributes::default(),
                content: v,
            });

        // Dots
        let dot: Vec<mxml::Dot> = (0..note.duration.dots)
            .map(|_| mxml::Dot {
                attributes: mxml::DotAttributes::default(),
                content: (),
            })
            .collect();

        // Notehead
        let notehead = if !note.notehead.is_empty() && note.notehead != "normal" {
            str_to_notehead_value(&note.notehead).map(|v| mxml::Notehead {
                attributes: mxml::NoteheadAttributes::default(),
                content: v,
            })
        } else {
            None
        };

        // Accidental display
        let accidental = if note.pitch.accidental != AccidentalDisplay::None {
            let acc_val = alter_to_accidental_value(note.pitch.alter);
            let mut acc_attrs = mxml::AccidentalAttributes::default();
            match note.pitch.accidental {
                AccidentalDisplay::Cautionary => {
                    acc_attrs.cautionary = Some(mdt::YesNo::Yes);
                }
                AccidentalDisplay::Editorial => {
                    acc_attrs.editorial = Some(mdt::YesNo::Yes);
                }
                _ => {}
            }
            Some(mxml::Accidental {
                attributes: acc_attrs,
                content: acc_val,
            })
        } else {
            None
        };

        // Time modification (tuplets)
        let time_modification =
            if note.duration.tuplet_actual != 1 || note.duration.tuplet_normal != 1 {
                Some(mxml::TimeModification {
                    attributes: (),
                    content: mxml::TimeModificationContents {
                        actual_notes: mxml::ActualNotes {
                            attributes: (),
                            content: mdt::NonNegativeInteger(note.duration.tuplet_actual as u32),
                        },
                        normal_notes: mxml::NormalNotes {
                            attributes: (),
                            content: mdt::NonNegativeInteger(note.duration.tuplet_normal as u32),
                        },
                        normal_type: None,
                        normal_dot: vec![],
                    },
                })
            } else {
                None
            };

        // Stem
        let stem = if !note.stem_direction.is_empty() {
            str_to_stem_value(&note.stem_direction).map(|v| mxml::Stem {
                attributes: mxml::StemAttributes::default(),
                content: v,
            })
        } else {
            None
        };

        // Staff
        let staff = if part_staves > 1 || note.staff > 1 {
            Some(mxml::Staff {
                attributes: (),
                content: mdt::PositiveInteger(note.staff as u32),
            })
        } else {
            None
        };

        // Beams
        let beam: Vec<mxml::Beam> = note
            .beams
            .iter()
            .filter_map(|b| {
                str_to_beam_value(&b.beam_type).map(|bv| mxml::Beam {
                    attributes: mxml::BeamAttributes {
                        number: Some(mdt::BeamLevel(b.number)),
                        ..Default::default()
                    },
                    content: bv,
                })
            })
            .collect();

        // Notations
        let notations = build_note_notations(note, chord_arpeggio);

        // Lyrics
        let lyric: Vec<mxml::Lyric> = note.lyrics.iter().map(build_lyric).collect();

        // print-object attribute
        let mut attrs = mxml::NoteAttributes::default();
        if !note.print_object {
            attrs.print_object = Some(mdt::YesNo::No);
        }

        mxml::Note {
            attributes: attrs,
            content: mxml::NoteContents {
                info,
                instrument: vec![],
                footnote: None,
                level: None,
                voice,
                r#type: note_type,
                dot,
                accidental,
                time_modification,
                stem,
                notehead,
                notehead_text: None,
                staff,
                beam,
                notations,
                lyric,
                play: None,
                listen: None,
            },
        }
    }

    pub(super) fn build_rest_note(
        &self,
        rest: &Rest,
        voice_num: u8,
        part_staves: u8,
    ) -> mxml::Note {
        let display_step = rest.display_step.as_ref().and_then(|s| {
            ir_step_str_to_mxml(s).map(|step| mxml::DisplayStep {
                attributes: (),
                content: step,
            })
        });
        let display_octave = rest.display_octave.map(|oct| mxml::DisplayOctave {
            attributes: (),
            content: mdt::Octave(oct as u8),
        });

        let rest_el = mxml::Rest {
            attributes: mxml::RestAttributes {
                measure: if rest.is_measure_rest {
                    Some(mdt::YesNo::Yes)
                } else {
                    None
                },
            },
            content: mxml::RestContents {
                display_step,
                display_octave,
            },
        };

        let info = mxml::NoteType::Normal(mxml::NormalInfo {
            chord: None,
            audible: mxml::AudibleType::Rest(rest_el),
            duration: mxml::Duration {
                attributes: (),
                content: mdt::PositiveDivisions(
                    self.duration_to_divisions(&rest.duration).max(1) as u32
                ),
            },
            tie: vec![],
        });

        let voice = Some(mxml::Voice {
            attributes: (),
            content: voice_num.to_string(),
        });

        let note_type = rest
            .duration
            .musicxml_type()
            .and_then(str_to_note_type_value)
            .map(|v| mxml::Type {
                attributes: mxml::TypeAttributes::default(),
                content: v,
            });

        let dot: Vec<mxml::Dot> = (0..rest.duration.dots)
            .map(|_| mxml::Dot {
                attributes: mxml::DotAttributes::default(),
                content: (),
            })
            .collect();

        let staff = if part_staves > 1 || rest.staff > 1 {
            Some(mxml::Staff {
                attributes: (),
                content: mdt::PositiveInteger(rest.staff as u32),
            })
        } else {
            None
        };

        let time_modification =
            if rest.duration.tuplet_actual != 1 || rest.duration.tuplet_normal != 1 {
                Some(mxml::TimeModification {
                    attributes: (),
                    content: mxml::TimeModificationContents {
                        actual_notes: mxml::ActualNotes {
                            attributes: (),
                            content: mdt::NonNegativeInteger(rest.duration.tuplet_actual as u32),
                        },
                        normal_notes: mxml::NormalNotes {
                            attributes: (),
                            content: mdt::NonNegativeInteger(rest.duration.tuplet_normal as u32),
                        },
                        normal_type: None,
                        normal_dot: vec![],
                    },
                })
            } else {
                None
            };

        // Notations (fermata, tuplet display)
        let mut notation_items: Vec<mxml::NotationContentTypes> = Vec::new();

        if let Some(tuplet) = &rest.tuplet {
            notation_items.push(mxml::NotationContentTypes::Tuplet(mxml::Tuplet {
                attributes: mxml::TupletAttributes {
                    r#type: start_stop_to_mxml_ss(&tuplet.tuplet_type),
                    bracket: if tuplet.bracket {
                        Some(mdt::YesNo::Yes)
                    } else {
                        None
                    },
                    default_x: None,
                    default_y: None,
                    id: None,
                    line_shape: None,
                    number: None,
                    placement: None,
                    relative_x: None,
                    relative_y: None,
                    show_number: None,
                    show_type: None,
                },
                content: mxml::TupletContents::default(),
            }));
        }

        if let Some(fermata) = &rest.fermata {
            notation_items.push(build_fermata_notation(fermata));
        }

        let notations = if notation_items.is_empty() {
            vec![]
        } else {
            vec![mxml::Notations {
                attributes: mxml::NotationsAttributes::default(),
                content: mxml::NotationsContents {
                    footnote: None,
                    level: None,
                    notations: notation_items,
                },
            }]
        };

        mxml::Note {
            attributes: mxml::NoteAttributes::default(),
            content: mxml::NoteContents {
                info,
                instrument: vec![],
                footnote: None,
                level: None,
                voice,
                r#type: note_type,
                dot,
                accidental: None,
                time_modification,
                stem: None,
                notehead: None,
                notehead_text: None,
                staff,
                beam: vec![],
                notations,
                lyric: vec![],
                play: None,
                listen: None,
            },
        }
    }

    pub(super) fn build_chord_notes(
        &self,
        chord: &Chord,
        voice_num: u8,
        part_staves: u8,
    ) -> Vec<mxml::Note> {
        chord
            .notes
            .iter()
            .enumerate()
            .map(|(i, note)| self.build_note(note, voice_num, i > 0, chord.arpeggio, part_staves))
            .collect()
    }
}

// ---------------------------------------------------------------------------
// Helper functions
// ---------------------------------------------------------------------------

fn build_ties(ties: &[crate::ir::TieEvent]) -> Vec<mxml::Tie> {
    ties.iter()
        .map(|t| mxml::Tie {
            attributes: mxml::TieAttributes {
                r#type: match t.tie_type {
                    StartStop::Start => mdt::StartStop::Start,
                    StartStop::Stop | StartStop::Continue => mdt::StartStop::Stop,
                },
                time_only: None,
            },
            content: (),
        })
        .collect()
}

fn build_note_notations(note: &Note, chord_arpeggio: Option<ArpeggioType>) -> Vec<mxml::Notations> {
    let has_notations = !note.ties.is_empty()
        || !note.slurs.is_empty()
        || !note.articulations.is_empty()
        || !note.ornaments.is_empty()
        || !note.technicals.is_empty()
        || note.fermata.is_some()
        || note.tuplet.is_some()
        || note.glissando.is_some()
        || note.slide.is_some()
        || chord_arpeggio.is_some();

    if !has_notations {
        return vec![];
    }

    let mut items: Vec<mxml::NotationContentTypes> = Vec::new();

    // Tied (notation level)
    for tie in &note.ties {
        items.push(mxml::NotationContentTypes::Tied(mxml::Tied {
            attributes: mxml::TiedAttributes {
                r#type: start_stop_to_mxml(&tie.tie_type),
                bezier_offset: None,
                bezier_offset2: None,
                bezier_x: None,
                bezier_x2: None,
                bezier_y: None,
                bezier_y2: None,
                color: None,
                dash_length: None,
                default_x: None,
                default_y: None,
                id: None,
                line_type: None,
                number: None,
                orientation: None,
                placement: None,
                relative_x: None,
                relative_y: None,
                space_length: None,
            },
            content: (),
        }));
    }

    // Slurs
    for slur in &note.slurs {
        items.push(mxml::NotationContentTypes::Slur(mxml::Slur {
            attributes: mxml::SlurAttributes {
                r#type: start_stop_to_mxml(&slur.slur_type),
                number: Some(mdt::NumberLevel(slur.number)),
                bezier_offset: None,
                bezier_offset2: None,
                bezier_x: None,
                bezier_x2: None,
                bezier_y: None,
                bezier_y2: None,
                color: None,
                dash_length: None,
                default_x: None,
                default_y: None,
                id: None,
                line_type: None,
                orientation: None,
                placement: None,
                relative_x: None,
                relative_y: None,
                space_length: None,
            },
            content: (),
        }));
    }

    // Tuplet display
    if let Some(tuplet) = &note.tuplet {
        items.push(mxml::NotationContentTypes::Tuplet(mxml::Tuplet {
            attributes: mxml::TupletAttributes {
                r#type: start_stop_to_mxml_ss(&tuplet.tuplet_type),
                bracket: if tuplet.bracket {
                    Some(mdt::YesNo::Yes)
                } else {
                    None
                },
                default_x: None,
                default_y: None,
                id: None,
                line_shape: None,
                number: None,
                placement: None,
                relative_x: None,
                relative_y: None,
                show_number: None,
                show_type: None,
            },
            content: mxml::TupletContents::default(),
        }));
    }

    // Fermata
    if let Some(fermata) = &note.fermata {
        items.push(build_fermata_notation(fermata));
    }

    // Articulations
    if !note.articulations.is_empty() {
        let art_items: Vec<mxml::ArticulationsType> = note
            .articulations
            .iter()
            .filter_map(|art| str_to_articulation_type(&art.name))
            .collect();
        if !art_items.is_empty() {
            items.push(mxml::NotationContentTypes::Articulations(
                mxml::Articulations {
                    attributes: mxml::ArticulationsAttributes::default(),
                    content: art_items,
                },
            ));
        }
    }

    // Ornaments
    if !note.ornaments.is_empty() {
        let orn_items: Vec<mxml::OrnamentType> = note
            .ornaments
            .iter()
            .filter_map(|orn| {
                if orn.name == "tremolo" {
                    let trem_type = if note.two_note_tremolo {
                        if note.tremolo_start {
                            mdt::TremoloType::Start
                        } else {
                            mdt::TremoloType::Stop
                        }
                    } else {
                        mdt::TremoloType::Single
                    };
                    Some(mxml::OrnamentType::Tremolo(mxml::Tremolo {
                        attributes: mxml::TremoloAttributes {
                            r#type: Some(trem_type),
                            ..Default::default()
                        },
                        content: mdt::TremoloMarks(note.tremolo_marks),
                    }))
                } else if let Some(wl_type) = orn.name.strip_prefix("wavy-line-") {
                    let ssc = match wl_type {
                        "start" => mdt::StartStopContinue::Start,
                        "stop" => mdt::StartStopContinue::Stop,
                        "continue" => mdt::StartStopContinue::Continue,
                        _ => mdt::StartStopContinue::Start,
                    };
                    Some(mxml::OrnamentType::WavyLine(mxml::WavyLine {
                        attributes: mxml::WavyLineAttributes {
                            r#type: ssc,
                            acclerate: None,
                            beats: None,
                            color: None,
                            default_x: None,
                            default_y: None,
                            last_beat: None,
                            number: None,
                            placement: None,
                            relative_x: None,
                            relative_y: None,
                            second_beat: None,
                            smufl: None,
                            start_note: None,
                            trill_step: None,
                            two_note_turn: None,
                        },
                        content: (),
                    }))
                } else {
                    str_to_ornament_type(&orn.name)
                }
            })
            .collect();
        if !orn_items.is_empty() {
            items.push(mxml::NotationContentTypes::Ornaments(mxml::Ornaments {
                attributes: mxml::OrnamentsAttributes::default(),
                content: mxml::OrnamentContents {
                    ornaments: orn_items,
                    accidental_mark: vec![],
                },
            }));
        }
    }

    // Technicals
    if !note.technicals.is_empty() {
        let tech_items: Vec<mxml::TechnicalContents> = note
            .technicals
            .iter()
            .filter_map(|tech| str_to_technical_type(&tech.name, &tech.value))
            .collect();
        if !tech_items.is_empty() {
            items.push(mxml::NotationContentTypes::Technical(mxml::Technical {
                attributes: mxml::TechnicalAttributes::default(),
                content: tech_items,
            }));
        }
    }

    // Glissando
    if let Some(gliss) = &note.glissando {
        let line_type = note
            .glissando_line_type
            .as_ref()
            .and_then(|lt| str_to_line_type(lt));
        items.push(mxml::NotationContentTypes::Glissando(mxml::Glissando {
            attributes: mxml::GlissandoAttributes {
                r#type: start_stop_to_mxml_ss(gliss),
                number: Some(mdt::NumberLevel(1)),
                color: None,
                dash_length: None,
                default_x: None,
                default_y: None,
                font_family: None,
                font_size: None,
                font_style: None,
                font_weight: None,
                id: None,
                line_type,
                relative_x: None,
                relative_y: None,
                space_length: None,
            },
            content: String::new(),
        }));
    }

    // Slide
    if let Some(slide) = &note.slide {
        items.push(mxml::NotationContentTypes::Slide(mxml::Slide {
            attributes: mxml::SlideAttributes {
                r#type: start_stop_to_mxml_ss(slide),
                number: Some(mdt::NumberLevel(1)),
                accelerate: None,
                beats: None,
                color: None,
                dash_length: None,
                default_x: None,
                default_y: None,
                first_beat: None,
                font_family: None,
                font_size: None,
                font_style: None,
                font_weight: None,
                id: None,
                last_beat: None,
                line_type: None,
                relative_x: None,
                relative_y: None,
                space_length: None,
            },
            content: String::new(),
        }));
    }

    // Arpeggiate / non-arpeggiate
    if let Some(arp) = chord_arpeggio {
        match arp {
            ArpeggioType::Up => {
                items.push(mxml::NotationContentTypes::Arpeggiate(mxml::Arpeggiate {
                    attributes: mxml::ArpeggiateAttributes {
                        direction: Some(mdt::UpDown::Up),
                        ..Default::default()
                    },
                    content: (),
                }));
            }
            ArpeggioType::Down => {
                items.push(mxml::NotationContentTypes::Arpeggiate(mxml::Arpeggiate {
                    attributes: mxml::ArpeggiateAttributes {
                        direction: Some(mdt::UpDown::Down),
                        ..Default::default()
                    },
                    content: (),
                }));
            }
            ArpeggioType::NonArpeggio => {
                items.push(mxml::NotationContentTypes::NonArpeggiate(
                    mxml::NonArpeggiate {
                        attributes: mxml::NonArpeggiateAttributes {
                            r#type: mdt::TopBottom::Top,
                            color: None,
                            default_x: None,
                            default_y: None,
                            id: None,
                            number: None,
                            placement: None,
                            relative_x: None,
                            relative_y: None,
                        },
                        content: (),
                    },
                ));
            }
        }
    }

    vec![mxml::Notations {
        attributes: mxml::NotationsAttributes::default(),
        content: mxml::NotationsContents {
            footnote: None,
            level: None,
            notations: items,
        },
    }]
}

fn build_fermata_notation(
    fermata: &crate::ir::articulation::Fermata,
) -> mxml::NotationContentTypes {
    let shape = match fermata.shape.as_str() {
        "normal" | "" => mdt::FermataShape::Normal,
        "angled" => mdt::FermataShape::Angled,
        "square" => mdt::FermataShape::Square,
        "double-angled" => mdt::FermataShape::DoubleAngled,
        "double-square" => mdt::FermataShape::DoubleSquare,
        "double-dot" => mdt::FermataShape::DoubleDot,
        "half-curve" => mdt::FermataShape::HalfCurve,
        "curlew" => mdt::FermataShape::Curlew,
        _ => mdt::FermataShape::Empty,
    };
    let mut attrs = mxml::FermataAttributes::default();
    if fermata.inverted {
        attrs.r#type = Some(mdt::UprightInverted::Inverted);
    }
    mxml::NotationContentTypes::Fermata(mxml::Fermata {
        attributes: attrs,
        content: shape,
    })
}

fn build_lyric(syl: &crate::ir::articulation::LyricSyllable) -> mxml::Lyric {
    let syllabic_val = match syl.syllabic {
        crate::ir::articulation::SyllabicType::Single => mdt::Syllabic::Single,
        crate::ir::articulation::SyllabicType::Begin => mdt::Syllabic::Begin,
        crate::ir::articulation::SyllabicType::End => mdt::Syllabic::End,
        crate::ir::articulation::SyllabicType::Middle => mdt::Syllabic::Middle,
    };

    let mut additional = Vec::new();
    if syl.elision {
        additional.push(mxml::AdditionalTextLyric {
            elision: Some(mxml::Elision {
                attributes: mxml::ElisionAttributes::default(),
                content: String::new(),
            }),
            syllabic: None,
            text: mxml::Text {
                attributes: mxml::TextAttributes::default(),
                content: String::new(),
            },
        });
    }

    let extend = if syl.extend {
        Some(mxml::Extend {
            attributes: mxml::ExtendAttributes::default(),
            content: (),
        })
    } else {
        None
    };

    mxml::Lyric {
        attributes: mxml::LyricAttributes {
            number: Some(mdt::NmToken(syl.number.to_string())),
            ..Default::default()
        },
        content: mxml::LyricContents::Text(mxml::TextLyric {
            syllabic: Some(mxml::Syllabic {
                attributes: (),
                content: syllabic_val,
            }),
            text: mxml::Text {
                attributes: mxml::TextAttributes::default(),
                content: syl.text.clone(),
            },
            additional,
            extend,
            end_line: None,
            end_paragraph: None,
            footnote: None,
            level: None,
        }),
    }
}

// ---------------------------------------------------------------------------
// String-to-enum conversion helpers
// ---------------------------------------------------------------------------

fn ir_step_to_mxml(step: &crate::ir::pitch::PitchStep) -> mdt::Step {
    match step {
        crate::ir::pitch::PitchStep::C => mdt::Step::C,
        crate::ir::pitch::PitchStep::D => mdt::Step::D,
        crate::ir::pitch::PitchStep::E => mdt::Step::E,
        crate::ir::pitch::PitchStep::F => mdt::Step::F,
        crate::ir::pitch::PitchStep::G => mdt::Step::G,
        crate::ir::pitch::PitchStep::A => mdt::Step::A,
        crate::ir::pitch::PitchStep::B => mdt::Step::B,
    }
}

fn ir_step_str_to_mxml(s: &str) -> Option<mdt::Step> {
    match s {
        "C" => Some(mdt::Step::C),
        "D" => Some(mdt::Step::D),
        "E" => Some(mdt::Step::E),
        "F" => Some(mdt::Step::F),
        "G" => Some(mdt::Step::G),
        "A" => Some(mdt::Step::A),
        "B" => Some(mdt::Step::B),
        _ => None,
    }
}

fn str_to_note_type_value(s: &str) -> Option<mdt::NoteTypeValue> {
    match s {
        "maxima" => Some(mdt::NoteTypeValue::Maxima),
        "long" => Some(mdt::NoteTypeValue::Long),
        "breve" => Some(mdt::NoteTypeValue::Breve),
        "whole" => Some(mdt::NoteTypeValue::Whole),
        "half" => Some(mdt::NoteTypeValue::Half),
        "quarter" => Some(mdt::NoteTypeValue::Quarter),
        "eighth" => Some(mdt::NoteTypeValue::Eighth),
        "16th" => Some(mdt::NoteTypeValue::Sixteenth),
        "32nd" => Some(mdt::NoteTypeValue::ThirtySecond),
        "64th" => Some(mdt::NoteTypeValue::SixtyFourth),
        "128th" => Some(mdt::NoteTypeValue::OneHundredTwentyEighth),
        "256th" => Some(mdt::NoteTypeValue::TwoHundredFiftySixth),
        "512th" => Some(mdt::NoteTypeValue::FiveHundredTwelfth),
        "1024th" => Some(mdt::NoteTypeValue::OneThousandTwentyFourth),
        _ => None,
    }
}

fn str_to_stem_value(s: &str) -> Option<mdt::StemValue> {
    match s {
        "up" => Some(mdt::StemValue::Up),
        "down" => Some(mdt::StemValue::Down),
        "double" => Some(mdt::StemValue::Double),
        "none" => Some(mdt::StemValue::None),
        _ => None,
    }
}

fn str_to_beam_value(s: &str) -> Option<mdt::BeamValue> {
    match s {
        "begin" => Some(mdt::BeamValue::Begin),
        "continue" => Some(mdt::BeamValue::Continue),
        "end" => Some(mdt::BeamValue::End),
        "forward hook" => Some(mdt::BeamValue::ForwardHook),
        "backward hook" => Some(mdt::BeamValue::BackwardHook),
        _ => None,
    }
}

fn str_to_notehead_value(s: &str) -> Option<mdt::NoteheadValue> {
    match s {
        "x" => Some(mdt::NoteheadValue::X),
        "diamond" => Some(mdt::NoteheadValue::Diamond),
        "square" => Some(mdt::NoteheadValue::Square),
        "cross" => Some(mdt::NoteheadValue::Cross),
        "triangle" => Some(mdt::NoteheadValue::Triangle),
        "circle-x" => Some(mdt::NoteheadValue::CircleX),
        "slash" => Some(mdt::NoteheadValue::Slash),
        "none" => Some(mdt::NoteheadValue::None),
        "normal" => Some(mdt::NoteheadValue::Normal),
        "do" => Some(mdt::NoteheadValue::Do),
        "re" => Some(mdt::NoteheadValue::Re),
        "mi" => Some(mdt::NoteheadValue::Mi),
        "fa" => Some(mdt::NoteheadValue::Fa),
        "so" => Some(mdt::NoteheadValue::So),
        "la" => Some(mdt::NoteheadValue::La),
        "ti" => Some(mdt::NoteheadValue::Ti),
        _ => None,
    }
}

fn str_to_articulation_type(name: &str) -> Option<mxml::ArticulationsType> {
    match name {
        "accent" => Some(mxml::ArticulationsType::Accent(mxml::Accent {
            attributes: mxml::AccentAttributes::default(),
            content: (),
        })),
        "strong-accent" => Some(mxml::ArticulationsType::StrongAccent(mxml::StrongAccent {
            attributes: mxml::StrongAccentAttributes::default(),
            content: (),
        })),
        "staccato" => Some(mxml::ArticulationsType::Staccato(mxml::Staccato {
            attributes: mxml::StaccatoAttributes::default(),
            content: (),
        })),
        "tenuto" => Some(mxml::ArticulationsType::Tenuto(mxml::Tenuto {
            attributes: mxml::TenutoAttributes::default(),
            content: (),
        })),
        "detached-legato" => Some(mxml::ArticulationsType::DetachedLegato(
            mxml::DetachedLegato {
                attributes: mxml::DetachedLegatoAttributes::default(),
                content: (),
            },
        )),
        "staccatissimo" => Some(mxml::ArticulationsType::Staccatissimo(
            mxml::Staccatissimo {
                attributes: mxml::StaccatissimoAttributes::default(),
                content: (),
            },
        )),
        "spiccato" => Some(mxml::ArticulationsType::Spiccato(mxml::Spiccato {
            attributes: mxml::SpiccatoAttributes::default(),
            content: (),
        })),
        "breath-mark" => Some(mxml::ArticulationsType::BreathMark(mxml::BreathMark {
            attributes: mxml::BreathMarkAttributes::default(),
            content: mdt::BreathMarkValue::Comma,
        })),
        "caesura" => Some(mxml::ArticulationsType::Caesura(mxml::Caesura {
            attributes: mxml::CaesuraAttributes::default(),
            content: mdt::CaesuraValue::Normal,
        })),
        "stress" => Some(mxml::ArticulationsType::Stress(mxml::Stress {
            attributes: mxml::StressAttributes::default(),
            content: (),
        })),
        "unstress" => Some(mxml::ArticulationsType::Unstress(mxml::Unstress {
            attributes: mxml::UnstressAttributes::default(),
            content: (),
        })),
        _ => None,
    }
}

fn str_to_ornament_type(name: &str) -> Option<mxml::OrnamentType> {
    match name {
        "trill-mark" => Some(mxml::OrnamentType::TrillMark(mxml::TrillMark {
            attributes: mxml::TrillMarkAttributes::default(),
            content: (),
        })),
        "turn" => Some(mxml::OrnamentType::Turn(mxml::Turn {
            attributes: mxml::TurnAttributes::default(),
            content: (),
        })),
        "inverted-turn" => Some(mxml::OrnamentType::InvertedTurn(mxml::InvertedTurn {
            attributes: mxml::InvertedTurnAttributes::default(),
            content: (),
        })),
        "mordent" => Some(mxml::OrnamentType::Mordent(mxml::Mordent {
            attributes: mxml::MordentAttributes::default(),
            content: (),
        })),
        "inverted-mordent" => Some(mxml::OrnamentType::InvertedMordent(mxml::InvertedMordent {
            attributes: mxml::InvertedMordentAttributes::default(),
            content: (),
        })),
        "schleifer" => Some(mxml::OrnamentType::Schleifer(mxml::Schleifer {
            attributes: mxml::SchleiferAttributes::default(),
            content: (),
        })),
        _ => None,
    }
}

fn str_to_technical_type(name: &str, value: &str) -> Option<mxml::TechnicalContents> {
    match name {
        "fingering" => Some(mxml::TechnicalContents::Fingering(mxml::Fingering {
            attributes: mxml::FingeringAttributes::default(),
            content: value.to_string(),
        })),
        "up-bow" => Some(mxml::TechnicalContents::UpBow(mxml::UpBow {
            attributes: mxml::UpBowAttributes::default(),
            content: (),
        })),
        "down-bow" => Some(mxml::TechnicalContents::DownBow(mxml::DownBow {
            attributes: mxml::DownBowAttributes::default(),
            content: (),
        })),
        "harmonic" => Some(mxml::TechnicalContents::Harmonic(mxml::Harmonic {
            attributes: mxml::HarmonicAttributes::default(),
            content: mxml::HarmonicContents::default(),
        })),
        "open-string" => Some(mxml::TechnicalContents::OpenString(mxml::OpenString {
            attributes: mxml::OpenStringAttributes::default(),
            content: (),
        })),
        "snap-pizzicato" => Some(mxml::TechnicalContents::SnapPizzicato(
            mxml::SnapPizzicato {
                attributes: mxml::SnapPizzicatoAttributes::default(),
                content: (),
            },
        )),
        "stopped" => Some(mxml::TechnicalContents::Stopped(mxml::Stopped {
            attributes: mxml::StoppedAttributes::default(),
            content: (),
        })),
        "string" => Some(mxml::TechnicalContents::StringNumber(mxml::StringNumber {
            attributes: mxml::StringAttributes::default(),
            content: mdt::StringNumber(value.parse().unwrap_or(0)),
        })),
        "fret" => Some(mxml::TechnicalContents::Fret(mxml::Fret {
            attributes: mxml::FretAttributes::default(),
            content: mdt::NonNegativeInteger(value.parse().unwrap_or(0)),
        })),
        _ => None,
    }
}

fn str_to_line_type(s: &str) -> Option<mdt::LineType> {
    match s {
        "solid" => Some(mdt::LineType::Solid),
        "dashed" => Some(mdt::LineType::Dashed),
        "dotted" => Some(mdt::LineType::Dotted),
        "wavy" => Some(mdt::LineType::Wavy),
        _ => None,
    }
}
