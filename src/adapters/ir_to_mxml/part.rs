//! Part-level and measure-level musicxml element construction.

use super::IrToMxmlAdapter;
use crate::ir::direction::BarlineType;
use crate::ir::harmony::FiguredBass;
use crate::ir::measure::{ClefSign, Measure, MeasureAttributes};
use crate::ir::note::VoiceElement;

use musicxml::datatypes as mdt;
use musicxml::elements as mxml;

impl IrToMxmlAdapter {
    pub(super) fn build_part(&self, part: &crate::ir::Part) -> mxml::Part {
        let id = if part.part_id.is_empty() {
            "P1"
        } else {
            &part.part_id
        };

        let content: Vec<mxml::PartElement> = part
            .measures
            .iter()
            .map(|m| mxml::PartElement::Measure(self.build_measure(m, part.staves)))
            .collect();

        mxml::Part {
            attributes: mxml::PartAttributes {
                id: mdt::IdRef(id.to_string()),
            },
            content,
        }
    }

    fn build_measure(&self, measure: &Measure, part_staves: u8) -> mxml::Measure {
        let attrs = mxml::MeasureAttributes {
            number: mdt::Token(measure.number.to_string()),
            id: None,
            implicit: if measure.implicit {
                Some(mdt::YesNo::Yes)
            } else {
                None
            },
            non_controlling: None,
            text: None,
            width: measure.width.map(|w| mdt::Tenths(w as f64)),
        };

        let content = self.build_measure_elements(measure, part_staves);

        mxml::Measure {
            attributes: attrs,
            content,
        }
    }

