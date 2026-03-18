//! IR → MusicXML emitter.
//!
//! Converts an IR [`Score`] into MusicXML 4.0 `<score-partwise>` XML.
//!
//! # Reference
//! Ported from the Python prototype `lytk-py/converters/ir_to_mxml.py`.

use std::io::Cursor;

use quick_xml::events::{BytesDecl, BytesEnd, BytesStart, BytesText, Event};
use quick_xml::Writer;

use crate::ir::articulation::{Placement, StartStop};
use crate::ir::direction::{BarlineType, Direction};
use crate::ir::harmony::{FiguredBass, Harmony};
use crate::ir::measure::{ClefSign, Measure, MeasureAttributes};
use crate::ir::note::{ArpeggioType, Chord, Note, Rest, VoiceElement};
use crate::ir::pitch::AccidentalDisplay;
use crate::ir::score::{PageLayout, Score, ScoreChild};
use crate::ir::voice::Voice;

use super::{FromIrAdapter, Result};

/// Default MusicXML divisions per quarter note.
const DEFAULT_DIVISIONS: u16 = 4;

// ---------------------------------------------------------------------------
// Adapter
// ---------------------------------------------------------------------------

/// Converts an IR [`Score`] to MusicXML.
pub struct IrToMxmlAdapter {
    /// MusicXML version string.
    version: String,
    /// Divisions per quarter note.
    divisions: u16,
}

impl IrToMxmlAdapter {
    pub fn new() -> Self {
        Self {
            version: "4.0".to_string(),
            divisions: DEFAULT_DIVISIONS,
        }
    }

    pub fn with_divisions(mut self, divisions: u16) -> Self {
        self.divisions = divisions;
        self
    }
}

impl Default for IrToMxmlAdapter {
    fn default() -> Self {
        Self::new()
    }
}

impl FromIrAdapter for IrToMxmlAdapter {
    fn convert(&self, score: &Score) -> Result<String> {
        // Auto-compute divisions that accommodate all tuplet ratios in the score
        let effective_divisions = compute_score_divisions(score, self.divisions);
        let adapter = Self {
            version: self.version.clone(),
            divisions: effective_divisions,
        };

        let buf = Cursor::new(Vec::new());
        let mut w = Writer::new_with_indent(buf, b' ', 2);

        // XML declaration
        w.write_event(Event::Decl(BytesDecl::new("1.0", Some("UTF-8"), None)))?;

        adapter.write_score(&mut w, score)?;

        let bytes = w.into_inner().into_inner();
        Ok(String::from_utf8(bytes).expect("XML output is valid UTF-8"))
    }
}

