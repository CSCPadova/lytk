//! Score-level MusicXML emission.

use quick_xml::events::{BytesEnd, BytesStart, BytesText, Event};

use super::helpers::{format_float, text_element};
use super::{IrToMxmlAdapter, W};
use crate::adapters::Result;
use crate::ir::score::{PageLayout, Score, ScoreChild};

impl IrToMxmlAdapter {
    /// Write the top-level `<score-partwise>` element.
    pub(super) fn write_score(&self, w: &mut W, score: &Score) -> Result<()> {
        let mut root = BytesStart::new("score-partwise");
        root.push_attribute(("version", self.version.as_str()));
        w.write_event(Event::Start(root))?;

        // Movement title
        if let Some(title) = &score.metadata.title {
            text_element(w, "movement-title", title)?;
        }

        // Identification
        self.write_identification(w, score)?;

        // Defaults (page layout / scaling)
        if let Some(pl) = &score.page_layout {
            self.write_defaults(w, pl)?;
        }

        // Credit elements (subtitle)
        if let Some(subtitle) = &score.metadata.subtitle {
            w.write_event(Event::Start(BytesStart::new("credit")))?;
            text_element(w, "credit-type", "subtitle")?;
            w.write_event(Event::Start(BytesStart::new("credit-words")))?;
            w.write_event(Event::Text(BytesText::new(subtitle)))?;
            w.write_event(Event::End(BytesEnd::new("credit-words")))?;
            w.write_event(Event::End(BytesEnd::new("credit")))?;
        }

        // Part list
        self.write_part_list(w, score)?;

        // Parts
        for part in score.parts() {
            self.write_part(w, part)?;
        }

        w.write_event(Event::End(BytesEnd::new("score-partwise")))?;
        Ok(())
    }

    fn write_identification(&self, w: &mut W, score: &Score) -> Result<()> {
        let meta = &score.metadata;
        w.write_event(Event::Start(BytesStart::new("identification")))?;

        if let Some(composer) = &meta.composer {
            let mut el = BytesStart::new("creator");
            el.push_attribute(("type", "composer"));
            w.write_event(Event::Start(el))?;
            w.write_event(Event::Text(BytesText::new(composer)))?;
            w.write_event(Event::End(BytesEnd::new("creator")))?;
        }
        if let Some(arranger) = &meta.arranger {
            let mut el = BytesStart::new("creator");
            el.push_attribute(("type", "arranger"));
            w.write_event(Event::Start(el))?;
            w.write_event(Event::Text(BytesText::new(arranger)))?;
            w.write_event(Event::End(BytesEnd::new("creator")))?;
        }
        if let Some(lyricist) = &meta.lyricist {
            let mut el = BytesStart::new("creator");
            el.push_attribute(("type", "lyricist"));
            w.write_event(Event::Start(el))?;
            w.write_event(Event::Text(BytesText::new(lyricist)))?;
            w.write_event(Event::End(BytesEnd::new("creator")))?;
        }

        // Extra creators (sorted for deterministic output)
        let mut extra_keys: Vec<&String> = meta.extra.keys().collect();
        extra_keys.sort();
        for key in extra_keys {
            if let Some(value) = meta.extra.get(key) {
                let mut el = BytesStart::new("creator");
                el.push_attribute(("type", key.as_str()));
                w.write_event(Event::Start(el))?;
                w.write_event(Event::Text(BytesText::new(value)))?;
                w.write_event(Event::End(BytesEnd::new("creator")))?;
            }
        }

        for (rtype, rtext) in &meta.rights {
            let mut el = BytesStart::new("rights");
            if !rtype.is_empty() {
                el.push_attribute(("type", rtype.as_str()));
            }
            w.write_event(Event::Start(el))?;
            w.write_event(Event::Text(BytesText::new(rtext)))?;
            w.write_event(Event::End(BytesEnd::new("rights")))?;
        }

        // Encoding
        w.write_event(Event::Start(BytesStart::new("encoding")))?;
        text_element(w, "software", "lytk")?;
        w.write_event(Event::End(BytesEnd::new("encoding")))?;

        w.write_event(Event::End(BytesEnd::new("identification")))?;
        Ok(())
    }

    pub(super) fn write_part_list(&self, w: &mut W, score: &Score) -> Result<()> {
        w.write_event(Event::Start(BytesStart::new("part-list")))?;

        let mut auto_group_number: u8 = 0;
        for child in &score.children {
            match child {
                ScoreChild::Part(part) => {
                    self.write_score_part(w, part)?;
                }
                ScoreChild::PartGroup(group) => {
                    let num = if group.number > 0 {
                        group.number
                    } else {
                        auto_group_number += 1;
                        auto_group_number
                    };

                    // part-group start
                    let mut pg = BytesStart::new("part-group");
                    pg.push_attribute(("type", "start"));
                    pg.push_attribute(("number", num.to_string().as_str()));
                    w.write_event(Event::Start(pg))?;
                    if !group.name.is_empty() {
                        text_element(w, "group-name", &group.name)?;
                    }
                    let symbol = match group.bracket.as_str() {
                        "brace" => "brace",
                        "line" => "line",
                        "square" => "square",
                        _ => "bracket",
                    };
                    text_element(w, "group-symbol", symbol)?;
                    w.write_event(Event::End(BytesEnd::new("part-group")))?;

                    // nested score-parts
                    for sc in &group.children {
                        if let ScoreChild::Part(p) = sc {
                            self.write_score_part(w, p)?;
                        }
                    }

                    // part-group stop
                    let mut pg_stop = BytesStart::new("part-group");
                    pg_stop.push_attribute(("type", "stop"));
                    pg_stop.push_attribute(("number", num.to_string().as_str()));
                    w.write_event(Event::Empty(pg_stop))?;
                }
            }
        }

        w.write_event(Event::End(BytesEnd::new("part-list")))?;
        Ok(())
    }

