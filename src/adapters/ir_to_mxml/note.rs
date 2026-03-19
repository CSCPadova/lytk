//! Note, chord, and rest MusicXML emission.

use quick_xml::events::{BytesEnd, BytesStart, BytesText, Event};

use super::helpers::{alter_to_accidental_name, start_stop_str, text_element};
use super::{IrToMxmlAdapter, W};
use crate::adapters::Result;
use crate::ir::articulation::StartStop;
use crate::ir::note::{ArpeggioType, Chord, Note, Rest};
use crate::ir::pitch::AccidentalDisplay;

impl IrToMxmlAdapter {
    pub(super) fn write_note(
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
            text_element(w, "alter", &super::helpers::format_float(alter_val))?;
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
            text_element(w, "actual-notes", &note.duration.tuplet_actual.to_string())?;
            text_element(w, "normal-notes", &note.duration.tuplet_normal.to_string())?;
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
                            el.push_attribute((
                                "type",
                                if note.tremolo_start { "start" } else { "stop" },
                            ));
                        } else {
                            el.push_attribute(("type", "single"));
                        }
                        w.write_event(Event::Start(el))?;
                        w.write_event(Event::Text(BytesText::new(
                            &note.tremolo_marks.to_string(),
                        )))?;
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

    pub(super) fn write_rest(
        &self,
        w: &mut W,
        rest: &Rest,
        voice_num: u8,
        part_staves: u8,
    ) -> Result<()> {
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
            text_element(w, "actual-notes", &rest.duration.tuplet_actual.to_string())?;
            text_element(w, "normal-notes", &rest.duration.tuplet_normal.to_string())?;
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

    pub(super) fn write_chord(
        &self,
        w: &mut W,
        chord: &Chord,
        voice_num: u8,
        part_staves: u8,
    ) -> Result<()> {
        for (i, note) in chord.notes.iter().enumerate() {
            self.write_note(w, note, voice_num, i > 0, chord.arpeggio, part_staves)?;
        }
        Ok(())
    }
}
