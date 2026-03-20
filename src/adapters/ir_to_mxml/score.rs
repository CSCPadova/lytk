//! Score-level musicxml element construction.

use super::IrToMxmlAdapter;
use crate::ir::score::{PageLayout, Score, ScoreChild};

use musicxml::datatypes as mdt;
use musicxml::elements as mxml;

impl IrToMxmlAdapter {
    /// Build the top-level `ScorePartwise` element.
    pub(super) fn build_score_partwise(&self, score: &Score) -> mxml::ScorePartwise {
        let version = Some(mdt::Token(self.version.clone()));

        // Movement title
        let movement_title = score.metadata.title.as_ref().map(|t| mxml::MovementTitle {
            attributes: (),
            content: t.clone(),
        });

        // Identification
        let identification = Some(self.build_identification(score));

        // Defaults (page layout / scaling)
        let defaults = score.page_layout.as_ref().map(|pl| self.build_defaults(pl));

        // Credit elements (subtitle)
        let credit: Vec<mxml::Credit> = if let Some(subtitle) = &score.metadata.subtitle {
            vec![mxml::Credit {
                attributes: mxml::CreditAttributes::default(),
                content: mxml::CreditContents {
                    credit_type: vec![mxml::CreditType {
                        attributes: (),
                        content: "subtitle".to_string(),
                    }],
                    link: vec![],
                    bookmark: vec![],
                    credit: mxml::CreditSubcontents::Text(mxml::CreditTextContents {
                        credit_words: Some(mxml::CreditWords {
                            attributes: mxml::CreditWordsAttributes::default(),
                            content: subtitle.clone(),
                        }),
                        credit_symbol: None,
                        additional: vec![],
                    }),
                },
            }]
        } else {
            vec![]
        };

        // Part list
        let part_list = self.build_part_list(score);

        // Parts
        let part: Vec<mxml::Part> = score.parts().iter().map(|p| self.build_part(p)).collect();

        mxml::ScorePartwise {
            attributes: mxml::ScorePartwiseAttributes { version },
            content: mxml::ScorePartwiseContents {
                work: None,
                movement_number: None,
                movement_title,
                identification,
                defaults,
                credit,
                part_list,
                part,
            },
        }
    }

    fn build_identification(&self, score: &Score) -> mxml::Identification {
        let meta = &score.metadata;
        let mut creator: Vec<mxml::Creator> = Vec::new();

        if let Some(composer) = &meta.composer {
            creator.push(mxml::Creator {
                attributes: mxml::CreatorAttributes {
                    r#type: Some(mdt::Token("composer".to_string())),
                },
                content: composer.clone(),
            });
        }
        if let Some(arranger) = &meta.arranger {
            creator.push(mxml::Creator {
                attributes: mxml::CreatorAttributes {
                    r#type: Some(mdt::Token("arranger".to_string())),
                },
                content: arranger.clone(),
            });
        }
        if let Some(lyricist) = &meta.lyricist {
            creator.push(mxml::Creator {
                attributes: mxml::CreatorAttributes {
                    r#type: Some(mdt::Token("lyricist".to_string())),
                },
                content: lyricist.clone(),
            });
        }

        // Extra creators (sorted for deterministic output)
        let mut extra_keys: Vec<&String> = meta.extra.keys().collect();
        extra_keys.sort();
        for key in extra_keys {
            if let Some(value) = meta.extra.get(key) {
                creator.push(mxml::Creator {
                    attributes: mxml::CreatorAttributes {
                        r#type: Some(mdt::Token(key.clone())),
                    },
                    content: value.clone(),
                });
            }
        }

        let rights: Vec<mxml::Rights> = meta
            .rights
            .iter()
            .map(|(rtype, rtext)| mxml::Rights {
                attributes: mxml::RightsAttributes {
                    r#type: if rtype.is_empty() {
                        None
                    } else {
                        Some(mdt::Token(rtype.clone()))
                    },
                },
                content: rtext.clone(),
            })
            .collect();

        // Encoding
        let encoding = Some(mxml::Encoding {
            attributes: (),
            content: vec![mxml::EncodingContents::Software(mxml::Software {
                attributes: (),
                content: "lytk".to_string(),
            })],
        });