    fn build_measure_elements(
        &self,
        measure: &Measure,
        part_staves: u8,
    ) -> Vec<mxml::MeasureElement> {
        let mut elements: Vec<mxml::MeasureElement> = Vec::new();

        // <print> element for layout breaks (emitted before attributes)
        for dir in &measure.directions {
            if let Some(ref lb) = dir.layout_break {
                let mut print_attrs = mxml::PrintAttributes::default();
                match lb {
                    crate::ir::direction::LayoutBreakType::Page => {
                        print_attrs.new_page = Some(mdt::YesNo::Yes);
                    }
                    crate::ir::direction::LayoutBreakType::System
                    | crate::ir::direction::LayoutBreakType::Section => {
                        print_attrs.new_system = Some(mdt::YesNo::Yes);
                    }
                }
                elements.push(mxml::MeasureElement::Print(mxml::Print {
                    attributes: print_attrs,
                    content: mxml::PrintContents::default(),
                }));
            }
        }

        // Attributes
        if let Some(attrs) = &measure.attributes {
            elements.push(mxml::MeasureElement::Attributes(
                self.build_attributes(attrs, measure.multi_measure_rest),
            ));
        }

        // Left barline
        if let Some(bl) = &measure.left_barline {
            elements.push(mxml::MeasureElement::Barline(
                self.build_barline(bl, "left"),
            ));
        }

        // Directions (skip layout-break-only directions; those are emitted as <print>)
        for dir in &measure.directions {
            if dir.layout_break.is_some()
                && dir.dynamic.is_none()
                && dir.wedge.is_none()
                && dir.tempo.is_none()
                && dir.text.is_none()
                && dir.rehearsal.is_none()
                && dir.octave_shift.is_none()
                && dir.pedal.is_none()
                && !dir.coda
                && !dir.segno
                && dir.da_capo.is_none()
                && dir.dal_segno.is_none()
                && dir.instrument_change.is_none()
            {
                continue;
            }
            elements.push(mxml::MeasureElement::Direction(self.build_direction(dir)));
        }

        // Harmony / chord symbols (before notes; offset positions within measure)
        for harmony in &measure.harmonies {
            elements.push(mxml::MeasureElement::Harmony(self.build_harmony(harmony)));
        }

        // Build an index of figured bass keyed by measure-offset (in divisions).
        let mut fb_by_offset: std::collections::BTreeMap<i32, Vec<&FiguredBass>> =
            std::collections::BTreeMap::new();
        for fb in &measure.figured_bass {
            fb_by_offset.entry(fb.offset).or_default().push(fb);
        }
        let mut fb_emitted_up_to: i32 = -1;

        // Voices with backup between them
        let voices = &measure.voices;
        for (vi, voice) in voices.iter().enumerate() {
            if vi > 0 {
                // Backup to start of measure for subsequent voices
                let prev = &voices[vi - 1];
                let total_dur = self.voice_duration(prev);
                if total_dur > 0 {
                    elements.push(mxml::MeasureElement::Backup(mxml::Backup {
                        attributes: (),
                        content: mxml::BackupContents {
                            duration: mxml::Duration {
                                attributes: (),
                                content: mdt::PositiveDivisions(total_dur as u32),
                            },
                            footnote: None,
                            level: None,
                        },
                    }));
                }
            }

            let mut fwd_pos: i64 = 0;
            for elem in &voice.elements {
                // Interleave figured bass into voice 1's note stream.
                if vi == 0 {
                    let cur_divs = fwd_pos as i32;
                    for (&off, fbs) in fb_by_offset.range(fb_emitted_up_to + 1..=cur_divs) {
                        for fb in fbs {
                            elements.push(mxml::MeasureElement::FiguredBass(
                                self.build_figured_bass(fb),
                            ));
                        }
                        fb_emitted_up_to = off;
                    }
                }

                match elem {
                    VoiceElement::Note(n) => {
                        // Note-level directions (dynamics, wedges, text)
                        for dir in self.build_note_direction_elements(n) {
                            elements.push(mxml::MeasureElement::Direction(dir));
                        }
                        let note = self.build_note(n, voice.number, false, None, part_staves);
                        elements.push(mxml::MeasureElement::Note(note));
                        if !n.is_grace {
                            fwd_pos += self.duration_to_divisions(&n.duration);
                        }
                    }
                    VoiceElement::Rest(r) => {
                        if r.is_spacer {
                            // Emit spacer rests as MusicXML <forward>
                            let dur_val = self.duration_to_divisions(&r.duration);
                            let mut fwd_content = mxml::ForwardContents {
                                duration: mxml::Duration {
                                    attributes: (),
                                    content: mdt::PositiveDivisions(dur_val.max(1) as u32),
                                },
                                footnote: None,
                                level: None,
                                voice: Some(mxml::Voice {
                                    attributes: (),
                                    content: voice.number.to_string(),
                                }),
                                staff: None,
                            };
                            if part_staves > 1 {
                                fwd_content.staff = Some(mxml::Staff {
                                    attributes: (),
                                    content: mdt::PositiveInteger(r.staff as u32),
                                });
                            }
                            elements.push(mxml::MeasureElement::Forward(mxml::Forward {
                                attributes: (),
                                content: fwd_content,
                            }));
                            fwd_pos += dur_val;
                        } else {
                            let rest_note = self.build_rest_note(r, voice.number, part_staves);
                            elements.push(mxml::MeasureElement::Note(rest_note));
                            fwd_pos += self.duration_to_divisions(&r.duration);
                        }
                    }
                    VoiceElement::Chord(c) => {
                        // Emit directions from the first note in the chord
                        if let Some(first) = c.notes.first() {
                            for dir in self.build_note_direction_elements(first) {
                                elements.push(mxml::MeasureElement::Direction(dir));
                            }
                        }
                        let chord_notes = self.build_chord_notes(c, voice.number, part_staves);
                        for note in chord_notes {
                            elements.push(mxml::MeasureElement::Note(note));
                        }
                        fwd_pos += self.duration_to_divisions(&c.duration);
                    }
                }
            }

            // Emit any remaining figured bass that falls after the last note (voice 1 only)
            if vi == 0 {
                for (&off, fbs) in fb_by_offset.range(fb_emitted_up_to + 1..) {
                    for fb in fbs {
                        elements.push(mxml::MeasureElement::FiguredBass(
                            self.build_figured_bass(fb),
                        ));
                    }
                    fb_emitted_up_to = off;
                }
            }
        }

        // Fallback: if there are no voices at all, emit figured bass with offsets
        if voices.is_empty() {
            for fbs in fb_by_offset.values() {
                for fb in fbs {
                    elements.push(mxml::MeasureElement::FiguredBass(
                        self.build_figured_bass(fb),
                    ));
                }
            }
        }

        // Right barline
        if let Some(bl) = &measure.right_barline {
            elements.push(mxml::MeasureElement::Barline(
                self.build_barline(bl, "right"),
            ));
        }

        elements
    }

