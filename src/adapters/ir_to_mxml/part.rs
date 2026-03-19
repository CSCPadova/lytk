//! Part-level and measure-level MusicXML emission.

use quick_xml::events::{BytesEnd, BytesStart, BytesText, Event};

use super::helpers::{format_float, text_element};
use super::{IrToMxmlAdapter, W};
use crate::adapters::Result;
use crate::ir::direction::BarlineType;
use crate::ir::harmony::FiguredBass;
use crate::ir::measure::{ClefSign, Measure, MeasureAttributes};
use crate::ir::note::VoiceElement;

impl IrToMxmlAdapter {
    pub(super) fn write_part(&self, w: &mut W, part: &crate::ir::Part) -> Result<()> {
        let mut el = BytesStart::new("part");
        let id = if part.part_id.is_empty() {
            "P1"
        } else {
            &part.part_id
        };
        el.push_attribute(("id", id));
        w.write_event(Event::Start(el))?;

        for measure in &part.measures {
            self.write_measure(w, measure, part.staves)?;
        }

        w.write_event(Event::End(BytesEnd::new("part")))?;
        Ok(())
    }

    pub(super) fn write_measure(
        &self,
        w: &mut W,
        measure: &Measure,
        part_staves: u8,
    ) -> Result<()> {
        let mut el = BytesStart::new("measure");
        el.push_attribute(("number", measure.number.to_string().as_str()));
        if measure.implicit {
            el.push_attribute(("implicit", "yes"));
        }
        if let Some(w_val) = measure.width {
            let w_str = format_float(w_val as f64);
            el.push_attribute(("width", w_str.as_str()));
        }
        w.write_event(Event::Start(el))?;

        // <print> element for layout breaks (emitted before attributes)
        let has_layout_break = measure.directions.iter().any(|d| d.layout_break.is_some());
        if has_layout_break {
            for dir in &measure.directions {
                if let Some(ref lb) = dir.layout_break {
                    let mut print_el = BytesStart::new("print");
                    match lb {
                        crate::ir::direction::LayoutBreakType::Page => {
                            print_el.push_attribute(("new-page", "yes"));
                        }
                        crate::ir::direction::LayoutBreakType::System => {
                            print_el.push_attribute(("new-system", "yes"));
                        }
                        crate::ir::direction::LayoutBreakType::Section => {
                            print_el.push_attribute(("new-system", "yes"));
                        }
                    }
                    w.write_event(Event::Empty(print_el))?;
                }
            }
        }

        // Attributes
        if let Some(attrs) = &measure.attributes {
            self.write_attributes(w, attrs, measure.multi_measure_rest)?;
        }

        // Left barline
        if let Some(bl) = &measure.left_barline {
            self.write_barline(w, bl, "left")?;
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
            self.write_direction(w, dir)?;
        }

        // Harmony / chord symbols (before notes; offset positions within measure)
        for harmony in &measure.harmonies {
            self.write_harmony(w, harmony)?;
        }

        // Build an index of figured bass keyed by measure-offset (in divisions).
        // We'll interleave them into the first voice's note stream.
        // Sort by offset so they land in temporal order.
        let mut fb_by_offset: std::collections::BTreeMap<i32, Vec<&FiguredBass>> =
            std::collections::BTreeMap::new();
        for fb in &measure.figured_bass {
            fb_by_offset.entry(fb.offset).or_default().push(fb);
        }
        // Offset of the last emitted figure group — to avoid double-emitting
        let mut fb_emitted_up_to: i32 = -1;

        // Voices with backup between them
        let voices = &measure.voices;
        for (vi, voice) in voices.iter().enumerate() {
            if vi > 0 {
                // Backup to start of measure for subsequent voices
                let prev = &voices[vi - 1];
                let total_dur = self.voice_duration(prev);
                if total_dur > 0 {
                    w.write_event(Event::Start(BytesStart::new("backup")))?;
                    text_element(w, "duration", &total_dur.to_string())?;
                    w.write_event(Event::End(BytesEnd::new("backup")))?;
                }
            }

            let mut fwd_pos: i64 = 0;
            for elem in &voice.elements {
                // Interleave figured bass into voice 1's note stream.
                // Emit all figures whose offset falls at the current note position.
                if vi == 0 {
                    let cur_divs = fwd_pos as i32;
                    for (&off, fbs) in fb_by_offset.range(fb_emitted_up_to + 1..=cur_divs) {
                        for fb in fbs {
                            self.write_figured_bass(w, fb)?;
                        }
                        fb_emitted_up_to = off;
                    }
                }

                match elem {
                    VoiceElement::Note(n) => {
                        self.emit_note_directions(w, n)?;
                        self.write_note(w, n, voice.number, false, None, part_staves)?;
                        if !n.is_grace {
                            fwd_pos += self.duration_to_divisions(&n.duration);
                        }
                    }
                    VoiceElement::Rest(r) => {
                        self.write_rest(w, r, voice.number, part_staves)?;
                        fwd_pos += self.duration_to_divisions(&r.duration);
                    }
                    VoiceElement::Chord(c) => {
                        // Emit directions from the first note in the chord
                        if let Some(first) = c.notes.first() {
                            self.emit_note_directions(w, first)?;
                        }
                        self.write_chord(w, c, voice.number, part_staves)?;
                        fwd_pos += self.duration_to_divisions(&c.duration);
                    }
                    VoiceElement::Forward(fwd) => {
                        w.write_event(Event::Start(BytesStart::new("forward")))?;
                        let dur_val = self.duration_to_divisions(&fwd.duration);
                        text_element(w, "duration", &dur_val.to_string())?;
                        text_element(w, "voice", &fwd.voice.to_string())?;
                        if part_staves > 1 {
                            text_element(w, "staff", &fwd.staff.to_string())?;
                        }
                        w.write_event(Event::End(BytesEnd::new("forward")))?;
                        fwd_pos += dur_val;
                    }
                    VoiceElement::Backup(bk) => {
                        w.write_event(Event::Start(BytesStart::new("backup")))?;
                        let dur_val = self.duration_to_divisions(&bk.duration);
                        text_element(w, "duration", &dur_val.to_string())?;
                        w.write_event(Event::End(BytesEnd::new("backup")))?;
                        fwd_pos -= dur_val;
                    }
                }
            }

            // Emit any remaining figured bass that falls after the last note (voice 1 only)
            if vi == 0 {
                for (&off, fbs) in fb_by_offset.range(fb_emitted_up_to + 1..) {
                    for fb in fbs {
                        self.write_figured_bass(w, fb)?;
                    }
                    fb_emitted_up_to = off;
                }
            }
        }

        // Fallback: if there are no voices at all, emit figured bass with offsets
        if voices.is_empty() {
            for fbs in fb_by_offset.values() {
                for fb in fbs {
                    self.write_figured_bass(w, fb)?;
                }
            }
        }

        // Right barline
        if let Some(bl) = &measure.right_barline {
            self.write_barline(w, bl, "right")?;
        }

        w.write_event(Event::End(BytesEnd::new("measure")))?;
        Ok(())
    }