        mxml::Identification {
            attributes: (),
            content: mxml::IdentificationContents {
                creator,
                rights,
                encoding,
                source: None,
                relation: vec![],
                miscellaneous: None,
            },
        }
    }

    fn build_part_list(&self, score: &Score) -> mxml::PartList {
        let mut content: Vec<mxml::PartListElement> = Vec::new();
        let mut auto_group_number: u8 = 0;

        for child in &score.children {
            match child {
                ScoreChild::Part(part) => {
                    content.push(mxml::PartListElement::ScorePart(
                        self.build_score_part(part),
                    ));
                }
                ScoreChild::PartGroup(group) => {
                    let num = if group.number > 0 {
                        group.number
                    } else {
                        auto_group_number += 1;
                        auto_group_number
                    };

                    // part-group start
                    let symbol = match group.bracket.as_str() {
                        "brace" => mdt::GroupSymbolValue::Brace,
                        "line" => mdt::GroupSymbolValue::Line,
                        "square" => mdt::GroupSymbolValue::Square,
                        _ => mdt::GroupSymbolValue::Bracket,
                    };
                    content.push(mxml::PartListElement::PartGroup(mxml::PartGroup {
                        attributes: mxml::PartGroupAttributes {
                            r#type: mdt::StartStop::Start,
                            number: Some(mdt::Token(num.to_string())),
                        },
                        content: mxml::PartGroupContents {
                            group_name: if group.name.is_empty() {
                                None
                            } else {
                                Some(mxml::GroupName {
                                    attributes: mxml::GroupNameAttributes::default(),
                                    content: group.name.clone(),
                                })
                            },
                            group_name_display: None,
                            group_abbreviation: None,
                            group_abbreviation_display: None,
                            group_symbol: Some(mxml::GroupSymbol {
                                attributes: mxml::GroupSymbolAttributes::default(),
                                content: symbol,
                            }),
                            group_barline: None,
                            group_time: None,
                            footnote: None,
                            level: None,
                        },
                    }));

                    // Nested score-parts
                    for sc in &group.children {
                        if let ScoreChild::Part(p) = sc {
                            content
                                .push(mxml::PartListElement::ScorePart(self.build_score_part(p)));
                        }
                    }

                    // part-group stop
                    content.push(mxml::PartListElement::PartGroup(mxml::PartGroup {
                        attributes: mxml::PartGroupAttributes {
                            r#type: mdt::StartStop::Stop,
                            number: Some(mdt::Token(num.to_string())),
                        },
                        content: mxml::PartGroupContents::default(),
                    }));
                }
            }
        }

        mxml::PartList {
            attributes: (),
            content: mxml::PartListContents { content },
        }
    }

    fn build_score_part(&self, part: &crate::ir::Part) -> mxml::ScorePart {
        let id = if part.part_id.is_empty() {
            "P1"
        } else {
            &part.part_id
        };

        let part_abbreviation = if part.abbreviation.is_empty() {
            None
        } else {
            Some(mxml::PartAbbreviation {
                attributes: mxml::PartAbbreviationAttributes::default(),
                content: part.abbreviation.clone(),
            })
        };

        // Score-instrument + MIDI instrument
        let has_midi =
            part.midi_channel > 0 || part.midi_program > 0 || !part.midi_instrument.is_empty();

        let (score_instrument, midi_instrument) = if has_midi {
            let inst_id = format!("{}-I1", id);

            let si = mxml::ScoreInstrument {
                attributes: mxml::ScoreInstrumentAttributes {
                    id: mdt::Id(inst_id.clone()),
                },
                content: mxml::ScoreInstrumentContents {
                    instrument_name: mxml::InstrumentName {
                        attributes: (),
                        content: if part.name.is_empty() {
                            "Instrument".to_string()
                        } else {
                            part.name.clone()
                        },
                    },
                    instrument_abbreviation: None,
                    instrument_sound: None,
                    solo: None,
                    ensemble: None,
                    virtual_instrument: None,
                },
            };

            let mut mi_content = mxml::MidiInstrumentContents {
                midi_channel: None,
                midi_name: None,
                midi_bank: None,
                midi_program: None,
                midi_unpitched: None,
                volume: None,
                pan: None,
                elevation: None,
            };
            if part.midi_channel > 0 {
                mi_content.midi_channel = Some(mxml::MidiChannel {
                    attributes: (),
                    content: mdt::Midi16(part.midi_channel),
                });
            }
            if !part.midi_instrument.is_empty() {
                mi_content.midi_name = Some(mxml::MidiName {
                    attributes: (),
                    content: part.midi_instrument.clone(),
                });
            }
            if part.midi_program > 0 {
                mi_content.midi_program = Some(mxml::MidiProgram {
                    attributes: (),
                    content: mdt::Midi128(part.midi_program),
                });
            }

            let mi = mxml::MidiInstrument {
                attributes: mxml::MidiInstrumentAttributes {
                    id: mdt::IdRef(inst_id),
                },
                content: mi_content,
            };

            (vec![si], vec![mi])
        } else {
            (vec![], vec![])
        };

        mxml::ScorePart {
            attributes: mxml::ScorePartAttributes {
                id: mdt::Id(id.to_string()),
            },
            content: mxml::ScorePartContents {
                identification: None,
                part_link: vec![],
                part_name: mxml::PartName {
                    attributes: mxml::PartNameAttributes::default(),
                    content: part.name.clone(),
                },
                part_name_display: None,
                part_abbreviation,
                part_abbreviation_display: None,
                group: vec![],
                score_instrument,
                player: vec![],
                midi_device: vec![],
                midi_instrument,
            },
        }
    }

    /// Build `<defaults>` element with page layout and scaling.
    fn build_defaults(&self, pl: &PageLayout) -> mxml::Defaults {
        // Compute mm-per-tenth from stored staff_size (points), falling back to
        // the standard MusicXML value of 7.056 mm / 40 tenths.
        let mm_per_tenth = if let Some(ss) = pl.staff_size {
            ss * 25.4 / (40.0 * 72.27)
        } else {
            7.056 / 40.0
        };

        let has_dimensions = pl.page_height.is_some()
            || pl.page_width.is_some()
            || pl.left_margin.is_some()
            || pl.system_distance.is_some();

        let scaling = if pl.staff_size.is_some() || has_dimensions {
            Some(mxml::Scaling {
                attributes: (),
                content: mxml::ScalingContents {
                    millimeters: mxml::Millimeters {
                        attributes: (),
                        content: mdt::Millimeters(mm_per_tenth * 40.0),
                    },
                    tenths: mxml::Tenths {
                        attributes: (),
                        content: mdt::Tenths(40.0),
                    },
                },
            })
        } else {
            None
        };

        let cm_to_tenths = |cm: f64| cm * 10.0 / mm_per_tenth;

        // Page layout
        let has_page = pl.page_height.is_some()
            || pl.page_width.is_some()
            || pl.left_margin.is_some()
            || pl.right_margin.is_some()
            || pl.top_margin.is_some()
            || pl.bottom_margin.is_some();

        let page_layout = if has_page {
            let has_margins = pl.left_margin.is_some()
                || pl.right_margin.is_some()
                || pl.top_margin.is_some()
                || pl.bottom_margin.is_some();

            let page_margins = if has_margins {
                vec![mxml::PageMargins {
                    attributes: mxml::PageMarginsAttributes {
                        r#type: Some(mdt::MarginType::Both),
                    },
                    content: mxml::PageMarginsContents {
                        left_margin: mxml::LeftMargin {
                            attributes: (),
                            content: mdt::Tenths(pl.left_margin.map(&cm_to_tenths).unwrap_or(0.0)),
                        },
                        right_margin: mxml::RightMargin {
                            attributes: (),
                            content: mdt::Tenths(pl.right_margin.map(&cm_to_tenths).unwrap_or(0.0)),
                        },
                        top_margin: mxml::TopMargin {
                            attributes: (),
                            content: mdt::Tenths(pl.top_margin.map(&cm_to_tenths).unwrap_or(0.0)),
                        },
                        bottom_margin: mxml::BottomMargin {
                            attributes: (),
                            content: mdt::Tenths(
                                pl.bottom_margin.map(&cm_to_tenths).unwrap_or(0.0),
                            ),
                        },
                    },
                }]
            } else {
                vec![]
            };

            Some(mxml::PageLayout {
                attributes: (),
                content: mxml::PageLayoutContents {
                    page_height: pl.page_height.map(|h| mxml::PageHeight {
                        attributes: (),
                        content: mdt::Tenths(cm_to_tenths(h)),
                    }),
                    page_width: pl.page_width.map(|w| mxml::PageWidth {
                        attributes: (),
                        content: mdt::Tenths(cm_to_tenths(w)),
                    }),
                    page_margins,
                },
            })
        } else {
            None
        };

        // System layout
        let has_system = pl.system_distance.is_some() || pl.top_system_distance.is_some();
        let system_layout = if has_system {
            Some(mxml::SystemLayout {
                attributes: (),
                content: mxml::SystemLayoutContents {
                    system_margins: Some(mxml::SystemMargins {
                        attributes: (),
                        content: mxml::SystemMarginsContents {
                            left_margin: mxml::LeftMargin {
                                attributes: (),
                                content: mdt::Tenths(0.0),
                            },
                            right_margin: mxml::RightMargin {
                                attributes: (),
                                content: mdt::Tenths(0.0),
                            },
                        },
                    }),
                    system_distance: pl.system_distance.map(|sd| mxml::SystemDistance {
                        attributes: (),
                        content: mdt::Tenths(cm_to_tenths(sd)),
                    }),
                    top_system_distance: pl.top_system_distance.map(|tsd| {
                        mxml::TopSystemDistance {
                            attributes: (),
                            content: mdt::Tenths(cm_to_tenths(tsd)),
                        }
                    }),
                    system_dividers: None,
                },
            })
        } else {
            None
        };

        mxml::Defaults {
            attributes: (),
            content: mxml::DefaultsContents {
                scaling,
                concert_score: None,
                page_layout,
                system_layout,
                staff_layout: vec![],
                appearance: None,
                music_font: None,
                word_font: None,
                lyric_font: vec![],
                lyric_language: vec![],
            },
        }
    }
}