    fn build_attributes(
        &self,
        attrs: &MeasureAttributes,
        multi_measure_rest: Option<u16>,
    ) -> mxml::Attributes {
        let divisions = Some(mxml::Divisions {
            attributes: (),
            content: mdt::PositiveDivisions(self.divisions as u32),
        });

        // Key
        let key: Vec<mxml::Key> = if let Some(k) = &attrs.key {
            let mode_val = match k.mode.as_str() {
                "major" => mdt::Mode::Major,
                "minor" => mdt::Mode::Minor,
                "dorian" => mdt::Mode::Dorian,
                "phrygian" => mdt::Mode::Phrygian,
                "lydian" => mdt::Mode::Lydian,
                "mixolydian" => mdt::Mode::Mixolydian,
                "aeolian" => mdt::Mode::Aeolian,
                "locrian" => mdt::Mode::Locrian,
                _ => mdt::Mode::Major,
            };
            vec![mxml::Key {
                attributes: mxml::KeyAttributes::default(),
                content: mxml::KeyContents::Explicit(mxml::ExplicitKeyContents {
                    cancel: None,
                    fifths: mxml::Fifths {
                        attributes: (),
                        content: mdt::Fifths(k.fifths),
                    },
                    mode: Some(mxml::Mode {
                        attributes: (),
                        content: mode_val,
                    }),
                    key_octave: vec![],
                }),
            }]
        } else {
            vec![]
        };

        // Time
        let time: Vec<mxml::Time> = if let Some(t) = &attrs.time {
            let mut time_attrs = mxml::TimeAttributes::default();
            if let Some(sym) = &t.symbol {
                time_attrs.symbol = match sym.as_str() {
                    "common" => Some(mdt::TimeSymbol::Common),
                    "cut" => Some(mdt::TimeSymbol::Cut),
                    "single-number" => Some(mdt::TimeSymbol::SingleNumber),
                    "normal" => Some(mdt::TimeSymbol::Normal),
                    _ => None,
                };
            }
            // Handle compound beats like "3+2"
            let beats: Vec<mxml::TimeBeatContents> = t
                .beats
                .split('+')
                .map(|beat_part| mxml::TimeBeatContents {
                    beats: mxml::Beats {
                        attributes: (),
                        content: beat_part.trim().to_string(),
                    },
                    beat_type: mxml::BeatType {
                        attributes: (),
                        content: t.beat_type.to_string(),
                    },
                })
                .collect();
            vec![mxml::Time {
                attributes: time_attrs,
                content: mxml::TimeContents {
                    beats,
                    interchangeable: None,
                    senza_misura: None,
                },
            }]
        } else {
            vec![]
        };

        // Staves
        let staves = attrs.staves.map(|s| mxml::Staves {
            attributes: (),
            content: mdt::NonNegativeInteger(s as u32),
        });

        // Clefs (sorted by staff number)
        let mut sorted_clefs: Vec<_> = attrs.clefs.iter().collect();
        sorted_clefs.sort_by_key(|(num, _)| **num);
        let clef: Vec<mxml::Clef> = sorted_clefs
            .iter()
            .map(|(&staff_num, c)| {
                let mut clef_attrs = mxml::ClefAttributes::default();
                if sorted_clefs.len() > 1 {
                    clef_attrs.number = Some(mdt::StaffNumber(staff_num));
                }
                let sign = match c.sign {
                    ClefSign::G => mdt::ClefSign::G,
                    ClefSign::F => mdt::ClefSign::F,
                    ClefSign::C => mdt::ClefSign::C,
                    ClefSign::Percussion => mdt::ClefSign::Percussion,
                    ClefSign::Tab => mdt::ClefSign::TAB,
                };
                let clef_octave_change = if c.octave_change != 0 {
                    Some(mxml::ClefOctaveChange {
                        attributes: (),
                        content: c.octave_change,
                    })
                } else {
                    None
                };
                mxml::Clef {
                    attributes: clef_attrs,
                    content: mxml::ClefContents {
                        sign: mxml::Sign {
                            attributes: (),
                            content: sign,
                        },
                        line: Some(mxml::Line {
                            attributes: (),
                            content: mdt::StaffLinePosition(c.line as i16),
                        }),
                        clef_octave_change,
                    },
                }
            })
            .collect();

        // Transpose
        let transpose: Vec<mxml::Transpose> = if let Some(tr) = &attrs.transpose {
            let octave_change = if tr.octave_change != 0 {
                Some(mxml::OctaveChange {
                    attributes: (),
                    content: tr.octave_change,
                })
            } else {
                None
            };
            vec![mxml::Transpose {
                attributes: mxml::TransposeAttributes::default(),
                content: mxml::TransposeContents {
                    diatonic: Some(mxml::Diatonic {
                        attributes: (),
                        content: tr.diatonic as i16,
                    }),
                    chromatic: mxml::Chromatic {
                        attributes: (),
                        content: mdt::Semitones(tr.chromatic as i16),
                    },
                    octave_change,
                    double: None,
                },
            }]
        } else {
            vec![]
        };

        // Staff details (non-default staff lines)
        let staff_details: Vec<mxml::StaffDetails> = if let Some(lines) = attrs.staff_lines {
            if lines != 5 {
                vec![mxml::StaffDetails {
                    attributes: mxml::StaffDetailsAttributes::default(),
                    content: mxml::StaffDetailsContents {
                        staff_type: None,
                        staff_lines: Some(mxml::StaffLines {
                            attributes: (),
                            content: mdt::NonNegativeInteger(lines as u32),
                        }),
                        line_detail: vec![],
                        staff_tuning: vec![],
                        capo: None,
                        staff_size: None,
                    },
                }]
            } else {
                vec![]
            }
        } else {
            vec![]
        };

        // Measure style (multi-measure rest)
        let measure_style: Vec<mxml::MeasureStyle> = if let Some(count) = multi_measure_rest {
            vec![mxml::MeasureStyle {
                attributes: mxml::MeasureStyleAttributes::default(),
                content: mxml::MeasureStyleContents::MultipleRest(mxml::MultipleRest {
                    attributes: mxml::MultipleRestAttributes::default(),
                    content: mdt::PositiveInteger(count as u32),
                }),
            }]
        } else {
            vec![]
        };

        mxml::Attributes {
            attributes: (),
            content: mxml::AttributesContents {
                footnote: None,
                level: None,
                divisions,
                key,
                time,
                staves,
                part_symbol: None,
                instruments: None,
                clef,
                staff_details,
                transpose,
                for_part: vec![],
                directive: vec![],
                measure_style,
            },
        }
    }

