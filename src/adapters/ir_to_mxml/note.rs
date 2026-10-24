//! Note, chord, and rest → musicxml element construction.

use super::helpers::{alter_to_accidental_value, start_stop_to_mxml, start_stop_to_mxml_ss};
use super::IrToMxmlAdapter;
use crate::ir::articulation::{
    ArticulationType, BeamValue, OrnamentType, Placement, StartStop, TechnicalType,
};
use crate::ir::note::{ArpeggioType, Chord, LineType, Note, Notehead, Rest, StemDirection};
use crate::ir::pitch::AccidentalDisplay;

use crate::ir::duration::NoteType;
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
            // Fractional alters (microtones) can't pass the crate's i16 —
            // encode them; `convert()` decodes the serialized string back to
            // a decimal. See `adapters::encode_fractional_alters`.
            let encoded = if alter_val.fract() != 0.0 {
                crate::adapters::ALTER_ENC_BASE + (alter_val * 100.0).round() as i32
            } else {
                alter_val as i32
            };
            Some(mxml::Alter {
                attributes: (),
                content: mdt::Semitones(encoded as i16),
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
        let note_type = note.duration.note_type().map(|t| mxml::Type {
            attributes: mxml::TypeAttributes::default(),
            content: note_type_value(t),
        });

        // Dots
        let dot: Vec<mxml::Dot> = (0..note.duration.dots)
            .map(|_| mxml::Dot {
                attributes: mxml::DotAttributes::default(),
                content: (),
            })
            .collect();

        // Notehead
        let notehead = note
            .notehead
            .filter(|h| *h != Notehead::Normal)
            .map(|head| mxml::Notehead {
                attributes: mxml::NoteheadAttributes::default(),
                content: notehead_value(head),
            });

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
        let stem = note.stem_direction.map(|s| mxml::Stem {
            attributes: mxml::StemAttributes::default(),
            content: stem_value(s),
        });

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
            .map(|b| mxml::Beam {
                attributes: mxml::BeamAttributes {
                    number: Some(mdt::BeamLevel(b.number)),
                    ..Default::default()
                },
                content: beam_value(b.beam_type),
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
        // A known velocity is MusicXML's `dynamics`, a percentage of 90.
        if let Some(v) = note.velocity {
            attrs.dynamics = Some(mdt::NonNegativeDecimal(
                (v as f64 / 0.9 * 100.0).round() / 100.0,
            ));
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
        let display_step = rest.display_step.map(|s| mxml::DisplayStep {
            attributes: (),
            content: ir_step_to_mxml(&s),
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

        let note_type = rest.duration.note_type().map(|t| mxml::Type {
            attributes: mxml::TypeAttributes::default(),
            content: note_type_value(t),
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
                placement: above_below(slur.placement),
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
            .filter_map(|art| str_to_articulation_type(&art.name, above_below(art.placement)))
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
                if orn.name == OrnamentType::Tremolo {
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
                } else if let Some(ssc) = match orn.name {
                    OrnamentType::WavyLineStart => Some(mdt::StartStopContinue::Start),
                    OrnamentType::WavyLineStop => Some(mdt::StartStopContinue::Stop),
                    OrnamentType::WavyLineContinue => Some(mdt::StartStopContinue::Continue),
                    _ => None,
                } {
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
                    str_to_ornament_type(&orn.name, above_below(orn.placement))
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
        let line_type = note.glissando_line_type.map(line_type);
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
    use crate::ir::articulation::FermataShape as S;
    let shape = match fermata.shape {
        S::Normal => mdt::FermataShape::Normal,
        S::Angled => mdt::FermataShape::Angled,
        S::Square => mdt::FermataShape::Square,
        S::DoubleAngled => mdt::FermataShape::DoubleAngled,
        S::DoubleSquare => mdt::FermataShape::DoubleSquare,
        S::DoubleDot => mdt::FermataShape::DoubleDot,
        S::HalfCurve => mdt::FermataShape::HalfCurve,
        S::Curlew => mdt::FermataShape::Curlew,
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

/// A `<lyric>`: an elided syllable (`my‿a`) as its texts with `<elision>`
/// between, each with the syllabic its place in the word gives.
fn build_lyric(syl: &crate::ir::articulation::LyricSyllable) -> mxml::Lyric {
    use crate::ir::articulation::SyllabicType;
    let syllabic = |s: SyllabicType| {
        Some(mxml::Syllabic {
            attributes: (),
            content: match s {
                SyllabicType::Single => mdt::Syllabic::Single,
                SyllabicType::Begin => mdt::Syllabic::Begin,
                SyllabicType::End => mdt::Syllabic::End,
                SyllabicType::Middle => mdt::Syllabic::Middle,
            },
        })
    };
    let text = |t: &str| mxml::Text {
        attributes: mxml::TextAttributes::default(),
        content: t.to_string(),
    };
    let words: Vec<&str> = if syl.elision {
        syl.text.split('\u{203F}').collect()
    } else {
        vec![syl.text.as_str()]
    };
    // The first word starts as the syllable does, the last ends as it does.
    let starts = matches!(syl.syllabic, SyllabicType::Single | SyllabicType::Begin);
    let ends = matches!(syl.syllabic, SyllabicType::Single | SyllabicType::End);
    let last = words.len() - 1;
    let syllabic_of = |k: usize| match (k > 0 || starts, k < last || ends) {
        (true, true) => SyllabicType::Single,
        (true, false) => SyllabicType::Begin,
        (false, true) => SyllabicType::End,
        (false, false) => SyllabicType::Middle,
    };
    let additional = (1..=last)
        .map(|k| mxml::AdditionalTextLyric {
            elision: Some(mxml::Elision {
                attributes: mxml::ElisionAttributes::default(),
                content: "\u{203F}".to_string(),
            }),
            syllabic: syllabic(syllabic_of(k)),
            text: text(words[k]),
        })
        .collect();
    let extend = syl.extend.then(|| mxml::Extend {
        attributes: mxml::ExtendAttributes::default(),
        content: (),
    });
    mxml::Lyric {
        attributes: mxml::LyricAttributes {
            number: Some(mdt::NmToken(syl.number.to_string())),
            name: syl.name.clone().map(mdt::Token),
            ..Default::default()
        },
        content: mxml::LyricContents::Text(mxml::TextLyric {
            syllabic: syllabic(syllabic_of(0)),
            text: text(words[0]),
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

pub(super) fn ir_step_to_mxml(step: &crate::ir::pitch::PitchStep) -> mdt::Step {
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

pub(super) fn note_type_value(t: NoteType) -> mdt::NoteTypeValue {
    use mdt::NoteTypeValue as V;
    match t {
        NoteType::Maxima => V::Maxima,
        NoteType::Long => V::Long,
        NoteType::Breve => V::Breve,
        NoteType::Whole => V::Whole,
        NoteType::Half => V::Half,
        NoteType::Quarter => V::Quarter,
        NoteType::Eighth => V::Eighth,
        NoteType::Sixteenth => V::Sixteenth,
        NoteType::ThirtySecond => V::ThirtySecond,
        NoteType::SixtyFourth => V::SixtyFourth,
        NoteType::OneHundredTwentyEighth => V::OneHundredTwentyEighth,
        NoteType::TwoHundredFiftySixth => V::TwoHundredFiftySixth,
        NoteType::FiveHundredTwelfth => V::FiveHundredTwelfth,
        NoteType::OneThousandTwentyFourth => V::OneThousandTwentyFourth,
    }
}

fn stem_value(stem: StemDirection) -> mdt::StemValue {
    match stem {
        StemDirection::Up => mdt::StemValue::Up,
        StemDirection::Down => mdt::StemValue::Down,
        StemDirection::Double => mdt::StemValue::Double,
        StemDirection::NoStem => mdt::StemValue::None,
    }
}

fn beam_value(beam: BeamValue) -> mdt::BeamValue {
    match beam {
        BeamValue::Begin => mdt::BeamValue::Begin,
        BeamValue::Continue => mdt::BeamValue::Continue,
        BeamValue::End => mdt::BeamValue::End,
        BeamValue::ForwardHook => mdt::BeamValue::ForwardHook,
        BeamValue::BackwardHook => mdt::BeamValue::BackwardHook,
    }
}

fn notehead_value(head: Notehead) -> mdt::NoteheadValue {
    use mdt::NoteheadValue as V;
    match head {
        Notehead::Slash => V::Slash,
        Notehead::Triangle => V::Triangle,
        Notehead::Diamond => V::Diamond,
        Notehead::Square => V::Square,
        Notehead::Cross => V::Cross,
        Notehead::X => V::X,
        Notehead::CircleX => V::CircleX,
        Notehead::InvertedTriangle => V::InvertedTriangle,
        Notehead::ArrowDown => V::ArrowDown,
        Notehead::ArrowUp => V::ArrowUp,
        Notehead::Circled => V::Circled,
        Notehead::Slashed => V::Slashed,
        Notehead::BackSlashed => V::BackSlashed,
        Notehead::Normal => V::Normal,
        Notehead::Cluster => V::Cluster,
        Notehead::CircleDot => V::CircleDot,
        Notehead::LeftTriangle => V::LeftTriangle,
        Notehead::Rectangle => V::Rectangle,
        Notehead::NoHead => V::None,
        Notehead::Do => V::Do,
        Notehead::Re => V::Re,
        Notehead::Mi => V::Mi,
        Notehead::Fa => V::Fa,
        Notehead::FaUp => V::FaUp,
        Notehead::So => V::So,
        Notehead::La => V::La,
        Notehead::Ti => V::Ti,
        Notehead::Other => V::Other,
    }
}

fn line_type(line: LineType) -> mdt::LineType {
    match line {
        LineType::Solid => mdt::LineType::Solid,
        LineType::Dashed => mdt::LineType::Dashed,
        LineType::Dotted => mdt::LineType::Dotted,
        LineType::Wavy => mdt::LineType::Wavy,
    }
}

fn str_to_articulation_type(
    name: &ArticulationType,
    placement: Option<mdt::AboveBelow>,
) -> Option<mxml::ArticulationsType> {
    match name {
        ArticulationType::Accent => Some(mxml::ArticulationsType::Accent(mxml::Accent {
            attributes: mxml::AccentAttributes {
                placement,
                ..Default::default()
            },
            content: (),
        })),
        ArticulationType::StrongAccent => {
            Some(mxml::ArticulationsType::StrongAccent(mxml::StrongAccent {
                attributes: mxml::StrongAccentAttributes {
                    placement,
                    ..Default::default()
                },
                content: (),
            }))
        }
        ArticulationType::Staccato => Some(mxml::ArticulationsType::Staccato(mxml::Staccato {
            attributes: mxml::StaccatoAttributes {
                placement,
                ..Default::default()
            },
            content: (),
        })),
        ArticulationType::Tenuto => Some(mxml::ArticulationsType::Tenuto(mxml::Tenuto {
            attributes: mxml::TenutoAttributes {
                placement,
                ..Default::default()
            },
            content: (),
        })),
        ArticulationType::DetachedLegato => Some(mxml::ArticulationsType::DetachedLegato(
            mxml::DetachedLegato {
                attributes: mxml::DetachedLegatoAttributes {
                    placement,
                    ..Default::default()
                },
                content: (),
            },
        )),
        ArticulationType::Staccatissimo => Some(mxml::ArticulationsType::Staccatissimo(
            mxml::Staccatissimo {
                attributes: mxml::StaccatissimoAttributes {
                    placement,
                    ..Default::default()
                },
                content: (),
            },
        )),
        ArticulationType::Spiccato => Some(mxml::ArticulationsType::Spiccato(mxml::Spiccato {
            attributes: mxml::SpiccatoAttributes {
                placement,
                ..Default::default()
            },
            content: (),
        })),
        ArticulationType::BreathMark => {
            Some(mxml::ArticulationsType::BreathMark(mxml::BreathMark {
                attributes: mxml::BreathMarkAttributes {
                    placement,
                    ..Default::default()
                },
                content: mdt::BreathMarkValue::Comma,
            }))
        }
        ArticulationType::Caesura => Some(mxml::ArticulationsType::Caesura(mxml::Caesura {
            attributes: mxml::CaesuraAttributes {
                placement,
                ..Default::default()
            },
            content: mdt::CaesuraValue::Normal,
        })),
        ArticulationType::Stress => Some(mxml::ArticulationsType::Stress(mxml::Stress {
            attributes: mxml::StressAttributes {
                placement,
                ..Default::default()
            },
            content: (),
        })),
        ArticulationType::Unstress => Some(mxml::ArticulationsType::Unstress(mxml::Unstress {
            attributes: mxml::UnstressAttributes {
                placement,
                ..Default::default()
            },
            content: (),
        })),
        _ => None,
    }
}

fn str_to_ornament_type(
    name: &OrnamentType,
    placement: Option<mdt::AboveBelow>,
) -> Option<mxml::OrnamentType> {
    match name {
        OrnamentType::TrillMark => Some(mxml::OrnamentType::TrillMark(mxml::TrillMark {
            attributes: mxml::TrillMarkAttributes {
                placement,
                ..Default::default()
            },
            content: (),
        })),
        OrnamentType::Turn => Some(mxml::OrnamentType::Turn(mxml::Turn {
            attributes: mxml::TurnAttributes {
                placement,
                ..Default::default()
            },
            content: (),
        })),
        OrnamentType::InvertedTurn => Some(mxml::OrnamentType::InvertedTurn(mxml::InvertedTurn {
            attributes: mxml::InvertedTurnAttributes {
                placement,
                ..Default::default()
            },
            content: (),
        })),
        OrnamentType::Mordent => Some(mxml::OrnamentType::Mordent(mxml::Mordent {
            attributes: mxml::MordentAttributes {
                placement,
                ..Default::default()
            },
            content: (),
        })),
        OrnamentType::InvertedMordent => {
            Some(mxml::OrnamentType::InvertedMordent(mxml::InvertedMordent {
                attributes: mxml::InvertedMordentAttributes {
                    placement,
                    ..Default::default()
                },
                content: (),
            }))
        }
        OrnamentType::Schleifer => Some(mxml::OrnamentType::Schleifer(mxml::Schleifer {
            attributes: mxml::SchleiferAttributes {
                placement,
                ..Default::default()
            },
            content: (),
        })),
        _ => None,
    }
}

fn str_to_technical_type(name: &TechnicalType, value: &str) -> Option<mxml::TechnicalContents> {
    match name {
        TechnicalType::Fingering => Some(mxml::TechnicalContents::Fingering(mxml::Fingering {
            attributes: mxml::FingeringAttributes::default(),
            content: value.to_string(),
        })),
        TechnicalType::UpBow => Some(mxml::TechnicalContents::UpBow(mxml::UpBow {
            attributes: mxml::UpBowAttributes::default(),
            content: (),
        })),
        TechnicalType::DownBow => Some(mxml::TechnicalContents::DownBow(mxml::DownBow {
            attributes: mxml::DownBowAttributes::default(),
            content: (),
        })),
        TechnicalType::Harmonic => Some(mxml::TechnicalContents::Harmonic(mxml::Harmonic {
            attributes: mxml::HarmonicAttributes::default(),
            content: mxml::HarmonicContents::default(),
        })),
        TechnicalType::OpenString => Some(mxml::TechnicalContents::OpenString(mxml::OpenString {
            attributes: mxml::OpenStringAttributes::default(),
            content: (),
        })),
        TechnicalType::SnapPizzicato => Some(mxml::TechnicalContents::SnapPizzicato(
            mxml::SnapPizzicato {
                attributes: mxml::SnapPizzicatoAttributes::default(),
                content: (),
            },
        )),
        TechnicalType::Stopped => Some(mxml::TechnicalContents::Stopped(mxml::Stopped {
            attributes: mxml::StoppedAttributes::default(),
            content: (),
        })),
        TechnicalType::String => Some(mxml::TechnicalContents::StringNumber(mxml::StringNumber {
            attributes: mxml::StringAttributes::default(),
            content: mdt::StringNumber(value.parse().unwrap_or(0)),
        })),
        TechnicalType::Fret => Some(mxml::TechnicalContents::Fret(mxml::Fret {
            attributes: mxml::FretAttributes::default(),
            content: mdt::NonNegativeInteger(value.parse().unwrap_or(0)),
        })),
        _ => None,
    }
}

/// MusicXML's `placement` for the IR's (none when unspecified).
fn above_below(p: Placement) -> Option<mdt::AboveBelow> {
    match p {
        Placement::Above => Some(mdt::AboveBelow::Above),
        Placement::Below => Some(mdt::AboveBelow::Below),
        Placement::Unspecified => None,
    }
}