    pub(super) fn write_attributes(
        &self,
        w: &mut W,
        attrs: &MeasureAttributes,
        multi_measure_rest: Option<u16>,
    ) -> Result<()> {
        w.write_event(Event::Start(BytesStart::new("attributes")))?;
        text_element(w, "divisions", &self.divisions.to_string())?;

        if let Some(key) = &attrs.key {
            w.write_event(Event::Start(BytesStart::new("key")))?;
            text_element(w, "fifths", &key.fifths.to_string())?;
            text_element(w, "mode", key.mode.as_str())?;
            w.write_event(Event::End(BytesEnd::new("key")))?;
        }

        if let Some(time) = &attrs.time {
            let mut time_el = BytesStart::new("time");
            if let Some(sym) = &time.symbol {
                time_el.push_attribute(("symbol", sym.as_str()));
            }
            w.write_event(Event::Start(time_el))?;
            // Handle compound beats like "3+2"
            for beat_part in time.beats.split('+') {
                text_element(w, "beats", beat_part.trim())?;
            }
            text_element(w, "beat-type", &time.beat_type.to_string())?;
            w.write_event(Event::End(BytesEnd::new("time")))?;
        }

        if let Some(staves) = attrs.staves {
            text_element(w, "staves", &staves.to_string())?;
        }

        let mut sorted_clefs: Vec<_> = attrs.clefs.iter().collect();
        sorted_clefs.sort_by_key(|(num, _)| **num);
        for (&staff_num, clef) in &sorted_clefs {
            let mut clef_el = BytesStart::new("clef");
            if sorted_clefs.len() > 1 {
                clef_el.push_attribute(("number", staff_num.to_string().as_str()));
            }
            w.write_event(Event::Start(clef_el))?;
            let sign = match clef.sign {
                ClefSign::G => "G",
                ClefSign::F => "F",
                ClefSign::C => "C",
                ClefSign::Percussion => "percussion",
                ClefSign::Tab => "TAB",
            };
            text_element(w, "sign", sign)?;
            text_element(w, "line", &clef.line.to_string())?;
            if clef.octave_change != 0 {
                text_element(w, "clef-octave-change", &clef.octave_change.to_string())?;
            }
            w.write_event(Event::End(BytesEnd::new("clef")))?;
        }

        if let Some(tr) = &attrs.transpose {
            w.write_event(Event::Start(BytesStart::new("transpose")))?;
            text_element(w, "diatonic", &tr.diatonic.to_string())?;
            text_element(w, "chromatic", &tr.chromatic.to_string())?;
            if tr.octave_change != 0 {
                text_element(w, "octave-change", &tr.octave_change.to_string())?;
            }
            w.write_event(Event::End(BytesEnd::new("transpose")))?;
        }

        // Staff details (non-default staff lines)
        if let Some(lines) = attrs.staff_lines {
            if lines != 5 {
                w.write_event(Event::Start(BytesStart::new("staff-details")))?;
                text_element(w, "staff-lines", &lines.to_string())?;
                w.write_event(Event::End(BytesEnd::new("staff-details")))?;
            }
        }

        // Measure style (multi-measure rest)
        if let Some(count) = multi_measure_rest {
            w.write_event(Event::Start(BytesStart::new("measure-style")))?;
            w.write_event(Event::Start(BytesStart::new("multiple-rest")))?;
            w.write_event(Event::Text(BytesText::new(&count.to_string())))?;
            w.write_event(Event::End(BytesEnd::new("multiple-rest")))?;
            w.write_event(Event::End(BytesEnd::new("measure-style")))?;
        }

        w.write_event(Event::End(BytesEnd::new("attributes")))?;
        Ok(())
    }