    fn build_barline(
        &self,
        barline: &crate::ir::direction::Barline,
        location: &str,
    ) -> mxml::Barline {
        let bar_location = match location {
            "left" => Some(mdt::RightLeftMiddle::Left),
            "right" => Some(mdt::RightLeftMiddle::Right),
            "middle" => Some(mdt::RightLeftMiddle::Middle),
            _ => None,
        };

        let style = match barline.style {
            BarlineType::Regular => mdt::BarStyle::Regular,
            BarlineType::Double => mdt::BarStyle::LightLight,
            BarlineType::Final => mdt::BarStyle::LightHeavy,
            BarlineType::RepeatForward => mdt::BarStyle::HeavyLight,
            BarlineType::RepeatBackward => mdt::BarStyle::LightHeavy,
            BarlineType::RepeatBoth => mdt::BarStyle::LightHeavy,
            BarlineType::Dashed => mdt::BarStyle::Dashed,
            BarlineType::Dotted => mdt::BarStyle::Dotted,
            BarlineType::Tick => mdt::BarStyle::Tick,
            BarlineType::Short => mdt::BarStyle::Short,
            BarlineType::None => mdt::BarStyle::None,
        };

        let repeat = barline.repeat_direction.as_ref().map(|rd| {
            let dir = match rd {
                crate::ir::direction::RepeatDirection::Forward => mdt::BackwardForward::Forward,
                crate::ir::direction::RepeatDirection::Backward => mdt::BackwardForward::Backward,
            };
            mxml::Repeat {
                attributes: mxml::RepeatAttributes {
                    direction: dir,
                    after_jump: None,
                    times: None,
                    winged: None,
                },
                content: (),
            }
        });

        let ending =
            if let (Some(num), Some(etype)) = (&barline.ending_number, &barline.ending_type) {
                let ending_type = match etype.as_str() {
                    "start" => mdt::StartStopDiscontinue::Start,
                    "stop" => mdt::StartStopDiscontinue::Stop,
                    "discontinue" => mdt::StartStopDiscontinue::Discontinue,
                    _ => mdt::StartStopDiscontinue::Start,
                };
                Some(mxml::Ending {
                    attributes: mxml::EndingAttributes {
                        number: mdt::EndingNumber(num.to_string()),
                        r#type: ending_type,
                        color: None,
                        default_x: None,
                        default_y: None,
                        end_length: None,
                        font_family: None,
                        font_size: None,
                        font_style: None,
                        font_weight: None,
                        print_object: None,
                        relative_x: None,
                        relative_y: None,
                        system: None,
                        text_x: None,
                        text_y: None,
                    },
                    content: String::new(),
                })
            } else {
                None
            };

        mxml::Barline {
            attributes: mxml::BarlineAttributes {
                location: bar_location,
                ..Default::default()
            },
            content: mxml::BarlineContents {
                bar_style: Some(mxml::BarStyle {
                    attributes: mxml::BarStyleAttributes::default(),
                    content: style,
                }),
                footnote: None,
                level: None,
                wavy_line: None,
                segno: None,
                coda: None,
                fermata: vec![],
                ending,
                repeat,
            },
        }
    }

    /// Convert an IR Duration to MusicXML duration value.
    ///
    /// Formula: actual_duration * 4 * divisions
    pub(super) fn duration_to_divisions(&self, duration: &crate::ir::duration::Duration) -> i64 {
        let actual = duration.actual_duration();
        let quarter_notes = actual * crate::ir::duration::Frac::from_integer(4);
        let result = quarter_notes * crate::ir::duration::Frac::from_integer(self.divisions as i64);
        // Round to nearest integer — should be exact for valid durations
        *result.numer() / *result.denom()
    }

    /// Calculate the total duration of a voice in divisions.
    pub(super) fn voice_duration(&self, voice: &crate::ir::voice::Voice) -> i64 {
        let mut total = crate::ir::duration::Frac::from_integer(0);
        for elem in &voice.elements {
            match elem {
                VoiceElement::Note(n) => total += n.duration.actual_duration(),
                VoiceElement::Rest(r) => total += r.duration.actual_duration(),
                VoiceElement::Chord(c) => total += c.duration.actual_duration(),
            }
        }
        let result = total * crate::ir::duration::Frac::from_integer(4 * self.divisions as i64);
        *result.numer() / *result.denom()
    }
}
