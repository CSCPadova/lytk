//! Direction, dynamics, wedge, pedal, harmony, and figured bass MusicXML emission.

use quick_xml::events::{BytesEnd, BytesStart, BytesText, Event};

use super::helpers::{format_float, text_element};
use super::{IrToMxmlAdapter, W};
use crate::adapters::Result;
use crate::ir::articulation::Placement;
use crate::ir::direction::Direction;
use crate::ir::harmony::{FiguredBass, Harmony};
use crate::ir::note::Note;

impl IrToMxmlAdapter {
    /// Emit `<direction>` elements for dynamics, wedges, and text directions
    /// attached directly to a [`Note`] (populated by the LY->IR path).
    pub(super) fn emit_note_directions(&self, w: &mut W, note: &Note) -> Result<()> {
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

    pub(super) fn write_direction(&self, w: &mut W, direction: &Direction) -> Result<()> {
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

    pub(super) fn write_harmony(&self, w: &mut W, harmony: &Harmony) -> Result<()> {
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

    pub(super) fn write_figured_bass(&self, w: &mut W, fb: &FiguredBass) -> Result<()> {
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
}