    pub(super) fn write_barline(
        &self,
        w: &mut W,
        barline: &crate::ir::direction::Barline,
        location: &str,
    ) -> Result<()> {
        let mut el = BytesStart::new("barline");
        el.push_attribute(("location", location));
        w.write_event(Event::Start(el))?;

        let style = match barline.style {
            BarlineType::Regular => "regular",
            BarlineType::Double => "light-light",
            BarlineType::Final => "light-heavy",
            BarlineType::RepeatForward => "heavy-light",
            BarlineType::RepeatBackward => "light-heavy",
            BarlineType::RepeatBoth => "light-heavy",
            BarlineType::Dashed => "dashed",
            BarlineType::Dotted => "dotted",
            BarlineType::Tick => "tick",
            BarlineType::Short => "short",
            BarlineType::None => "none",
        };
        text_element(w, "bar-style", style)?;

        if let Some(rd) = &barline.repeat_direction {
            let dir_str = match rd {
                crate::ir::direction::RepeatDirection::Forward => "forward",
                crate::ir::direction::RepeatDirection::Backward => "backward",
            };
            let mut rep = BytesStart::new("repeat");
            rep.push_attribute(("direction", dir_str));
            w.write_event(Event::Empty(rep))?;
        }

        if let (Some(num), Some(etype)) = (&barline.ending_number, &barline.ending_type) {
            let mut ending = BytesStart::new("ending");
            ending.push_attribute(("number", num.to_string().as_str()));
            ending.push_attribute(("type", etype.as_str()));
            w.write_event(Event::Empty(ending))?;
        }

        w.write_event(Event::End(BytesEnd::new("barline")))?;
        Ok(())
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
                VoiceElement::Forward(f) => total += f.duration.actual_duration(),
                VoiceElement::Backup(_) => {} // backups don't advance time
            }
        }
        let result = total * crate::ir::duration::Frac::from_integer(4 * self.divisions as i64);
        *result.numer() / *result.denom()
    }
}