    fn write_score_part(&self, w: &mut W, part: &crate::ir::Part) -> Result<()> {
        let mut sp = BytesStart::new("score-part");
        let id = if part.part_id.is_empty() {
            "P1"
        } else {
            &part.part_id
        };
        sp.push_attribute(("id", id));
        w.write_event(Event::Start(sp))?;
        text_element(w, "part-name", &part.name)?;
        if !part.abbreviation.is_empty() {
            text_element(w, "part-abbreviation", &part.abbreviation)?;
        }

        // Score-instrument + MIDI instrument
        let has_midi = part.midi_channel > 0
            || part.midi_program > 0
            || !part.midi_instrument.is_empty();
        if has_midi {
            let inst_id = format!("{}-I1", id);

            let mut si = BytesStart::new("score-instrument");
            si.push_attribute(("id", inst_id.as_str()));
            w.write_event(Event::Start(si))?;
            text_element(w, "instrument-name", if part.name.is_empty() { "Instrument" } else { &part.name })?;
            w.write_event(Event::End(BytesEnd::new("score-instrument")))?;

            let mut mi = BytesStart::new("midi-instrument");
            mi.push_attribute(("id", inst_id.as_str()));
            w.write_event(Event::Start(mi))?;
            if part.midi_channel > 0 {
                text_element(w, "midi-channel", &part.midi_channel.to_string())?;
            }
            if !part.midi_instrument.is_empty() {
                text_element(w, "midi-name", &part.midi_instrument)?;
            }
            if part.midi_program > 0 {
                text_element(w, "midi-program", &part.midi_program.to_string())?;
            }
            w.write_event(Event::End(BytesEnd::new("midi-instrument")))?;
        }

        w.write_event(Event::End(BytesEnd::new("score-part")))?;
        Ok(())
    }

    /// Write `<defaults>` element with page layout and scaling.
    pub(super) fn write_defaults(&self, w: &mut W, pl: &PageLayout) -> Result<()> {
        w.write_event(Event::Start(BytesStart::new("defaults")))?;

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

        if pl.staff_size.is_some() || has_dimensions {
            w.write_event(Event::Start(BytesStart::new("scaling")))?;
            text_element(w, "millimeters", &format_float(mm_per_tenth * 40.0))?;
            text_element(w, "tenths", "40")?;
            w.write_event(Event::End(BytesEnd::new("scaling")))?;
        }

        let cm_to_tenths = |cm: f64| cm * 10.0 / mm_per_tenth;

        // page-layout
        let has_page = pl.page_height.is_some()
            || pl.page_width.is_some()
            || pl.left_margin.is_some()
            || pl.right_margin.is_some()
            || pl.top_margin.is_some()
            || pl.bottom_margin.is_some();
        if has_page {
            w.write_event(Event::Start(BytesStart::new("page-layout")))?;
            if let Some(h) = pl.page_height {
                text_element(w, "page-height", &format_float(cm_to_tenths(h)))?;
            }
            if let Some(wd) = pl.page_width {
                text_element(w, "page-width", &format_float(cm_to_tenths(wd)))?;
            }
            let has_margins = pl.left_margin.is_some()
                || pl.right_margin.is_some()
                || pl.top_margin.is_some()
                || pl.bottom_margin.is_some();
            if has_margins {
                let mut pm = BytesStart::new("page-margins");
                pm.push_attribute(("type", "both"));
                w.write_event(Event::Start(pm))?;
                if let Some(v) = pl.left_margin {
                    text_element(w, "left-margin", &format_float(cm_to_tenths(v)))?;
                }
                if let Some(v) = pl.right_margin {
                    text_element(w, "right-margin", &format_float(cm_to_tenths(v)))?;
                }
                if let Some(v) = pl.top_margin {
                    text_element(w, "top-margin", &format_float(cm_to_tenths(v)))?;
                }
                if let Some(v) = pl.bottom_margin {
                    text_element(w, "bottom-margin", &format_float(cm_to_tenths(v)))?;
                }
                w.write_event(Event::End(BytesEnd::new("page-margins")))?;
            }
            w.write_event(Event::End(BytesEnd::new("page-layout")))?;
        }

        // system-layout
        let has_system =
            pl.system_distance.is_some() || pl.top_system_distance.is_some();
        if has_system {
            w.write_event(Event::Start(BytesStart::new("system-layout")))?;
            w.write_event(Event::Start(BytesStart::new("system-margins")))?;
            text_element(w, "left-margin", "0")?;
            text_element(w, "right-margin", "0")?;
            w.write_event(Event::End(BytesEnd::new("system-margins")))?;
            if let Some(sd) = pl.system_distance {
                text_element(w, "system-distance", &format_float(cm_to_tenths(sd)))?;
            }
            if let Some(tsd) = pl.top_system_distance {
                text_element(
                    w,
                    "top-system-distance",
                    &format_float(cm_to_tenths(tsd)),
                )?;
            }
            w.write_event(Event::End(BytesEnd::new("system-layout")))?;
        }

        w.write_event(Event::End(BytesEnd::new("defaults")))?;
        Ok(())
    }
}