impl super::FromMusicAdapter for IrToMxmlAdapter {
    fn convert_music(
        &self,
        doc: &crate::ir::music::MusicDocument,
    ) -> Result<String> {
        let score = crate::ir::lower::lower_to_score(doc);
        self.convert(&score)
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

type W = Writer<Cursor<Vec<u8>>>;

impl IrToMxmlAdapter {
    // ── score ─────────────────────────────────────────────────────────────

    fn write_score(&self, w: &mut W, score: &Score) -> Result<()> {
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

    fn write_part_list(&self, w: &mut W, score: &Score) -> Result<()> {
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

    // ── part / measure ────────────────────────────────────────────────────

    fn write_part(&self, w: &mut W, part: &crate::ir::Part) -> Result<()> {
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

    fn write_measure(&self, w: &mut W, measure: &Measure, part_staves: u8) -> Result<()> {
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

    // ── attributes ────────────────────────────────────────────────────────

    fn write_attributes(&self, w: &mut W, attrs: &MeasureAttributes, multi_measure_rest: Option<u16>) -> Result<()> {
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

    // ── barline ───────────────────────────────────────────────────────────

    fn write_barline(
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

    // ── note ──────────────────────────────────────────────────────────────

    fn write_note(
        &self,
        w: &mut W,
        note: &Note,
        voice_num: u8,
        is_chord: bool,
        chord_arpeggio: Option<ArpeggioType>,
        part_staves: u8,
    ) -> Result<()> {
        if note.print_object {
            w.write_event(Event::Start(BytesStart::new("note")))?;
        } else {
            let mut note_el = BytesStart::new("note");
            note_el.push_attribute(("print-object", "no"));
            w.write_event(Event::Start(note_el))?;
        }

        if is_chord {
            w.write_event(Event::Empty(BytesStart::new("chord")))?;
        }
        if note.is_grace {
            let mut el = BytesStart::new("grace");
            if note.after_grace {
                el.push_attribute(("steal-time-previous", "100"));
            }
            if note.grace_slash {
                el.push_attribute(("slash", "yes"));
            }
            w.write_event(Event::Empty(el))?;
        }
        if note.is_cue {
            w.write_event(Event::Empty(BytesStart::new("cue")))?;
        }

        // Pitch
        w.write_event(Event::Start(BytesStart::new("pitch")))?;
        text_element(w, "step", note.pitch.step.name())?;
        let alter_val = *note.pitch.alter.numer() as f64 / *note.pitch.alter.denom() as f64;
        if alter_val != 0.0 {
            text_element(w, "alter", &format_float(alter_val))?;
        }
        text_element(w, "octave", &note.pitch.octave.to_string())?;
        w.write_event(Event::End(BytesEnd::new("pitch")))?;

        // Duration (not for grace notes)
        if !note.is_grace {
            let dur_val = self.duration_to_divisions(&note.duration);
            text_element(w, "duration", &dur_val.to_string())?;
        }

        // Tie (sound level)
        for tie in &note.ties {
            let mut el = BytesStart::new("tie");
            let tie_str = match tie.tie_type {
                StartStop::Start => "start",
                StartStop::Stop => "stop",
                StartStop::Continue => "continue",
            };
            el.push_attribute(("type", tie_str));
            w.write_event(Event::Empty(el))?;
        }

        // Voice
        text_element(w, "voice", &voice_num.to_string())?;

        // Type
        if let Some(mxml_type) = note.duration.musicxml_type() {
            text_element(w, "type", mxml_type)?;
        }

        // Dots
        for _ in 0..note.duration.dots {
            w.write_event(Event::Empty(BytesStart::new("dot")))?;
        }

        // Notehead
        if !note.notehead.is_empty() && note.notehead != "normal" {
            text_element(w, "notehead", &note.notehead)?;
        }

        // Accidental display
        if note.pitch.accidental != AccidentalDisplay::None {
            let acc_text = alter_to_accidental_name(note.pitch.alter);
            let mut acc_el = BytesStart::new("accidental");
            match note.pitch.accidental {
                AccidentalDisplay::Cautionary => {
                    acc_el.push_attribute(("cautionary", "yes"));
                }
                AccidentalDisplay::Editorial => {
                    acc_el.push_attribute(("editorial", "yes"));
                }
                _ => {}
            }
            w.write_event(Event::Start(acc_el))?;
            w.write_event(Event::Text(BytesText::new(acc_text)))?;
            w.write_event(Event::End(BytesEnd::new("accidental")))?;
        }

        // Time modification (tuplets)
        if note.duration.tuplet_actual != 1 || note.duration.tuplet_normal != 1 {
            w.write_event(Event::Start(BytesStart::new("time-modification")))?;
            text_element(
                w,
                "actual-notes",
                &note.duration.tuplet_actual.to_string(),
            )?;
            text_element(
                w,
                "normal-notes",
                &note.duration.tuplet_normal.to_string(),
            )?;
            w.write_event(Event::End(BytesEnd::new("time-modification")))?;
        }

        // Stem
        if !note.stem_direction.is_empty() {
            text_element(w, "stem", &note.stem_direction)?;
        }

        // Staff — always emit for multi-staff parts so staff assignment is unambiguous
        if part_staves > 1 || note.staff > 1 {
            text_element(w, "staff", &note.staff.to_string())?;
        }

        // Beams
        for beam in &note.beams {
            let mut el = BytesStart::new("beam");
            el.push_attribute(("number", beam.number.to_string().as_str()));
            w.write_event(Event::Start(el))?;
            w.write_event(Event::Text(BytesText::new(&beam.beam_type)))?;
            w.write_event(Event::End(BytesEnd::new("beam")))?;
        }

        // Notations
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

        if has_notations {
            w.write_event(Event::Start(BytesStart::new("notations")))?;

            // Tied (notation level)
            for tie in &note.ties {
                let mut el = BytesStart::new("tied");
                let tie_str = match tie.tie_type {
                    StartStop::Start => "start",
                    StartStop::Stop => "stop",
                    StartStop::Continue => "continue",
                };
                el.push_attribute(("type", tie_str));
                w.write_event(Event::Empty(el))?;
            }

            for slur in &note.slurs {
                let mut el = BytesStart::new("slur");
                let slur_str = match slur.slur_type {
                    StartStop::Start => "start",
                    StartStop::Stop => "stop",
                    StartStop::Continue => "continue",
                };
                el.push_attribute(("type", slur_str));
                el.push_attribute(("number", slur.number.to_string().as_str()));
                w.write_event(Event::Empty(el))?;
            }

            if let Some(tuplet) = &note.tuplet {
                let mut el = BytesStart::new("tuplet");
                let t_str = match tuplet.tuplet_type {
                    StartStop::Start => "start",
                    StartStop::Stop => "stop",
                    StartStop::Continue => "continue",
                };
                el.push_attribute(("type", t_str));
                w.write_event(Event::Empty(el))?;
            }

            if let Some(fermata) = &note.fermata {
                let mut el = BytesStart::new("fermata");
                if fermata.inverted {
                    el.push_attribute(("type", "inverted"));
                }
                w.write_event(Event::Start(el))?;
                w.write_event(Event::Text(BytesText::new(&fermata.shape)))?;
                w.write_event(Event::End(BytesEnd::new("fermata")))?;
            }

            if !note.articulations.is_empty() {
                w.write_event(Event::Start(BytesStart::new("articulations")))?;
                for art in &note.articulations {
                    w.write_event(Event::Empty(BytesStart::new(&art.name)))?;
                }
                w.write_event(Event::End(BytesEnd::new("articulations")))?;
            }

            if !note.ornaments.is_empty() {
                w.write_event(Event::Start(BytesStart::new("ornaments")))?;
                for orn in &note.ornaments {
                    if orn.name == "tremolo" {
                        // Tremolo needs text content (marks count) and type attribute
                        let mut el = BytesStart::new("tremolo");
                        if note.two_note_tremolo {
                            el.push_attribute(("type", if note.tremolo_start { "start" } else { "stop" }));
                        } else {
                            el.push_attribute(("type", "single"));
                        }
                        w.write_event(Event::Start(el))?;
                        w.write_event(Event::Text(BytesText::new(&note.tremolo_marks.to_string())))?;
                        w.write_event(Event::End(BytesEnd::new("tremolo")))?;
                    } else if orn.name.starts_with("wavy-line-") {
                        // wavy-line-start, wavy-line-stop, wavy-line-continue
                        let wl_type = orn.name.strip_prefix("wavy-line-").unwrap_or("start");
                        let mut el = BytesStart::new("wavy-line");
                        el.push_attribute(("type", wl_type));
                        w.write_event(Event::Empty(el))?;
                    } else {
                        w.write_event(Event::Empty(BytesStart::new(&orn.name)))?;
                    }
                }
                w.write_event(Event::End(BytesEnd::new("ornaments")))?;
            }

            if !note.technicals.is_empty() {
                w.write_event(Event::Start(BytesStart::new("technical")))?;
                for tech in &note.technicals {
                    if tech.value.is_empty() {
                        w.write_event(Event::Empty(BytesStart::new(&tech.name)))?;
                    } else {
                        w.write_event(Event::Start(BytesStart::new(&tech.name)))?;
                        w.write_event(Event::Text(BytesText::new(&tech.value)))?;
                        w.write_event(Event::End(BytesEnd::new(&tech.name)))?;
                    }
                }
                w.write_event(Event::End(BytesEnd::new("technical")))?;
            }

            // Glissando
            if let Some(gliss) = &note.glissando {
                let mut el = BytesStart::new("glissando");
                el.push_attribute(("type", start_stop_str(gliss)));
                el.push_attribute(("number", "1"));
                if let Some(lt) = &note.glissando_line_type {
                    el.push_attribute(("line-type", lt.as_str()));
                }
                w.write_event(Event::Empty(el))?;
            }

            // Slide (portamento)
            if let Some(slide) = &note.slide {
                let mut el = BytesStart::new("slide");
                el.push_attribute(("type", start_stop_str(slide)));
                el.push_attribute(("number", "1"));
                w.write_event(Event::Empty(el))?;
            }

            // Arpeggiate / non-arpeggiate (from chord-level flag)
            if let Some(arp) = chord_arpeggio {
                match arp {
                    ArpeggioType::Up => {
                        let mut el = BytesStart::new("arpeggiate");
                        el.push_attribute(("direction", "up"));
                        w.write_event(Event::Empty(el))?;
                    }
                    ArpeggioType::Down => {
                        let mut el = BytesStart::new("arpeggiate");
                        el.push_attribute(("direction", "down"));
                        w.write_event(Event::Empty(el))?;
                    }
                    ArpeggioType::NonArpeggio => {
                        w.write_event(Event::Empty(BytesStart::new("non-arpeggiate")))?;
                    }
                }
            }

            w.write_event(Event::End(BytesEnd::new("notations")))?;
        }

        // Lyrics
        for syllable in &note.lyrics {
            let mut lyric_el = BytesStart::new("lyric");
            lyric_el.push_attribute(("number", syllable.number.to_string().as_str()));
            w.write_event(Event::Start(lyric_el))?;
            let syllabic = match syllable.syllabic {
                crate::ir::articulation::SyllabicType::Single => "single",
                crate::ir::articulation::SyllabicType::Begin => "begin",
                crate::ir::articulation::SyllabicType::End => "end",
                crate::ir::articulation::SyllabicType::Middle => "middle",
            };
            text_element(w, "syllabic", syllabic)?;
            text_element(w, "text", &syllable.text)?;
            if syllable.elision {
                w.write_event(Event::Empty(BytesStart::new("elision")))?;
            }
            if syllable.extend {
                w.write_event(Event::Empty(BytesStart::new("extend")))?;
            }
            w.write_event(Event::End(BytesEnd::new("lyric")))?;
        }

        w.write_event(Event::End(BytesEnd::new("note")))?;
        Ok(())
    }

    // ── rest ──────────────────────────────────────────────────────────────

    fn write_rest(&self, w: &mut W, rest: &Rest, voice_num: u8, part_staves: u8) -> Result<()> {
        w.write_event(Event::Start(BytesStart::new("note")))?;

        let mut rest_el = BytesStart::new("rest");
        if rest.is_measure_rest {
            rest_el.push_attribute(("measure", "yes"));
        }
        let has_display = rest.display_step.is_some() || rest.display_octave.is_some();
        if has_display {
            w.write_event(Event::Start(rest_el))?;
            if let Some(step) = &rest.display_step {
                text_element(w, "display-step", step)?;
            }
            if let Some(oct) = rest.display_octave {
                text_element(w, "display-octave", &oct.to_string())?;
            }
            w.write_event(Event::End(BytesEnd::new("rest")))?;
        } else {
            w.write_event(Event::Empty(rest_el))?;
        }

        let dur_val = self.duration_to_divisions(&rest.duration);
        text_element(w, "duration", &dur_val.to_string())?;
        text_element(w, "voice", &voice_num.to_string())?;

        if let Some(mxml_type) = rest.duration.musicxml_type() {
            text_element(w, "type", mxml_type)?;
        }
        for _ in 0..rest.duration.dots {
            w.write_event(Event::Empty(BytesStart::new("dot")))?;
        }

        // Staff
        if part_staves > 1 || rest.staff > 1 {
            text_element(w, "staff", &rest.staff.to_string())?;
        }

        // Time modification (tuplets)
        if rest.duration.tuplet_actual != 1 || rest.duration.tuplet_normal != 1 {
            w.write_event(Event::Start(BytesStart::new("time-modification")))?;
            text_element(
                w,
                "actual-notes",
                &rest.duration.tuplet_actual.to_string(),
            )?;
            text_element(
                w,
                "normal-notes",
                &rest.duration.tuplet_normal.to_string(),
            )?;
            w.write_event(Event::End(BytesEnd::new("time-modification")))?;
        }

        // Notations (fermata, tuplet display)
        let has_notations = rest.fermata.is_some() || rest.tuplet.is_some();
        if has_notations {
            w.write_event(Event::Start(BytesStart::new("notations")))?;

            if let Some(tuplet) = &rest.tuplet {
                let mut el = BytesStart::new("tuplet");
                let t_str = match tuplet.tuplet_type {
                    StartStop::Start => "start",
                    StartStop::Stop => "stop",
                    StartStop::Continue => "start", // fallback
                };
                el.push_attribute(("type", t_str));
                if tuplet.bracket {
                    el.push_attribute(("bracket", "yes"));
                }
                w.write_event(Event::Empty(el))?;
            }

            if let Some(fermata) = &rest.fermata {
                let mut el = BytesStart::new("fermata");
                if fermata.inverted {
                    el.push_attribute(("type", "inverted"));
                }
                w.write_event(Event::Start(el))?;
                w.write_event(Event::Text(BytesText::new(&fermata.shape)))?;
                w.write_event(Event::End(BytesEnd::new("fermata")))?;
            }

            w.write_event(Event::End(BytesEnd::new("notations")))?;
        }

        w.write_event(Event::End(BytesEnd::new("note")))?;
        Ok(())
    }

    // ── chord ─────────────────────────────────────────────────────────────

    fn write_chord(&self, w: &mut W, chord: &Chord, voice_num: u8, part_staves: u8) -> Result<()> {
        for (i, note) in chord.notes.iter().enumerate() {
            self.write_note(w, note, voice_num, i > 0, chord.arpeggio, part_staves)?;
        }
        Ok(())
    }

    // ── note-level dynamics → direction ───────────────────────────────────

    /// Emit `<direction>` elements for dynamics, wedges, and text directions
    /// attached directly to a [`Note`] (populated by the LY→IR path).
    fn emit_note_directions(&self, w: &mut W, note: &Note) -> Result<()> {
        for dyn_mark in &note.dynamics {
            let dir = Direction {
                dynamic: Some(dyn_mark.clone()),
                placement: Placement::Below,
                ..Direction::default()
            };
            self.write_direction(w, &dir)?;
        }
        for wedge in &note.wedges {
            let dir = Direction {
                wedge: Some(wedge.clone()),
                placement: Placement::Below,
                ..Direction::default()
            };
            self.write_direction(w, &dir)?;
        }
        for td in &note.text_directions {
            let dir = Direction {
                text: Some(td.clone()),
                placement: td.placement,
                ..Direction::default()
            };
            self.write_direction(w, &dir)?;
        }
        Ok(())
    }

    // ── direction ─────────────────────────────────────────────────────────

    fn write_direction(&self, w: &mut W, direction: &Direction) -> Result<()> {
        let mut dir_el = BytesStart::new("direction");
        match direction.placement {
            Placement::Above => dir_el.push_attribute(("placement", "above")),
            Placement::Below => dir_el.push_attribute(("placement", "below")),
            Placement::Unspecified => {}
        }
        w.write_event(Event::Start(dir_el))?;

        // Collect direction-type groups. Each group becomes its own
        // <direction-type> element. We must never emit an empty one.
        // A "group" is a set of related child elements.

        // Group 1: dynamics
        if let Some(dyn_mark) = &direction.dynamic {
            w.write_event(Event::Start(BytesStart::new("direction-type")))?;
            w.write_event(Event::Start(BytesStart::new("dynamics")))?;
            w.write_event(Event::Empty(BytesStart::new(&dyn_mark.sign)))?;
            w.write_event(Event::End(BytesEnd::new("dynamics")))?;
            w.write_event(Event::End(BytesEnd::new("direction-type")))?;
        }

        // Group 2: wedge (cresc/decresc hairpin)
        if let Some(wedge) = &direction.wedge {
            w.write_event(Event::Start(BytesStart::new("direction-type")))?;
            let mut el = BytesStart::new("wedge");
            el.push_attribute(("type", wedge.wedge_type.as_str()));
            w.write_event(Event::Empty(el))?;
            w.write_event(Event::End(BytesEnd::new("direction-type")))?;
        }

        // Group 3: text direction (words)
        if let Some(text_dir) = &direction.text {
            w.write_event(Event::Start(BytesStart::new("direction-type")))?;
            let mut words_el = BytesStart::new("words");
            if let Some(ref fs) = text_dir.font_style {
                words_el.push_attribute(("font-style", fs.as_str()));
            }
            if let Some(ref fw) = text_dir.font_weight {
                words_el.push_attribute(("font-weight", fw.as_str()));
            }
            w.write_event(Event::Start(words_el))?;
            w.write_event(Event::Text(BytesText::new(&text_dir.text)))?;
            w.write_event(Event::End(BytesEnd::new("words")))?;
            w.write_event(Event::End(BytesEnd::new("direction-type")))?;
        }

        // Group 4: rehearsal mark
        if let Some(reh) = &direction.rehearsal {
            w.write_event(Event::Start(BytesStart::new("direction-type")))?;
            w.write_event(Event::Start(BytesStart::new("rehearsal")))?;
            w.write_event(Event::Text(BytesText::new(&reh.text)))?;
            w.write_event(Event::End(BytesEnd::new("rehearsal")))?;
            w.write_event(Event::End(BytesEnd::new("direction-type")))?;
        }

        // Group 5: octave shift
        if let Some(os) = &direction.octave_shift {
            w.write_event(Event::Start(BytesStart::new("direction-type")))?;
            let mut el = BytesStart::new("octave-shift");
            el.push_attribute(("type", os.shift_type.as_str()));
            el.push_attribute(("size", os.size.to_string().as_str()));
            w.write_event(Event::Empty(el))?;
            w.write_event(Event::End(BytesEnd::new("direction-type")))?;
        }

        // Group 6: pedal
        if let Some(ped) = &direction.pedal {
            w.write_event(Event::Start(BytesStart::new("direction-type")))?;
            let mut el = BytesStart::new("pedal");
            el.push_attribute(("type", ped.pedal_type.as_str()));
            if ped.line {
                el.push_attribute(("line", "yes"));
            }
            w.write_event(Event::Empty(el))?;
            w.write_event(Event::End(BytesEnd::new("direction-type")))?;
        }

        // Group 7: tempo (text label in one direction-type, metronome in another)
        if let Some(tempo) = &direction.tempo {
            if let Some(ref label) = tempo.text {
                w.write_event(Event::Start(BytesStart::new("direction-type")))?;
                w.write_event(Event::Start(BytesStart::new("words")))?;
                w.write_event(Event::Text(BytesText::new(label)))?;
                w.write_event(Event::End(BytesEnd::new("words")))?;
                w.write_event(Event::End(BytesEnd::new("direction-type")))?;
            }
            if let (Some(beat_unit), Some(per_min)) = (&tempo.beat_unit, tempo.per_minute) {
                w.write_event(Event::Start(BytesStart::new("direction-type")))?;
                w.write_event(Event::Start(BytesStart::new("metronome")))?;
                text_element(w, "beat-unit", beat_unit)?;
                for _ in 0..tempo.dots {
                    w.write_event(Event::Empty(BytesStart::new("beat-unit-dot")))?;
                }
                text_element(w, "per-minute", &format_float(per_min))?;
                w.write_event(Event::End(BytesEnd::new("metronome")))?;
                w.write_event(Event::End(BytesEnd::new("direction-type")))?;
            }
        }

        // Group 8: coda
        if direction.coda {
            w.write_event(Event::Start(BytesStart::new("direction-type")))?;
            w.write_event(Event::Empty(BytesStart::new("coda")))?;
            w.write_event(Event::End(BytesEnd::new("direction-type")))?;
        }

        // Group 9: segno
        if direction.segno {
            w.write_event(Event::Start(BytesStart::new("direction-type")))?;
            w.write_event(Event::Empty(BytesStart::new("segno")))?;
            w.write_event(Event::End(BytesEnd::new("direction-type")))?;
        }

        // Group 10: da capo text
        if let Some(text) = &direction.da_capo {
            w.write_event(Event::Start(BytesStart::new("direction-type")))?;
            w.write_event(Event::Start(BytesStart::new("words")))?;
            w.write_event(Event::Text(BytesText::new(text)))?;
            w.write_event(Event::End(BytesEnd::new("words")))?;
            w.write_event(Event::End(BytesEnd::new("direction-type")))?;
        }

        // Group 11: dal segno text
        if let Some(text) = &direction.dal_segno {
            w.write_event(Event::Start(BytesStart::new("direction-type")))?;
            w.write_event(Event::Start(BytesStart::new("words")))?;
            w.write_event(Event::Text(BytesText::new(text)))?;
            w.write_event(Event::End(BytesEnd::new("words")))?;
            w.write_event(Event::End(BytesEnd::new("direction-type")))?;
        }

        // Merged <sound> element for tempo, dacapo, dalsegno
        {
            let tempo_bpm = direction.tempo.as_ref().and_then(|t| t.per_minute);
            let has_dacapo = direction.da_capo.is_some();
            let has_dalsegno = direction.dal_segno.is_some();
            if tempo_bpm.is_some() || has_dacapo || has_dalsegno {
                let mut sound = BytesStart::new("sound");
                if let Some(bpm) = tempo_bpm {
                    sound.push_attribute(("tempo", format_float(bpm).as_str()));
                }
                if has_dacapo {
                    sound.push_attribute(("dacapo", "yes"));
                }
                if has_dalsegno {
                    sound.push_attribute(("dalsegno", "yes"));
                }
                w.write_event(Event::Empty(sound))?;
            }
        }

        // Instrument change (separate <sound> since it has child elements)
        if let Some(ref ic) = direction.instrument_change {
            w.write_event(Event::Start(BytesStart::new("sound")))?;
            let mut midi_el = BytesStart::new("midi-instrument");
            midi_el.push_attribute(("id", ic.instrument_id.as_str()));
            w.write_event(Event::Start(midi_el))?;
            if let Some(ref name) = ic.instrument_name {
                text_element(w, "midi-name", name)?;
            }
            w.write_event(Event::End(BytesEnd::new("midi-instrument")))?;
            w.write_event(Event::End(BytesEnd::new("sound")))?;
        }

        w.write_event(Event::End(BytesEnd::new("direction")))?;
        Ok(())
    }

    // ── page layout defaults ──────────────────────────────────────────────

    fn write_defaults(&self, w: &mut W, pl: &PageLayout) -> Result<()> {
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

    // ── harmony / chord symbols ───────────────────────────────────────────

    fn write_harmony(&self, w: &mut W, harmony: &Harmony) -> Result<()> {
        w.write_event(Event::Start(BytesStart::new("harmony")))?;

        // Root
        w.write_event(Event::Start(BytesStart::new("root")))?;
        text_element(w, "root-step", &harmony.root.step)?;
        if harmony.root.alter != 0.0 {
            text_element(w, "root-alter", &format_float(harmony.root.alter))?;
        }
        w.write_event(Event::End(BytesEnd::new("root")))?;

        // Kind
        text_element(w, "kind", &harmony.kind)?;

        // Bass (inversion / slash notation)
        if let Some(bass) = &harmony.bass {
            w.write_event(Event::Start(BytesStart::new("bass")))?;
            text_element(w, "bass-step", &bass.step)?;
            if bass.alter != 0.0 {
                text_element(w, "bass-alter", &format_float(bass.alter))?;
            }
            w.write_event(Event::End(BytesEnd::new("bass")))?;
        }

        // Degree modifications
        for deg in &harmony.degrees {
            w.write_event(Event::Start(BytesStart::new("degree")))?;
            text_element(w, "degree-value", &deg.value.to_string())?;
            text_element(w, "degree-alter", &format_float(deg.alter))?;
            text_element(w, "degree-type", &deg.degree_type)?;
            w.write_event(Event::End(BytesEnd::new("degree")))?;
        }

        // Offset from measure start in divisions
        if harmony.offset != 0 {
            text_element(w, "offset", &harmony.offset.to_string())?;
        }

        w.write_event(Event::End(BytesEnd::new("harmony")))?;
        Ok(())
    }

    // ── figured bass ──────────────────────────────────────────────────────

    fn write_figured_bass(&self, w: &mut W, fb: &FiguredBass) -> Result<()> {
        let mut el = BytesStart::new("figured-bass");
        if fb.parentheses {
            el.push_attribute(("parentheses", "yes"));
        }
        w.write_event(Event::Start(el))?;

        for figure in &fb.figures {
            w.write_event(Event::Start(BytesStart::new("figure")))?;
            if let Some(prefix) = &figure.prefix {
                text_element(w, "prefix", prefix)?;
            }
            if let Some(n) = figure.number {
                text_element(w, "figure-number", &n.to_string())?;
            }
            if let Some(suffix) = &figure.suffix {
                text_element(w, "suffix", suffix)?;
            }
            w.write_event(Event::End(BytesEnd::new("figure")))?;
        }

        let dur_val = self.duration_to_divisions(&fb.duration);
        text_element(w, "duration", &dur_val.to_string())?;

        w.write_event(Event::End(BytesEnd::new("figured-bass")))?;
        Ok(())
    }

    // ── duration math ─────────────────────────────────────────────────────

    /// Convert an IR Duration to MusicXML duration value.
    ///
    /// Formula: actual_duration * 4 * divisions
    fn duration_to_divisions(&self, duration: &crate::ir::duration::Duration) -> i64 {
        let actual = duration.actual_duration();
        let quarter_notes = actual * crate::ir::duration::Frac::from_integer(4);
        let result = quarter_notes * crate::ir::duration::Frac::from_integer(self.divisions as i64);
        // Round to nearest integer — should be exact for valid durations
        *result.numer() / *result.denom()
    }

    /// Calculate the total duration of a voice in divisions.
    fn voice_duration(&self, voice: &Voice) -> i64 {
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
        let result =
            total * crate::ir::duration::Frac::from_integer(4 * self.divisions as i64);
        *result.numer() / *result.denom()
    }
}

// ---------------------------------------------------------------------------
// Division auto-computation
// ---------------------------------------------------------------------------

/// Compute divisions per quarter note that exactly represent all durations in
/// the score (including tuplets and short durations).
///
/// Starting from `base` (typically 4), takes the LCM with every `tuplet_actual`
/// value and every base-duration denominator (in quarter-note units) found in
/// the score so that `duration_to_divisions` never truncates.
///
/// For example, a 32nd note has base = 1/32 of a whole note = 1/8 of a quarter,
/// so divisions must be divisible by 8.  A 64th note needs divisible by 16.
fn compute_score_divisions(score: &Score, base: u16) -> u16 {
    let mut result = base as u64;
    for part in score.parts() {
        for measure in &part.measures {
            for voice in &measure.voices {
                for elem in &voice.elements {
                    let dur = match elem {
                        VoiceElement::Note(n) => &n.duration,
                        VoiceElement::Rest(r) => &r.duration,
                        VoiceElement::Chord(c) => &c.duration,
                        VoiceElement::Forward(f) => &f.duration,
                        VoiceElement::Backup(b) => &b.duration,
                    };
                    // Account for tuplet ratios
                    if dur.tuplet_actual > 1 {
                        result = lcm_u64(result, dur.tuplet_actual as u64);
                    }
                    // Account for short base durations: base = n/d of a whole
                    // note, so in quarter-note units the denominator is d/4n.
                    // divisions must be divisible by that denominator.
                    let base_n = *dur.base.numer();
                    let base_d = *dur.base.denom();
                    // Quarter-note fraction = base * 4 = 4n/d.
                    // For this to produce an integer when multiplied by
                    // divisions, we need divisions * 4n / d to be integer,
                    // i.e. divisions must be divisible by d / gcd(d, 4n).
                    let g = gcd_u64(base_d.unsigned_abs(), (4 * base_n).unsigned_abs());
                    let needed = base_d.unsigned_abs() / g;
                    if needed > 1 {
                        result = lcm_u64(result, needed);
                    }
                }
            }
        }
    }
    result as u16
}

fn gcd_u64(mut a: u64, mut b: u64) -> u64 {
    while b != 0 {
        let t = b;
        b = a % b;
        a = t;
    }
    a
}

fn lcm_u64(a: u64, b: u64) -> u64 {
    a / gcd_u64(a, b) * b
}

// ---------------------------------------------------------------------------
// Utility functions
// ---------------------------------------------------------------------------

/// Write a simple `<tag>text</tag>` element.
fn text_element(w: &mut W, tag: &str, text: &str) -> Result<()> {
    w.write_event(Event::Start(BytesStart::new(tag)))?;
    w.write_event(Event::Text(BytesText::new(text)))?;
    w.write_event(Event::End(BytesEnd::new(tag)))?;
    Ok(())
}

/// Format a float, removing trailing ".0" for integer values.
fn format_float(val: f64) -> String {
    if val == val.floor() {
        format!("{}", val as i64)
    } else {
        format!("{val}")
    }
}

/// Map a chromatic alteration (stored as `Alter = Ratio<i32>`) to the
/// MusicXML accidental name used inside `<accidental>`.
fn alter_to_accidental_name(alter: crate::ir::pitch::Alter) -> &'static str {
    let num = *alter.numer();
    let den = *alter.denom();
    // Normalise to halves so we can match on small integers.
    let halves = num * 2 / den; // rounds toward zero
    match halves {
        -4 => "double-flat",
        -3 => "three-quarters-flat",
        -2 => "flat",
        -1 => "quarter-flat",
        0 => "natural",
        1 => "quarter-sharp",
        2 => "sharp",
        3 => "three-quarters-sharp",
        4 => "double-sharp",
        _ => "natural",
    }
}

/// Convert a `StartStop` enum to its MusicXML attribute string.
fn start_stop_str(ss: &StartStop) -> &'static str {
    match ss {
        StartStop::Start => "start",
        StartStop::Stop => "stop",
        StartStop::Continue => "continue",
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::articulation::{Articulation, Fermata, SlurEvent, TieEvent};
    use crate::ir::direction::TempoDirection;
    use crate::ir::duration::Duration;
    use crate::ir::measure::{Clef, KeyMode, KeySignature, MeasureAttributes, TimeSignature};
    use crate::ir::note::{Chord, Note, Rest};
    use crate::ir::pitch::{Pitch, PitchStep};
    use crate::ir::score::{Score, ScoreChild, ScoreMetadata};
    use crate::ir::voice::Voice;
    use crate::ir::Part;
    use std::collections::HashMap;

    fn make_simple_score() -> Score {
        let mut attrs = MeasureAttributes {
            divisions: 4,
            key: Some(KeySignature {
                fifths: 0,
                mode: KeyMode::Major,
            }),
            time: Some(TimeSignature::default()),
            clefs: HashMap::new(),
            transpose: None,
            staves: None,
            staff_lines: None,
        };
        attrs.clefs.insert(1, Clef::default());

        let note = Note::new(
            Pitch::new(PitchStep::C, 4),
            Duration::quarter(),
        );
        let voice = Voice {
            number: 1,
            elements: vec![VoiceElement::Note(Box::new(note))],
        };
        let measure = crate::ir::measure::Measure {
            number: 1,
            implicit: false,
            width: None,
            attributes: Some(attrs),
            left_barline: None,
            right_barline: None,
            directions: vec![],
            harmonies: vec![],
            figured_bass: vec![],
            print_object: true,
            multi_measure_rest: None,
            voices: vec![voice],
        };

        let part = Part {
            name: "Piano".to_string(),
            abbreviation: "Pno.".to_string(),
            part_id: "P1".to_string(),
            midi_instrument: String::new(),
            midi_channel: 0,
            midi_program: 0,
            staves: 1,
            measures: vec![measure],
        };

        Score {
            metadata: ScoreMetadata {
                title: Some("Test".to_string()),
                composer: Some("Composer".to_string()),
                ..Default::default()
            },
            page_layout: None,
            children: vec![ScoreChild::Part(part)],
        }
    }

    #[test]
    fn simple_score_structure() {
        let score = make_simple_score();
        let adapter = IrToMxmlAdapter::new();
        let xml = adapter.convert(&score).unwrap();

        assert!(xml.contains("<score-partwise"));
        assert!(xml.contains("<movement-title>Test</movement-title>"));
        assert!(xml.contains("type=\"composer\""));
        assert!(xml.contains("Composer"));
        assert!(xml.contains("<software>lytk</software>"));
        assert!(xml.contains("<score-part id=\"P1\""));
        assert!(xml.contains("<part-name>Piano</part-name>"));
        assert!(xml.contains("<part-abbreviation>Pno.</part-abbreviation>"));
        assert!(xml.contains("<part id=\"P1\""));
    }

    #[test]
    fn attributes_key_time_clef() {
        let score = make_simple_score();
        let adapter = IrToMxmlAdapter::new();
        let xml = adapter.convert(&score).unwrap();

        assert!(xml.contains("<divisions>4</divisions>"));
        assert!(xml.contains("<fifths>0</fifths>"));
        assert!(xml.contains("<mode>major</mode>"));
        assert!(xml.contains("<beats>4</beats>"));
        assert!(xml.contains("<beat-type>4</beat-type>"));
        assert!(xml.contains("<sign>G</sign>"));
        assert!(xml.contains("<line>2</line>"));
    }

    #[test]
    fn note_with_pitch() {
        let score = make_simple_score();
        let adapter = IrToMxmlAdapter::new();
        let xml = adapter.convert(&score).unwrap();

        assert!(xml.contains("<step>C</step>"));
        assert!(xml.contains("<octave>4</octave>"));
        assert!(xml.contains("<duration>4</duration>"));
        assert!(xml.contains("<voice>1</voice>"));
        assert!(xml.contains("<type>quarter</type>"));
    }

    #[test]
    fn rest_emission() {
        let rest = Rest::new(Duration::half());
        let voice = Voice {
            number: 1,
            elements: vec![VoiceElement::Rest(rest)],
        };
        let measure = crate::ir::measure::Measure {
            number: 1,
            implicit: false,
            width: None,
            attributes: None,
            left_barline: None,
            right_barline: None,
            directions: vec![],
            harmonies: vec![],
            figured_bass: vec![],
            print_object: true,
            multi_measure_rest: None,
            voices: vec![voice],
        };
        let part = Part {
            name: String::new(),
            part_id: "P1".to_string(),
            ..Part::new("P1")
        };
        let mut score = Score::new();
        let mut part = part;
        part.measures.push(measure);
        score.children.push(ScoreChild::Part(part));

        let adapter = IrToMxmlAdapter::new();
        let xml = adapter.convert(&score).unwrap();

        assert!(xml.contains("<rest/>"));
        assert!(xml.contains("<duration>8</duration>"));
        assert!(xml.contains("<type>half</type>"));
    }

    #[test]
    fn measure_rest() {
        let rest = Rest::measure_rest(Duration::whole());
        let voice = Voice {
            number: 1,
            elements: vec![VoiceElement::Rest(rest)],
        };
        let measure = crate::ir::measure::Measure {
            number: 1,
            implicit: false,
            width: None,
            attributes: None,
            left_barline: None,
            right_barline: None,
            directions: vec![],
            harmonies: vec![],
            figured_bass: vec![],
            print_object: true,
            multi_measure_rest: None,
            voices: vec![voice],
        };
        let mut part = Part::new("P1");
        part.measures.push(measure);
        let mut score = Score::new();
        score.children.push(ScoreChild::Part(part));

        let adapter = IrToMxmlAdapter::new();
        let xml = adapter.convert(&score).unwrap();

        assert!(xml.contains("measure=\"yes\""));
    }

    #[test]
    fn chord_emission() {
        let n1 = Note::new(Pitch::new(PitchStep::C, 4), Duration::quarter());
        let n2 = Note::new(Pitch::new(PitchStep::E, 4), Duration::quarter());
        let n3 = Note::new(Pitch::new(PitchStep::G, 4), Duration::quarter());
        let chord = Chord::new(Duration::quarter(), vec![n1, n2, n3]);
        let voice = Voice {
            number: 1,
            elements: vec![VoiceElement::Chord(chord)],
        };
        let measure = crate::ir::measure::Measure {
            number: 1,
            implicit: false,
            width: None,
            attributes: None,
            left_barline: None,
            right_barline: None,
            directions: vec![],
            harmonies: vec![],
            figured_bass: vec![],
            print_object: true,
            multi_measure_rest: None,
            voices: vec![voice],
        };
        let mut part = Part::new("P1");
        part.measures.push(measure);
        let mut score = Score::new();
        score.children.push(ScoreChild::Part(part));

        let adapter = IrToMxmlAdapter::new();
        let xml = adapter.convert(&score).unwrap();

        // First note has no <chord/>, subsequent notes do
        let chord_count = xml.matches("<chord/>").count();
        assert_eq!(chord_count, 2, "expected 2 <chord/> tags for a 3-note chord");
        assert!(xml.contains("<step>C</step>"));
        assert!(xml.contains("<step>E</step>"));
        assert!(xml.contains("<step>G</step>"));
    }

    #[test]
    fn note_with_tie() {
        let mut note = Note::new(Pitch::new(PitchStep::D, 4), Duration::half());
        note.ties.push(TieEvent {
            tie_type: StartStop::Start,
        });
        let voice = Voice {
            number: 1,
            elements: vec![VoiceElement::Note(Box::new(note))],
        };
        let measure = crate::ir::measure::Measure {
            number: 1,
            implicit: false,
            width: None,
            attributes: None,
            left_barline: None,
            right_barline: None,
            directions: vec![],
            harmonies: vec![],
            figured_bass: vec![],
            print_object: true,
            multi_measure_rest: None,
            voices: vec![voice],
        };
        let mut part = Part::new("P1");
        part.measures.push(measure);
        let mut score = Score::new();
        score.children.push(ScoreChild::Part(part));

        let adapter = IrToMxmlAdapter::new();
        let xml = adapter.convert(&score).unwrap();

        assert!(xml.contains("<tie type=\"start\""));
        assert!(xml.contains("<tied type=\"start\""));
    }

    #[test]
    fn note_with_articulations() {
        let mut note = Note::new(Pitch::new(PitchStep::E, 5), Duration::eighth());
        note.articulations.push(Articulation {
            name: "staccato".to_string(),
            placement: Placement::Above,
        });
        note.slurs.push(SlurEvent {
            slur_type: StartStop::Start,
            number: 1,
            placement: Placement::Above,
        });
        let voice = Voice {
            number: 1,
            elements: vec![VoiceElement::Note(Box::new(note))],
        };
        let measure = crate::ir::measure::Measure {
            number: 1,
            implicit: false,
            width: None,
            attributes: None,
            left_barline: None,
            right_barline: None,
            directions: vec![],
            harmonies: vec![],
            figured_bass: vec![],
            print_object: true,
            multi_measure_rest: None,
            voices: vec![voice],
        };
        let mut part = Part::new("P1");
        part.measures.push(measure);
        let mut score = Score::new();
        score.children.push(ScoreChild::Part(part));

        let adapter = IrToMxmlAdapter::new();
        let xml = adapter.convert(&score).unwrap();

        assert!(xml.contains("<notations>"));
        assert!(xml.contains("<slur type=\"start\""));
        assert!(xml.contains("<articulations>"));
        assert!(xml.contains("<staccato/>"));
    }

    #[test]
    fn direction_with_dynamics() {
        use crate::ir::articulation::DynamicMark;

        let dir = Direction {
            dynamic: Some(DynamicMark {
                sign: "ff".to_string(),
                placement: Placement::Below,
            }),
            ..Default::default()
        };
        let measure = crate::ir::measure::Measure {
            number: 1,
            implicit: false,
            width: None,
            attributes: None,
            left_barline: None,
            right_barline: None,
            directions: vec![dir],
            harmonies: vec![],
            figured_bass: vec![],
            print_object: true,
            multi_measure_rest: None,
            voices: vec![],
        };
        let mut part = Part::new("P1");
        part.measures.push(measure);
        let mut score = Score::new();
        score.children.push(ScoreChild::Part(part));

        let adapter = IrToMxmlAdapter::new();
        let xml = adapter.convert(&score).unwrap();

        assert!(xml.contains("<direction>"));
        assert!(xml.contains("<dynamics>"));
        assert!(xml.contains("<ff/>"));
    }

    #[test]
    fn direction_with_tempo() {
        let dir = Direction {
            tempo: Some(TempoDirection {
                text: None,
                beat_unit: Some("quarter".to_string()),
                per_minute: Some(120.0),
                dots: 0,
                placement: Placement::Above,
            }),
            ..Default::default()
        };
        let measure = crate::ir::measure::Measure {
            number: 1,
            implicit: false,
            width: None,
            attributes: None,
            left_barline: None,
            right_barline: None,
            directions: vec![dir],
            harmonies: vec![],
            figured_bass: vec![],
            print_object: true,
            multi_measure_rest: None,
            voices: vec![],
        };
        let mut part = Part::new("P1");
        part.measures.push(measure);
        let mut score = Score::new();
        score.children.push(ScoreChild::Part(part));

        let adapter = IrToMxmlAdapter::new();
        let xml = adapter.convert(&score).unwrap();

        assert!(xml.contains("<metronome>"));
        assert!(xml.contains("<beat-unit>quarter</beat-unit>"));
        assert!(xml.contains("<per-minute>120</per-minute>"));
    }

    #[test]
    fn fermata_on_note() {
        let mut note = Note::new(Pitch::new(PitchStep::G, 4), Duration::whole());
        note.fermata = Some(Fermata {
            shape: "normal".to_string(),
            inverted: false,
        });
        let voice = Voice {
            number: 1,
            elements: vec![VoiceElement::Note(Box::new(note))],
        };
        let measure = crate::ir::measure::Measure {
            number: 1,
            implicit: false,
            width: None,
            attributes: None,
            left_barline: None,
            right_barline: None,
            directions: vec![],
            harmonies: vec![],
            figured_bass: vec![],
            print_object: true,
            multi_measure_rest: None,
            voices: vec![voice],
        };
        let mut part = Part::new("P1");
        part.measures.push(measure);
        let mut score = Score::new();
        score.children.push(ScoreChild::Part(part));

        let adapter = IrToMxmlAdapter::new();
        let xml = adapter.convert(&score).unwrap();

        assert!(xml.contains("<fermata>normal</fermata>"));
    }

    #[test]
    fn multi_voice_backup() {
        let n1 = Note::new(Pitch::new(PitchStep::C, 5), Duration::quarter());
        let n2 = Note::new(Pitch::new(PitchStep::E, 4), Duration::quarter());
        let v1 = Voice {
            number: 1,
            elements: vec![VoiceElement::Note(Box::new(n1))],
        };
        let v2 = Voice {
            number: 2,
            elements: vec![VoiceElement::Note(Box::new(n2))],
        };
        let measure = crate::ir::measure::Measure {
            number: 1,
            implicit: false,
            width: None,
            attributes: None,
            left_barline: None,
            right_barline: None,
            directions: vec![],
            harmonies: vec![],
            figured_bass: vec![],
            print_object: true,
            multi_measure_rest: None,
            voices: vec![v1, v2],
        };
        let mut part = Part::new("P1");
        part.measures.push(measure);
        let mut score = Score::new();
        score.children.push(ScoreChild::Part(part));

        let adapter = IrToMxmlAdapter::new();
        let xml = adapter.convert(&score).unwrap();

        // Should contain a backup between the two voices
        assert!(xml.contains("<backup>"));
        assert!(xml.contains("<voice>1</voice>"));
        assert!(xml.contains("<voice>2</voice>"));
    }

    // ── accidental display tests ──────────────────────────────────────────

    #[test]
    fn test_emit_forced_accidental() {
        use crate::ir::pitch::AccidentalDisplay;

        let mut note = Note::new(Pitch::new(PitchStep::F, 4), Duration::quarter());
        note.pitch.alter = crate::ir::pitch::Alter::new(1, 1); // sharp
        note.pitch.accidental = AccidentalDisplay::Forced;

        let xml = emit_single_note(note);
        assert!(xml.contains("<accidental>sharp</accidental>"), "{xml}");
    }

    #[test]
    fn test_emit_cautionary_accidental() {
        use crate::ir::pitch::AccidentalDisplay;

        let mut note = Note::new(Pitch::new(PitchStep::B, 4), Duration::quarter());
        note.pitch.alter = crate::ir::pitch::Alter::new(-1, 1); // flat
        note.pitch.accidental = AccidentalDisplay::Cautionary;

        let xml = emit_single_note(note);
        assert!(
            xml.contains("<accidental cautionary=\"yes\">flat</accidental>"),
            "{xml}"
        );
    }

    #[test]
    fn test_emit_editorial_accidental() {
        use crate::ir::pitch::AccidentalDisplay;

        let mut note = Note::new(Pitch::new(PitchStep::C, 4), Duration::quarter());
        // natural – alter stays 0
        note.pitch.accidental = AccidentalDisplay::Editorial;

        let xml = emit_single_note(note);
        assert!(
            xml.contains("<accidental editorial=\"yes\">natural</accidental>"),
            "{xml}"
        );
    }

    // ── after-grace steal-time tests ──────────────────────────────────────

    #[test]
    fn test_emit_after_grace_steal_time() {
        let mut note = Note::new(Pitch::new(PitchStep::D, 5), Duration::eighth());
        note.is_grace = true;
        note.after_grace = true;

        let xml = emit_single_note(note);
        assert!(
            xml.contains("steal-time-previous=\"100\""),
            "{xml}"
        );
    }

    #[test]
    fn test_emit_regular_grace_has_no_steal_time() {
        let mut note = Note::new(Pitch::new(PitchStep::D, 5), Duration::eighth());
        note.is_grace = true;
        note.after_grace = false;

        let xml = emit_single_note(note);
        assert!(!xml.contains("steal-time-previous"), "{xml}");
        assert!(xml.contains("<grace/>"), "{xml}");
    }

    // ── glissando / slide tests ───────────────────────────────────────────

    #[test]
    fn test_emit_glissando_start() {
        use crate::ir::articulation::StartStop;

        let mut note = Note::new(Pitch::new(PitchStep::C, 4), Duration::quarter());
        note.glissando = Some(StartStop::Start);
        note.glissando_line_type = Some("wavy".to_string());

        let xml = emit_single_note(note);
        assert!(xml.contains("<glissando"), "{xml}");
        assert!(xml.contains("type=\"start\""), "{xml}");
        assert!(xml.contains("line-type=\"wavy\""), "{xml}");
    }

    #[test]
    fn test_emit_slide_stop() {
        use crate::ir::articulation::StartStop;

        let mut note = Note::new(Pitch::new(PitchStep::G, 4), Duration::quarter());
        note.slide = Some(StartStop::Stop);

        let xml = emit_single_note(note);
        assert!(xml.contains("<slide"), "{xml}");
        assert!(xml.contains("type=\"stop\""), "{xml}");
    }

    // ── arpeggiate / non-arpeggiate tests ────────────────────────────────

    #[test]
    fn test_emit_arpeggiate_up() {
        use crate::ir::note::ArpeggioType;

        let note1 = Note::new(Pitch::new(PitchStep::C, 4), Duration::quarter());
        let note2 = Note::new(Pitch::new(PitchStep::E, 4), Duration::quarter());
        let mut chord = Chord::new(Duration::quarter(), vec![note1, note2]);
        chord.arpeggio = Some(ArpeggioType::Up);

        let xml = emit_chord(chord);
        assert!(xml.contains("<arpeggiate"), "{xml}");
        assert!(xml.contains("direction=\"up\""), "{xml}");
    }

    #[test]
    fn test_emit_non_arpeggiate() {
        use crate::ir::note::ArpeggioType;

        let note1 = Note::new(Pitch::new(PitchStep::C, 4), Duration::quarter());
        let note2 = Note::new(Pitch::new(PitchStep::G, 4), Duration::quarter());
        let mut chord = Chord::new(Duration::quarter(), vec![note1, note2]);
        chord.arpeggio = Some(ArpeggioType::NonArpeggio);

        let xml = emit_chord(chord);
        assert!(xml.contains("<non-arpeggiate/>"), "{xml}");
    }

    // ── harmony tests ─────────────────────────────────────────────────────

    #[test]
    fn test_emit_harmony_basic() {
        use crate::ir::harmony::{ChordPitch, Harmony};

        let harmony = Harmony {
            root: ChordPitch { step: "C".to_string(), alter: 0.0 },
            kind: "major".to_string(),
            bass: None,
            degrees: vec![],
            offset: 0,
        };

        let xml = emit_measure_with_harmony(harmony);
        assert!(xml.contains("<harmony>"), "{xml}");
        assert!(xml.contains("<root-step>C</root-step>"), "{xml}");
        assert!(xml.contains("<kind>major</kind>"), "{xml}");
    }

    #[test]
    fn test_emit_harmony_with_bass() {
        use crate::ir::harmony::{ChordPitch, Harmony};

        let harmony = Harmony {
            root: ChordPitch { step: "G".to_string(), alter: 0.0 },
            kind: "major".to_string(),
            bass: Some(ChordPitch { step: "B".to_string(), alter: 0.0 }),
            degrees: vec![],
            offset: 0,
        };

        let xml = emit_measure_with_harmony(harmony);
        assert!(xml.contains("<bass-step>B</bass-step>"), "{xml}");
    }

    #[test]
    fn test_emit_harmony_with_offset() {
        use crate::ir::harmony::{ChordPitch, Harmony};

        let harmony = Harmony {
            root: ChordPitch { step: "F".to_string(), alter: 0.0 },
            kind: "minor".to_string(),
            bass: None,
            degrees: vec![],
            offset: 4,
        };

        let xml = emit_measure_with_harmony(harmony);
        assert!(xml.contains("<offset>4</offset>"), "{xml}");
    }

    // ── figured bass tests ────────────────────────────────────────────────

    #[test]
    fn test_emit_figured_bass() {
        use crate::ir::harmony::{Figure, FiguredBass};

        let fb = FiguredBass {
            figures: vec![
                Figure { number: Some(6), prefix: None, suffix: None },
                Figure { number: Some(4), prefix: None, suffix: None },
            ],
            duration: Duration::quarter(),
            parentheses: false,
            offset: 0,
        };

        let xml = emit_measure_with_figured_bass(fb);
        assert!(xml.contains("<figured-bass>"), "{xml}");
        assert!(xml.contains("<figure-number>6</figure-number>"), "{xml}");
        assert!(xml.contains("<figure-number>4</figure-number>"), "{xml}");
    }

    // ── page layout / defaults tests ──────────────────────────────────────

    #[test]
    fn test_emit_page_layout_defaults() {
        use crate::ir::score::PageLayout;

        let layout = PageLayout {
            page_height: Some(29.7),
            page_width: Some(21.0),
            left_margin: Some(1.5),
            right_margin: Some(1.5),
            top_margin: Some(1.5),
            bottom_margin: Some(1.5),
            system_distance: None,
            top_system_distance: None,
            staff_size: Some(20.0),
        };

        let mut score = make_simple_score();
        score.page_layout = Some(layout);

        let adapter = IrToMxmlAdapter::new();
        let xml = adapter.convert(&score).unwrap();

        assert!(xml.contains("<defaults>"), "{xml}");
        assert!(xml.contains("<scaling>"), "{xml}");
        assert!(xml.contains("<page-layout>"), "{xml}");
        assert!(xml.contains("<page-height>"), "{xml}");
        assert!(xml.contains("<page-margins"), "{xml}");
    }

    // ── coda / segno / da_capo / dal_segno tests ─────────────────────────

    #[test]
    fn test_emit_coda_direction() {
        use crate::ir::direction::Direction;

        let mut dir = Direction::default();
        dir.coda = true;

        let xml = emit_direction(dir);
        assert!(xml.contains("<coda/>"), "{xml}");
    }

    #[test]
    fn test_emit_segno_direction() {
        use crate::ir::direction::Direction;

        let mut dir = Direction::default();
        dir.segno = true;

        let xml = emit_direction(dir);
        assert!(xml.contains("<segno/>"), "{xml}");
    }

    #[test]
    fn test_emit_da_capo_direction() {
        use crate::ir::direction::Direction;

        let mut dir = Direction::default();
        dir.da_capo = Some("D.C.".to_string());

        let xml = emit_direction(dir);
        assert!(xml.contains("<words>D.C.</words>"), "{xml}");
        assert!(xml.contains("<sound dacapo=\"yes\"/>"), "{xml}");
    }

    #[test]
    fn test_emit_dal_segno_direction() {
        use crate::ir::direction::Direction;

        let mut dir = Direction::default();
        dir.dal_segno = Some("D.S.".to_string());

        let xml = emit_direction(dir);
        assert!(xml.contains("<words>D.S.</words>"), "{xml}");
        assert!(xml.contains("<sound dalsegno=\"yes\"/>"), "{xml}");
    }

    #[test]
    fn note_level_dynamics_emitted_as_direction() {
        use crate::ir::articulation::DynamicMark;

        let mut note = Note::new(Pitch::new(PitchStep::C, 4), Duration::quarter());
        note.dynamics.push(DynamicMark {
            sign: "ff".to_string(),
            placement: Placement::Below,
        });
        let xml = emit_single_note(note);

        assert!(
            xml.contains("<dynamics>"),
            "note-level dynamics should emit <direction> with <dynamics>: {xml}"
        );
        assert!(xml.contains("<ff/>"), "expected <ff/> in output: {xml}");
    }

    #[test]
    fn note_level_wedge_emitted_as_direction() {
        use crate::ir::articulation::Wedge;

        let mut note = Note::new(Pitch::new(PitchStep::D, 4), Duration::quarter());
        note.wedges.push(Wedge {
            wedge_type: "crescendo".to_string(),
            placement: Placement::Below,
        });
        let xml = emit_single_note(note);

        assert!(
            xml.contains("<wedge type=\"crescendo\""),
            "note-level wedge should emit <direction> with <wedge>: {xml}"
        );
    }

    #[test]
    fn triplet_divisions_auto_computed() {
        let mut note = Note::new(Pitch::new(PitchStep::C, 4), Duration::eighth());
        note.duration.tuplet_actual = 3;
        note.duration.tuplet_normal = 2;

        let voice = Voice {
            number: 1,
            elements: vec![VoiceElement::Note(Box::new(note))],
        };
        let mut measure = make_empty_measure();
        measure.attributes = Some(MeasureAttributes::default());
        measure.voices = vec![voice];
        let mut part = Part::new("P1");
        part.measures.push(measure);
        let mut score = Score::new();
        score.children.push(ScoreChild::Part(part));

        let adapter = IrToMxmlAdapter::new();
        let xml = adapter.convert(&score).unwrap();

        // With divisions=12 (lcm(4,3)), triplet eighth = duration 4
        assert!(
            xml.contains("<divisions>12</divisions>"),
            "divisions should be 12 for triplets: {xml}"
        );
        assert!(
            xml.contains("<duration>4</duration>"),
            "triplet eighth with divisions=12 should be duration 4: {xml}"
        );
    }

    // ── test helpers ──────────────────────────────────────────────────────

    fn emit_single_note(note: Note) -> String {
        let mut measure = make_empty_measure();
        let voice = Voice {
            number: 1,
            elements: vec![VoiceElement::Note(Box::new(note))],
        };
        measure.voices = vec![voice];
        emit_measure(measure)
    }

    fn emit_chord(chord: Chord) -> String {
        let mut measure = make_empty_measure();
        let voice = Voice {
            number: 1,
            elements: vec![VoiceElement::Chord(chord)],
        };
        measure.voices = vec![voice];
        emit_measure(measure)
    }

    fn emit_measure_with_harmony(harmony: crate::ir::harmony::Harmony) -> String {
        let mut measure = make_empty_measure();
        measure.harmonies = vec![harmony];
        emit_measure(measure)
    }

    fn emit_measure_with_figured_bass(fb: crate::ir::harmony::FiguredBass) -> String {
        let mut measure = make_empty_measure();
        measure.figured_bass = vec![fb];
        emit_measure(measure)
    }

    fn emit_direction(dir: crate::ir::direction::Direction) -> String {
        let mut measure = make_empty_measure();
        measure.directions = vec![dir];
        emit_measure(measure)
    }

    fn emit_measure(measure: crate::ir::measure::Measure) -> String {
        let mut part = Part::new("P1");
        part.measures.push(measure);
        let mut score = Score::new();
        score.children.push(ScoreChild::Part(part));
        let adapter = IrToMxmlAdapter::new();
        adapter.convert(&score).unwrap()
    }

    fn make_empty_measure() -> crate::ir::measure::Measure {
        crate::ir::measure::Measure {
            number: 1,
            implicit: false,
            width: None,
            attributes: None,
            left_barline: None,
            right_barline: None,
            directions: vec![],
            harmonies: vec![],
            figured_bass: vec![],
            print_object: true,
            multi_measure_rest: None,
            voices: vec![],
        }
    }

    #[test]
    fn tremolo_single_note_emission() {
        let mut note = Note::new(Pitch::new(PitchStep::C, 4), Duration::quarter());
        note.tremolo_marks = 3;
        note.ornaments.push(crate::ir::articulation::Ornament {
            name: "tremolo".to_string(),
            placement: Placement::Unspecified,
        });
        let voice = Voice {
            number: 1,
            elements: vec![VoiceElement::Note(Box::new(note))],
        };
        let mut measure = make_empty_measure();
        measure.voices.push(voice);
        let xml = emit_measure(measure);
        assert!(xml.contains("<tremolo type=\"single\">3</tremolo>"), "should emit tremolo with type and marks: {xml}");
    }

    #[test]
    fn tremolo_two_note_emission() {
        let mut n1 = Note::new(Pitch::new(PitchStep::C, 4), Duration::quarter());
        n1.tremolo_marks = 2;
        n1.two_note_tremolo = true;
        n1.tremolo_start = true;
        n1.ornaments.push(crate::ir::articulation::Ornament {
            name: "tremolo".to_string(),
            placement: Placement::Unspecified,
        });
        let mut n2 = Note::new(Pitch::new(PitchStep::E, 4), Duration::quarter());
        n2.tremolo_marks = 2;
        n2.two_note_tremolo = true;
        n2.tremolo_start = false;
        n2.ornaments.push(crate::ir::articulation::Ornament {
            name: "tremolo".to_string(),
            placement: Placement::Unspecified,
        });
        let voice = Voice {
            number: 1,
            elements: vec![
                VoiceElement::Note(Box::new(n1)),
                VoiceElement::Note(Box::new(n2)),
            ],
        };
        let mut measure = make_empty_measure();
        measure.voices.push(voice);
        let xml = emit_measure(measure);
        assert!(xml.contains("<tremolo type=\"start\">2</tremolo>"), "first note should have tremolo start: {xml}");
        assert!(xml.contains("<tremolo type=\"stop\">2</tremolo>"), "second note should have tremolo stop: {xml}");
    }

    #[test]
    fn layout_break_emission() {
        let mut measure = make_empty_measure();
        measure.directions.push(Direction {
            layout_break: Some(crate::ir::direction::LayoutBreakType::Page),
            ..Default::default()
        });
        let xml = emit_measure(measure);
        assert!(xml.contains("<print new-page=\"yes\""), "should emit page break as <print>: {xml}");
        assert!(!xml.contains("<direction>"), "layout-break-only directions should not emit <direction>: {xml}");
    }

    #[test]
    fn system_break_emission() {
        let mut measure = make_empty_measure();
        measure.directions.push(Direction {
            layout_break: Some(crate::ir::direction::LayoutBreakType::System),
            ..Default::default()
        });
        let xml = emit_measure(measure);
        assert!(xml.contains("<print new-system=\"yes\""), "should emit system break: {xml}");
    }

    #[test]
    fn staff_lines_emission() {
        let mut measure = make_empty_measure();
        measure.attributes = Some(crate::ir::measure::MeasureAttributes {
            staff_lines: Some(1),
            ..Default::default()
        });
        let xml = emit_measure(measure);
        assert!(xml.contains("<staff-details>"), "should emit staff-details: {xml}");
        assert!(xml.contains("<staff-lines>1</staff-lines>"), "should emit staff-lines: {xml}");
    }

    #[test]
    fn staff_lines_5_not_emitted() {
        let mut measure = make_empty_measure();
        measure.attributes = Some(crate::ir::measure::MeasureAttributes {
            staff_lines: Some(5),
            ..Default::default()
        });
        let xml = emit_measure(measure);
        assert!(!xml.contains("<staff-details>"), "standard 5-line staff should not emit staff-details: {xml}");
    }

    #[test]
    fn multi_measure_rest_emission() {
        let mut measure = make_empty_measure();
        measure.attributes = Some(crate::ir::measure::MeasureAttributes::default());
        measure.multi_measure_rest = Some(4);
        let xml = emit_measure(measure);
        assert!(xml.contains("<measure-style>"), "should emit measure-style: {xml}");
        assert!(xml.contains("<multiple-rest>4</multiple-rest>"), "should emit multiple-rest count: {xml}");
    }

    #[test]
    fn sound_tempo_with_metronome() {
        let mut measure = make_empty_measure();
        measure.directions.push(Direction {
            tempo: Some(crate::ir::direction::TempoDirection {
                text: None,
                beat_unit: Some("quarter".to_string()),
                per_minute: Some(120.0),
                dots: 0,
                placement: Placement::Above,
            }),
            ..Default::default()
        });
        let xml = emit_measure(measure);
        assert!(xml.contains("<metronome>"), "should emit metronome: {xml}");
        assert!(xml.contains("<sound tempo=\"120\""), "should also emit sound tempo: {xml}");
    }

    #[test]
    fn wavy_line_emission() {
        let mut note = Note::new(Pitch::new(PitchStep::D, 5), Duration::half());
        note.ornaments.push(crate::ir::articulation::Ornament {
            name: "trill-mark".to_string(),
            placement: Placement::Above,
        });
        note.ornaments.push(crate::ir::articulation::Ornament {
            name: "wavy-line-start".to_string(),
            placement: Placement::Above,
        });
        let voice = Voice {
            number: 1,
            elements: vec![VoiceElement::Note(Box::new(note))],
        };
        let mut measure = make_empty_measure();
        measure.voices.push(voice);
        let xml = emit_measure(measure);
        assert!(xml.contains("<trill-mark/>"), "should emit trill-mark: {xml}");
        assert!(xml.contains("<wavy-line type=\"start\""), "should emit wavy-line with type: {xml}");
    }

    // ── Phase 5: tests for newly-added emission features ─────────────────

    #[test]
    fn grace_slash_attribute() {
        let mut note = Note::new(Pitch::new(PitchStep::C, 4), Duration::new(num::rational::Ratio::new(1, 16)));
        note.is_grace = true;
        note.grace_slash = true;
        let voice = Voice {
            number: 1,
            elements: vec![VoiceElement::Note(Box::new(note))],
        };
        let mut measure = make_empty_measure();
        measure.voices.push(voice);
        let xml = emit_measure(measure);
        assert!(xml.contains("slash=\"yes\""), "should emit slash=yes on grace: {xml}");
    }

    #[test]
    fn print_object_no() {
        let mut note = Note::new(Pitch::new(PitchStep::C, 4), Duration::quarter());
        note.print_object = false;
        let voice = Voice {
            number: 1,
            elements: vec![VoiceElement::Note(Box::new(note))],
        };
        let mut measure = make_empty_measure();
        measure.voices.push(voice);
        let xml = emit_measure(measure);
        assert!(xml.contains("print-object=\"no\""), "should emit print-object=no: {xml}");
    }

    #[test]
    fn notehead_emission() {
        let mut note = Note::new(Pitch::new(PitchStep::C, 4), Duration::quarter());
        note.notehead = "x".to_string();
        let voice = Voice {
            number: 1,
            elements: vec![VoiceElement::Note(Box::new(note))],
        };
        let mut measure = make_empty_measure();
        measure.voices.push(voice);
        let xml = emit_measure(measure);
        assert!(xml.contains("<notehead>x</notehead>"), "should emit notehead: {xml}");
    }

    #[test]
    fn measure_width_attribute() {
        let mut measure = make_empty_measure();
        measure.width = Some(120.5);
        let xml = emit_measure(measure);
        assert!(xml.contains("width=\"120.5\""), "should emit width: {xml}");
    }

    #[test]
    fn text_direction_font_attrs() {
        let dir = Direction {
            text: Some(crate::ir::direction::TextDirection {
                text: "pizz.".to_string(),
                placement: Placement::Above,
                font_style: Some("italic".to_string()),
                font_weight: Some("bold".to_string()),
            }),
            ..Direction::default()
        };
        let xml = emit_direction(dir);
        assert!(xml.contains("font-style=\"italic\""), "should emit font-style: {xml}");
        assert!(xml.contains("font-weight=\"bold\""), "should emit font-weight: {xml}");
    }

    #[test]
    fn pedal_line_attribute() {
        let dir = Direction {
            pedal: Some(crate::ir::direction::PedalEvent {
                pedal_type: "start".to_string(),
                line: true,
            }),
            ..Direction::default()
        };
        let xml = emit_direction(dir);
        assert!(xml.contains("line=\"yes\""), "should emit line=yes: {xml}");
    }

    #[test]
    fn lyric_elision() {
        let mut note = Note::new(Pitch::new(PitchStep::C, 4), Duration::quarter());
        note.lyrics.push(crate::ir::articulation::LyricSyllable {
            text: "la".to_string(),
            syllabic: crate::ir::articulation::SyllabicType::Single,
            number: 1,
            extend: false,
            elision: true,
        });
        let voice = Voice {
            number: 1,
            elements: vec![VoiceElement::Note(Box::new(note))],
        };
        let mut measure = make_empty_measure();
        measure.voices.push(voice);
        let xml = emit_measure(measure);
        assert!(xml.contains("<elision/>"), "should emit elision: {xml}");
    }

    #[test]
    fn midi_instrument_in_score_part() {
        let part = Part {
            name: "Cello".to_string(),
            abbreviation: "Vc.".to_string(),
            part_id: "P1".to_string(),
            midi_instrument: "Cello".to_string(),
            midi_channel: 1,
            midi_program: 43,
            staves: 1,
            measures: vec![make_empty_measure()],
        };
        let mut score = Score::new();
        score.children.push(ScoreChild::Part(part));
        let adapter = IrToMxmlAdapter::new();
        let xml = adapter.convert(&score).unwrap();
        assert!(xml.contains("<midi-instrument"), "should emit midi-instrument: {xml}");
        assert!(xml.contains("<midi-channel>1</midi-channel>"), "should emit midi-channel: {xml}");
        assert!(xml.contains("<midi-program>43</midi-program>"), "should emit midi-program: {xml}");
        assert!(xml.contains("<midi-name>Cello</midi-name>"), "should emit midi-name: {xml}");
        assert!(xml.contains("<score-instrument"), "should emit score-instrument: {xml}");
    }

    #[test]
    fn subtitle_credit() {
        let mut score = Score::new();
        score.metadata.subtitle = Some("Op. 1".to_string());
        score.children.push(ScoreChild::Part(Part::new("P1")));
        let adapter = IrToMxmlAdapter::new();
        let xml = adapter.convert(&score).unwrap();
        assert!(xml.contains("<credit>"), "should emit credit: {xml}");
        assert!(xml.contains("<credit-type>subtitle</credit-type>"), "should emit credit-type: {xml}");
        assert!(xml.contains("Op. 1"), "should contain subtitle text: {xml}");
    }

    #[test]
    fn extra_creators() {
        let mut score = Score::new();
        score.metadata.extra.insert("editor".to_string(), "John".to_string());
        score.children.push(ScoreChild::Part(Part::new("P1")));
        let adapter = IrToMxmlAdapter::new();
        let xml = adapter.convert(&score).unwrap();
        assert!(xml.contains("type=\"editor\""), "should emit extra creator type: {xml}");
        assert!(xml.contains("John"), "should emit extra creator value: {xml}");
    }

    #[test]
    fn part_group_number_preserved() {
        let mut group = crate::ir::score::PartGroup::new("StaffGroup");
        group.number = 3;
        group.children.push(ScoreChild::Part(Part::new("P1")));
        let mut score = Score::new();
        score.children.push(ScoreChild::PartGroup(group));
        let adapter = IrToMxmlAdapter::new();
        let xml = adapter.convert(&score).unwrap();
        assert!(xml.contains("number=\"3\""), "should preserve group number: {xml}");
    }

    #[test]
    fn tempo_text_as_words() {
        let dir = Direction {
            tempo: Some(TempoDirection {
                text: Some("Allegro".to_string()),
                beat_unit: Some("quarter".to_string()),
                per_minute: Some(120.0),
                dots: 0,
                placement: Placement::Above,
            }),
            ..Direction::default()
        };
        let xml = emit_direction(dir);
        assert!(xml.contains("<words>Allegro</words>"), "should emit tempo text as words: {xml}");
        assert!(xml.contains("<metronome>"), "should emit metronome: {xml}");
        // Words should come before metronome
        let words_pos = xml.find("<words>Allegro</words>").unwrap();
        let metro_pos = xml.find("<metronome>").unwrap();
        assert!(words_pos < metro_pos, "words should precede metronome: {xml}");
    }

    #[test]
    fn sound_element_merged() {
        let dir = Direction {
            tempo: Some(TempoDirection {
                text: None,
                beat_unit: Some("quarter".to_string()),
                per_minute: Some(120.0),
                dots: 0,
                placement: Placement::Above,
            }),
            da_capo: Some("D.C.".to_string()),
            ..Direction::default()
        };
        let xml = emit_direction(dir);
        // Should have a single <sound with both tempo and dacapo
        assert!(xml.contains("tempo=\"120\""), "should emit tempo in sound: {xml}");
        assert!(xml.contains("dacapo=\"yes\""), "should emit dacapo in sound: {xml}");
        // Count <sound occurrences — should be exactly 1
        let sound_count = xml.matches("<sound ").count();
        assert_eq!(sound_count, 1, "should merge into single sound element: {xml}");
    }

    #[test]
    fn test_music_to_mxml_round_trip() {
        use crate::adapters::FromMusicAdapter;
        use crate::ir::annotation::Annotation;
        use crate::ir::music::{ContextType, Music, MusicDocument};
        use crate::ir::pitch::{Pitch, PitchStep};

        let music = Music::Sequential(vec![
            Music::TimeSignature(TimeSignature {
                beats: "4".to_string(),
                beat_type: 4,
                symbol: None,
            }),
            Music::Note {
                pitch: Pitch::new(PitchStep::C, 4),
                duration: Duration::quarter(),
                annotations: vec![],
            },
            Music::Note {
                pitch: Pitch::new(PitchStep::D, 4),
                duration: Duration::quarter(),
                annotations: vec![],
            },
            Music::Note {
                pitch: Pitch::new(PitchStep::E, 4),
                duration: Duration::quarter(),
                annotations: vec![],
            },
            Music::Note {
                pitch: Pitch::new(PitchStep::F, 4),
                duration: Duration::quarter(),
                annotations: vec![],
            },
        ])
        .in_context(ContextType::Staff, None);

        let doc = MusicDocument::new(music);

        let adapter = IrToMxmlAdapter::new();
        let xml = adapter.convert_music(&doc).unwrap();

        assert!(xml.contains("<note"), "should contain notes");
        assert!(xml.contains("<time>"), "should contain time signature");
        assert!(xml.contains("<step>C</step>"), "should contain C note");
    }
}
