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
use crate::ir::measure::{ClefSign, Measure, MeasureAttributes};
use crate::ir::note::{Chord, Note, Rest, VoiceElement};
use crate::ir::score::{Score, ScoreChild};
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
        let buf = Cursor::new(Vec::new());
        let mut w = Writer::new_with_indent(buf, b' ', 2);

        // XML declaration
        w.write_event(Event::Decl(BytesDecl::new("1.0", Some("UTF-8"), None)))?;

        self.write_score(&mut w, score)?;

        let bytes = w.into_inner().into_inner();
        Ok(String::from_utf8(bytes).expect("XML output is valid UTF-8"))
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

        let mut group_number: u8 = 0;
        for child in &score.children {
            match child {
                ScoreChild::Part(part) => {
                    self.write_score_part(w, part)?;
                }
                ScoreChild::PartGroup(group) => {
                    group_number += 1;

                    // part-group start
                    let mut pg = BytesStart::new("part-group");
                    pg.push_attribute(("type", "start"));
                    pg.push_attribute(("number", group_number.to_string().as_str()));
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
                    pg_stop.push_attribute(("number", group_number.to_string().as_str()));
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
            self.write_measure(w, measure)?;
        }

        w.write_event(Event::End(BytesEnd::new("part")))?;
        Ok(())
    }

    fn write_measure(&self, w: &mut W, measure: &Measure) -> Result<()> {
        let mut el = BytesStart::new("measure");
        el.push_attribute(("number", measure.number.to_string().as_str()));
        if measure.implicit {
            el.push_attribute(("implicit", "yes"));
        }
        w.write_event(Event::Start(el))?;

        // Attributes
        if let Some(attrs) = &measure.attributes {
            self.write_attributes(w, attrs)?;
        }

        // Left barline
        if let Some(bl) = &measure.left_barline {
            self.write_barline(w, bl, "left")?;
        }

        // Directions
        for dir in &measure.directions {
            self.write_direction(w, dir)?;
        }

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

            for elem in &voice.elements {
                match elem {
                    VoiceElement::Note(n) => {
                        self.write_note(w, n, voice.number, false)?;
                    }
                    VoiceElement::Rest(r) => {
                        self.write_rest(w, r, voice.number)?;
                    }
                    VoiceElement::Chord(c) => {
                        self.write_chord(w, c, voice.number)?;
                    }
                    VoiceElement::Forward(fwd) => {
                        w.write_event(Event::Start(BytesStart::new("forward")))?;
                        let dur_val = self.duration_to_divisions(&fwd.duration);
                        text_element(w, "duration", &dur_val.to_string())?;
                        w.write_event(Event::End(BytesEnd::new("forward")))?;
                    }
                    VoiceElement::Backup(bk) => {
                        w.write_event(Event::Start(BytesStart::new("backup")))?;
                        let dur_val = self.duration_to_divisions(&bk.duration);
                        text_element(w, "duration", &dur_val.to_string())?;
                        w.write_event(Event::End(BytesEnd::new("backup")))?;
                    }
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

    fn write_attributes(&self, w: &mut W, attrs: &MeasureAttributes) -> Result<()> {
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
    ) -> Result<()> {
        w.write_event(Event::Start(BytesStart::new("note")))?;

        if is_chord {
            w.write_event(Event::Empty(BytesStart::new("chord")))?;
        }
        if note.is_grace {
            w.write_event(Event::Empty(BytesStart::new("grace")))?;
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

        // Staff
        if note.staff > 1 {
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
            || note.tuplet.is_some();

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
                    w.write_event(Event::Empty(BytesStart::new(&orn.name)))?;
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
            if syllable.extend {
                w.write_event(Event::Empty(BytesStart::new("extend")))?;
            }
            w.write_event(Event::End(BytesEnd::new("lyric")))?;
        }

        w.write_event(Event::End(BytesEnd::new("note")))?;
        Ok(())
    }

    // ── rest ──────────────────────────────────────────────────────────────

    fn write_rest(&self, w: &mut W, rest: &Rest, voice_num: u8) -> Result<()> {
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

        if let Some(fermata) = &rest.fermata {
            w.write_event(Event::Start(BytesStart::new("notations")))?;
            let mut el = BytesStart::new("fermata");
            if fermata.inverted {
                el.push_attribute(("type", "inverted"));
            }
            w.write_event(Event::Start(el))?;
            w.write_event(Event::Text(BytesText::new(&fermata.shape)))?;
            w.write_event(Event::End(BytesEnd::new("fermata")))?;
            w.write_event(Event::End(BytesEnd::new("notations")))?;
        }

        w.write_event(Event::End(BytesEnd::new("note")))?;
        Ok(())
    }

    // ── chord ─────────────────────────────────────────────────────────────

    fn write_chord(&self, w: &mut W, chord: &Chord, voice_num: u8) -> Result<()> {
        for (i, note) in chord.notes.iter().enumerate() {
            self.write_note(w, note, voice_num, i > 0)?;
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

        w.write_event(Event::Start(BytesStart::new("direction-type")))?;

        if let Some(dyn_mark) = &direction.dynamic {
            w.write_event(Event::Start(BytesStart::new("dynamics")))?;
            w.write_event(Event::Empty(BytesStart::new(&dyn_mark.sign)))?;
            w.write_event(Event::End(BytesEnd::new("dynamics")))?;
        }

        if let Some(wedge) = &direction.wedge {
            let mut el = BytesStart::new("wedge");
            el.push_attribute(("type", wedge.wedge_type.as_str()));
            w.write_event(Event::Empty(el))?;
        }

        if let Some(text_dir) = &direction.text {
            w.write_event(Event::Start(BytesStart::new("words")))?;
            w.write_event(Event::Text(BytesText::new(&text_dir.text)))?;
            w.write_event(Event::End(BytesEnd::new("words")))?;
        }

        if let Some(reh) = &direction.rehearsal {
            w.write_event(Event::Start(BytesStart::new("rehearsal")))?;
            w.write_event(Event::Text(BytesText::new(&reh.text)))?;
            w.write_event(Event::End(BytesEnd::new("rehearsal")))?;
        }

        if let Some(os) = &direction.octave_shift {
            let mut el = BytesStart::new("octave-shift");
            el.push_attribute(("type", os.shift_type.as_str()));
            el.push_attribute(("size", os.size.to_string().as_str()));
            w.write_event(Event::Empty(el))?;
        }

        if let Some(ped) = &direction.pedal {
            let mut el = BytesStart::new("pedal");
            el.push_attribute(("type", ped.pedal_type.as_str()));
            w.write_event(Event::Empty(el))?;
        }

        if let Some(tempo) = &direction.tempo {
            if let (Some(beat_unit), Some(per_min)) = (&tempo.beat_unit, tempo.per_minute) {
                w.write_event(Event::Start(BytesStart::new("metronome")))?;
                text_element(w, "beat-unit", beat_unit)?;
                for _ in 0..tempo.dots {
                    w.write_event(Event::Empty(BytesStart::new("beat-unit-dot")))?;
                }
                text_element(w, "per-minute", &format_float(per_min))?;
                w.write_event(Event::End(BytesEnd::new("metronome")))?;
            }
        }

        w.write_event(Event::End(BytesEnd::new("direction-type")))?;

        // Sound element for tempo without beat-unit
        if let Some(tempo) = &direction.tempo {
            if tempo.beat_unit.is_none() {
                if let Some(per_min) = tempo.per_minute {
                    let mut sound = BytesStart::new("sound");
                    sound.push_attribute(("tempo", format_float(per_min).as_str()));
                    w.write_event(Event::Empty(sound))?;
                }
            }
        }

        w.write_event(Event::End(BytesEnd::new("direction")))?;
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
        };
        attrs.clefs.insert(1, Clef::default());

        let note = Note::new(
            Pitch::new(PitchStep::C, 4),
            Duration::quarter(),
        );
        let voice = Voice {
            number: 1,
            elements: vec![VoiceElement::Note(note)],
        };
        let measure = crate::ir::measure::Measure {
            number: 1,
            implicit: false,
            width: None,
            attributes: Some(attrs),
            left_barline: None,
            right_barline: None,
            directions: vec![],
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
            elements: vec![VoiceElement::Note(note)],
        };
        let measure = crate::ir::measure::Measure {
            number: 1,
            implicit: false,
            width: None,
            attributes: None,
            left_barline: None,
            right_barline: None,
            directions: vec![],
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
            elements: vec![VoiceElement::Note(note)],
        };
        let measure = crate::ir::measure::Measure {
            number: 1,
            implicit: false,
            width: None,
            attributes: None,
            left_barline: None,
            right_barline: None,
            directions: vec![],
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
            elements: vec![VoiceElement::Note(note)],
        };
        let measure = crate::ir::measure::Measure {
            number: 1,
            implicit: false,
            width: None,
            attributes: None,
            left_barline: None,
            right_barline: None,
            directions: vec![],
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
            elements: vec![VoiceElement::Note(n1)],
        };
        let v2 = Voice {
            number: 2,
            elements: vec![VoiceElement::Note(n2)],
        };
        let measure = crate::ir::measure::Measure {
            number: 1,
            implicit: false,
            width: None,
            attributes: None,
            left_barline: None,
            right_barline: None,
            directions: vec![],
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
}
