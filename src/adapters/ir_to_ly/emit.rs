//! Measure, voice element, note/chord/rest emission.

use crate::ir::articulation::{StartStop, TupletDisplay};
use crate::ir::duration::{Duration, Frac};
use crate::ir::language::{PitchLanguage, PitchMode};
use crate::ir::note::{ArpeggioType, Chord, Note, Rest, VoiceElement};
use crate::ir::pitch::Pitch;
use crate::ir::voice::Voice;
use crate::ir::Part;

use super::maps::{
    articulation_to_ly, clef_to_ly, duration_to_ly, key_to_ly, ornament_to_ly, pitch_to_ly,
    tempo_to_ly, time_to_ly, tremolo_suffix,
};

/// Persistent state across measure boundaries during LilyPond emission.
#[derive(Default)]
pub(super) struct EmitState {
    pub(super) prev_pitch: Option<Pitch>,
    pub(super) auto_beam_off: bool,
    pub(super) in_melisma: bool,
}

/// Extract the tuplet display hint from a voice element, if present.
fn element_tuplet(elem: &VoiceElement) -> Option<&TupletDisplay> {
    match elem {
        VoiceElement::Note(n) => n.tuplet.as_ref(),
        VoiceElement::Chord(c) => c.notes.first().and_then(|n| n.tuplet.as_ref()),
        VoiceElement::Rest(r) => r.tuplet.as_ref(),
        _ => None,
    }
}

/// Extract the tuplet ratio (actual, normal) from a voice element's duration.
fn element_tuplet_ratio(elem: &VoiceElement) -> (u8, u8) {
    let dur = match elem {
        VoiceElement::Note(n) => &n.duration,
        VoiceElement::Rest(r) => &r.duration,
        VoiceElement::Chord(c) => &c.duration,
        VoiceElement::Forward(f) => &f.duration,
        VoiceElement::Backup(b) => &b.duration,
    };
    (dur.tuplet_actual, dur.tuplet_normal)
}

/// Convert a Duration to a number of MusicXML divisions.
fn duration_to_divisions(dur: &Duration, divisions: i64) -> i64 {
    let frac = dur.actual_duration() * Frac::from_integer(4 * divisions);
    // Should always be an integer when divisions is correctly set.
    (*frac.numer() / *frac.denom()).max(0)
}

pub(super) fn emit_measures(
    part: &Part,
    lang: PitchLanguage,
    mode: PitchMode,
    staff_filter: Option<u8>,
    partial_dur: Option<&Duration>,
    indent: usize,
    lines: &mut Vec<String>,
) {
    let pad = " ".repeat(indent);
    let mut emit_state = EmitState::default();
    let mut is_first_measure = true;
    let mut last_divisions: i64 = 1;

    for measure in &part.measures {
        // Anacrusis: emit \partial before first measure
        if is_first_measure {
            if let Some(dur) = partial_dur {
                lines.push(format!("{pad}\\partial {}", duration_to_ly(dur)));
            }
            is_first_measure = false;
        }

        // Attributes
        if let Some(attrs) = &measure.attributes {
            if let Some(key) = &attrs.key {
                lines.push(format!("{pad}{}", key_to_ly(key)));
            }
            if let Some(ts) = &attrs.time {
                lines.push(format!("{pad}{}", time_to_ly(ts)));
            }
            // Clef for our staff
            let staff_num = staff_filter.unwrap_or(1);
            if let Some(clef) = attrs.clefs.get(&staff_num) {
                lines.push(format!("{pad}{}", clef_to_ly(clef)));
            } else if staff_filter.is_none() {
                // Single-staff: emit first clef
                if let Some(clef) = attrs.clefs.values().next() {
                    lines.push(format!("{pad}{}", clef_to_ly(clef)));
                }
            }
        }

        // Separate directions into standalone (tempo, rehearsal) and note-attached (dynamics, wedges, markup).
        // Note-attached directions are grouped by their forward-position offset
        // (populated in mxml_to_ir) so they attach to the correct voice element.
        let mut dir_at_offset: std::collections::BTreeMap<i32, Vec<String>> =
            std::collections::BTreeMap::new();
        for dir in &measure.directions {
            // Tempo and rehearsal marks can stand alone
            if let Some(tempo) = &dir.tempo {
                lines.push(format!("{pad}{}", tempo_to_ly(tempo)));
            }
            if dir.rehearsal.is_some() {
                lines.push(format!("{pad}\\mark \\default"));
            }
            if dir.coda {
                lines.push(format!(
                    "{pad}\\mark \\markup {{ \\musicglyph \"scripts.coda\" }}"
                ));
            }
            if dir.segno {
                lines.push(format!(
                    "{pad}\\mark \\markup {{ \\musicglyph \"scripts.segno\" }}"
                ));
            }
            if let Some(text) = &dir.da_capo {
                lines.push(format!("{pad}\\mark \"{text}\""));
            }
            if let Some(text) = &dir.dal_segno {
                lines.push(format!("{pad}\\mark \"{text}\""));
            }
            if let Some(lb) = &dir.layout_break {
                match lb {
                    crate::ir::direction::LayoutBreakType::System => {
                        lines.push(format!("{pad}\\break"));
                    }
                    crate::ir::direction::LayoutBreakType::Page => {
                        lines.push(format!("{pad}\\pageBreak"));
                    }
                    crate::ir::direction::LayoutBreakType::Section => {
                        lines.push(format!("{pad}\\section"));
                    }
                }
            }
            // Dynamics, wedges, text, pedal, octave shifts must attach to a note
            let mut parts: Vec<String> = Vec::new();
            if let Some(dyn_mark) = &dir.dynamic {
                parts.push(format!("\\{}", dyn_mark.sign));
            }
            if let Some(wedge) = &dir.wedge {
                let cmd = match wedge.wedge_type.as_str() {
                    "crescendo" => "\\<",
                    "diminuendo" => "\\>",
                    "stop" => "\\!",
                    _ => "",
                };
                if !cmd.is_empty() {
                    parts.push(cmd.to_string());
                }
            }
            if let Some(text) = &dir.text {
                if !text.text.is_empty() {
                    parts.push(format!("^\\markup {{ \"{}\" }}", text.text));
                }
            }
            if let Some(pedal) = &dir.pedal {
                match pedal.pedal_type.as_str() {
                    "start" => parts.push("\\sustainOn".to_string()),
                    "stop" => parts.push("\\sustainOff".to_string()),
                    "change" => parts.push("\\sustainOff\\sustainOn".to_string()),
                    _ => {}
                }
            }
            if let Some(oct) = &dir.octave_shift {
                match oct.shift_type.as_str() {
                    "up" => parts.push(format!("\\ottava #{}", oct.size / 8)),
                    "down" => parts.push(format!("\\ottava #-{}", oct.size / 8)),
                    "stop" => parts.push("\\ottava #0".to_string()),
                    _ => {}
                }
            }
            if !parts.is_empty() {
                dir_at_offset.entry(dir.offset).or_default().extend(parts);
            }
        }

        // Compute divisions for mapping direction offsets to voice element indices.
        let divisions: i64 = measure
            .attributes
            .as_ref()
            .map(|a| a.divisions as i64)
            .unwrap_or(last_divisions);
        last_divisions = divisions;

        // Left barline
        if let Some(bl) = &measure.left_barline {
            if bl.repeat_direction.is_some() && bl.ending_number.is_none() {
                lines.push(format!("{pad}\\repeat volta 2 {{"));
            }
            if let Some(ending_num) = bl.ending_number {
                if bl.ending_type.as_deref() == Some("start") {
                    if ending_num == 1 {
                        // Close the repeat body and open \alternative
                        lines.push(format!("{pad}}}"));
                        lines.push(format!("{pad}\\alternative {{"));
                    }
                    // Open this alternative's block
                    lines.push(format!("{pad}  {{"));
                }
            }
        }

        // Voices
        let voices: Vec<&Voice> = if let Some(sf) = staff_filter {
            measure
                .voices
                .iter()
                .filter(|v| super::voice_matches_staff(v, sf))
                .filter(|v| super::voice_has_content(v))
                .collect()
        } else {
            measure.voices.iter().collect()
        };

        if voices.len() <= 1 {
            if let Some(voice) = voices.first() {
                emit_voice_elements(
                    voice,
                    lang,
                    mode,
                    &mut emit_state,
                    &pad,
                    &dir_at_offset,
                    divisions,
                    lines,
                );
            }
        } else {
            // Multi-voice: << \\ >> syntax
            lines.push(format!("{pad}<<"));
            let empty_dirs = std::collections::BTreeMap::new();
            for (i, voice) in voices.iter().enumerate() {
                if i > 0 {
                    lines.push(format!("{pad}  \\\\"));
                }
                lines.push(format!("{pad}  {{"));
                let inner_pad = format!("{pad}    ");
                // Only attach directions to the first voice
                let dirs_for_voice = if i == 0 { &dir_at_offset } else { &empty_dirs };
                emit_voice_elements(
                    voice,
                    lang,
                    mode,
                    &mut emit_state,
                    &inner_pad,
                    dirs_for_voice,
                    divisions,
                    lines,
                );
                lines.push(format!("{pad}  }}"));
            }
            lines.push(format!("{pad}>>"));
        }

        // Right barline
        if let Some(bl) = &measure.right_barline {
            if let Some(_ending_num) = bl.ending_number {
                if bl.ending_type.as_deref() == Some("stop") {
                    // Close this alternative's block
                    lines.push(format!("{pad}  }}"));
                }
                // Check if this is the last alternative (has backward repeat)
                if bl.repeat_direction.is_some() {
                    // Close \alternative and \repeat
                    lines.push(format!("{pad}}}"));
                }
            } else if bl.repeat_direction.is_some() {
                lines.push(format!("{pad}}}"));
            } else {
                let bar_cmd = match bl.style {
                    crate::ir::direction::BarlineType::Final => Some("\\bar \"|.\""),
                    crate::ir::direction::BarlineType::Double => Some("\\bar \"||\""),
                    crate::ir::direction::BarlineType::Dashed => Some("\\bar \"!\""),
                    crate::ir::direction::BarlineType::RepeatBoth => Some("\\bar \":|.|:\""),
                    _ => None,
                };
                if let Some(cmd) = bar_cmd {
                    lines.push(format!("{pad}{cmd}"));
                }
            }
        }

        // Measure separator comment
        if measure.number > 0 {
            lines.push(format!("{pad}| % {}", measure.number));
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn emit_voice_elements(
    voice: &Voice,
    lang: PitchLanguage,
    mode: PitchMode,
    state: &mut EmitState,
    pad: &str,
    dir_at_offset: &std::collections::BTreeMap<i32, Vec<String>>,
    divisions: i64,
    lines: &mut Vec<String>,
) {
    let mut tokens: Vec<String> = Vec::new();
    let mut in_tuplet = false;
    let mut current_stem: String = String::new(); // track stem direction changes

    // Running forward position in divisions -- mirrors the value computed in
    // mxml_to_ir during parse_measure.
    let mut fwd_pos: i64 = 0;

    // Helper: collect direction strings whose offset matches `pos` and return
    // them concatenated (to append after a note token).
    let dirs_at = |pos: i64| -> String {
        if let Some(parts) = dir_at_offset.get(&(pos as i32)) {
            parts.join("")
        } else {
            String::new()
        }
    };

    for elem in &voice.elements {
        // Check for tuplet start
        if let Some(td) = element_tuplet(elem) {
            if td.tuplet_type == StartStop::Start && !in_tuplet {
                let (actual, normal) = element_tuplet_ratio(elem);
                tokens.push(format!("\\tuplet {actual}/{normal} {{"));
                in_tuplet = true;
            }
        }

        // Collect any directions that should attach at the current position.
        let dir_suffix = dirs_at(fwd_pos);

        // Emit stem direction change if needed
        let elem_stem: &str = match elem {
            VoiceElement::Note(n) => &n.stem_direction,
            VoiceElement::Chord(c) => c.notes.first().map_or("", |n| &n.stem_direction),
            _ => "",
        };
        if !elem_stem.is_empty() && elem_stem != current_stem {
            let cmd = match elem_stem {
                "up" => "\\stemUp",
                "down" => "\\stemDown",
                _ => "",
            };
            if !cmd.is_empty() {
                tokens.push(cmd.to_string());
                current_stem = elem_stem.to_string();
            }
        } else if elem_stem.is_empty() && !current_stem.is_empty() {
            tokens.push("\\stemNeutral".to_string());
            current_stem.clear();
        }

        // Emit \autoBeamOff / \autoBeamOn state changes
        if let VoiceElement::Note(note) = elem {
            if note.no_auto_beam && !state.auto_beam_off && !note.is_grace {
                tokens.push("\\autoBeamOff".to_string());
                state.auto_beam_off = true;
            } else if !note.no_auto_beam && state.auto_beam_off && !note.is_grace {
                tokens.push("\\autoBeamOn".to_string());
                state.auto_beam_off = false;
            }
        }

        match elem {
            VoiceElement::Note(note) => {
                // Glissando style override (must precede the note)
                if note.glissando == Some(StartStop::Start) {
                    if let Some(lt) = &note.glissando_line_type {
                        let style = match lt.as_str() {
                            "dashed" => Some("dashed-line"),
                            "dotted" => Some("dotted-line"),
                            "wavy" => Some("trill"),
                            _ => None,
                        };
                        if let Some(s) = style {
                            tokens.push(format!("\\once \\override Glissando.style = #'{s}"));
                        }
                    }
                }
                let mut token = note_to_ly(note, lang, mode, state.prev_pitch.as_ref());
                if !dir_suffix.is_empty() {
                    token = format!("{token}{dir_suffix}");
                }
                // Append \melisma / \melismaEnd state changes to the note token
                if !note.is_grace {
                    if note.in_melisma && !state.in_melisma {
                        token = format!("{token}\\melisma");
                        state.in_melisma = true;
                    } else if !note.in_melisma && state.in_melisma {
                        // \melismaEnd goes on the last melisma note -- patch the previous token
                        if let Some(prev_token) = tokens.last_mut() {
                            *prev_token = format!("{prev_token}\\melismaEnd");
                        }
                        state.in_melisma = false;
                    }
                }
                state.prev_pitch = Some(note.pitch);
                // Advance position for non-grace notes
                if !note.is_grace {
                    let dur_divs = duration_to_divisions(&note.duration, divisions);
                    fwd_pos += dur_divs;
                }
                tokens.push(token);
            }
            VoiceElement::Rest(rest) => {
                let mut token = rest_to_ly(rest);
                if !dir_suffix.is_empty() {
                    token = format!("{token}{dir_suffix}");
                }
                let dur_divs = duration_to_divisions(&rest.duration, divisions);
                fwd_pos += dur_divs;
                tokens.push(token);
            }
            VoiceElement::Chord(chord) => {
                // Arpeggio direction / style override (must precede the chord)
                if let Some(arp) = &chord.arpeggio {
                    match arp {
                        ArpeggioType::Up => {
                            tokens.push("\\arpeggioArrowUp".to_string());
                        }
                        ArpeggioType::Down => {
                            tokens.push("\\arpeggioArrowDown".to_string());
                        }
                        ArpeggioType::NonArpeggio => {
                            tokens.push("\\arpeggioBracket".to_string());
                        }
                    }
                }
                let (mut token, last) = chord_to_ly(chord, lang, mode, state.prev_pitch.as_ref());
                if !dir_suffix.is_empty() {
                    token = format!("{token}{dir_suffix}");
                }
                state.prev_pitch = last;
                let dur_divs = duration_to_divisions(&chord.duration, divisions);
                fwd_pos += dur_divs;
                tokens.push(token);
            }
            VoiceElement::Forward(fwd) => {
                let dur_divs = duration_to_divisions(&fwd.duration, divisions);
                fwd_pos += dur_divs;
                tokens.push(format!("s{}", duration_to_ly(&fwd.duration)));
            }
            VoiceElement::Backup(bk) => {
                let dur_divs = duration_to_divisions(&bk.duration, divisions);
                fwd_pos -= dur_divs;
                // Backups are structural; they don't emit LilyPond tokens
            }
        }

        // Check for tuplet stop
        if let Some(td) = element_tuplet(elem) {
            if td.tuplet_type == StartStop::Stop && in_tuplet {
                tokens.push("}".to_string());
                in_tuplet = false;
            }
        }
    }

    // Attach any remaining directions that didn't match a note position
    // (e.g. at the very end of the measure): append to the last token.
    for (&off, parts) in dir_at_offset.iter() {
        if (off as i64) >= fwd_pos && !parts.is_empty() {
            let suffix = parts.join("");
            if let Some(last) = tokens.last_mut() {
                *last = format!("{last}{suffix}");
            }
        }
    }

    // Safety: close any unclosed tuplet
    if in_tuplet {
        tokens.push("}".to_string());
    }

    // Group tokens into lines of ~72 chars
    if !tokens.is_empty() {
        let mut current_line: Vec<&str> = Vec::new();
        let mut current_len = 0usize;
        for token in &tokens {
            current_len += token.len() + 1;
            current_line.push(token);
            if current_len > 72 {
                lines.push(format!("{pad}{}", current_line.join(" ")));
                current_line.clear();
                current_len = 0;
            }
        }
        if !current_line.is_empty() {
            lines.push(format!("{pad}{}", current_line.join(" ")));
        }
    }
}

fn note_to_ly(note: &Note, lang: PitchLanguage, mode: PitchMode, prev: Option<&Pitch>) -> String {
    if note.is_grace {
        return grace_note_to_ly(note, lang, mode, prev);
    }

    let p = pitch_to_ly(&note.pitch, lang, prev, mode);
    let d = duration_to_ly(&note.duration);
    let trem = tremolo_suffix(note);
    let attach = attachments_to_ly(note);
    format!("{p}{d}{trem}{attach}")
}

fn grace_note_to_ly(
    note: &Note,
    lang: PitchLanguage,
    mode: PitchMode,
    prev: Option<&Pitch>,
) -> String {
    let p = pitch_to_ly(&note.pitch, lang, prev, mode);
    let d = duration_to_ly(&note.duration);
    let attach = attachments_to_ly(note);
    if note.after_grace {
        // \afterGrace <main-note> { <grace-note> }
        // The main note is emitted separately; we emit only the grace part.
        return format!("\\afterGrace {{ {p}{d}{attach} }}");
    }
    let cmd = if note.grace_slash {
        "\\acciaccatura"
    } else {
        "\\appoggiatura"
    };
    format!("{cmd} {p}{d}{attach}")
}

pub(super) fn rest_to_ly(rest: &Rest) -> String {
    let d = duration_to_ly(&rest.duration);
    if rest.is_measure_rest {
        let mut s = format!("R{d}");
        if rest.fermata.is_some() {
            s.push_str("\\fermata");
        }
        s
    } else if rest.is_spacer {
        format!("s{d}")
    } else {
        let mut s = format!("r{d}");
        if rest.fermata.is_some() {
            s.push_str("\\fermata");
        }
        s
    }
}

pub(super) fn chord_to_ly(
    chord: &Chord,
    lang: PitchLanguage,
    mode: PitchMode,
    prev: Option<&Pitch>,
) -> (String, Option<Pitch>) {
    if chord.notes.is_empty() {
        return (format!("r{}", duration_to_ly(&chord.duration)), None);
    }

    let mut pitch_strs: Vec<String> = Vec::new();
    let mut last_pitch = prev.cloned();
    for n in &chord.notes {
        let p = pitch_to_ly(&n.pitch, lang, last_pitch.as_ref(), mode);
        pitch_strs.push(p);
        last_pitch = Some(n.pitch);
    }

    let d = duration_to_ly(&chord.duration);
    let trem = tremolo_suffix(&chord.notes[0]);
    let attach = attachments_to_ly(&chord.notes[0]);
    let arp = if chord.arpeggio.is_some() {
        "\\arpeggio"
    } else {
        ""
    };
    let result = format!("<{}>{d}{trem}{attach}{arp}", pitch_strs.join(" "));
    (result, last_pitch)
}

pub(super) fn attachments_to_ly(note: &Note) -> String {
    let mut parts: Vec<&str> = Vec::new();
    let mut owned: Vec<String> = Vec::new();

    // Beam brackets (must come immediately after pitch+duration)
    // Look at level-1 beam only; `[` for begin, `]` for end
    for beam in &note.beams {
        if beam.number == 1 {
            match beam.beam_type.as_str() {
                "begin" => parts.push("["),
                "end" => parts.push("]"),
                _ => {}
            }
        }
    }

    // Ties
    for tie in &note.ties {
        if tie.tie_type == StartStop::Start {
            parts.push("~");
        }
    }

    // Slurs
    for slur in &note.slurs {
        match slur.slur_type {
            StartStop::Start => parts.push("("),
            StartStop::Stop => parts.push(")"),
            _ => {}
        }
    }

    // Articulations
    for art in &note.articulations {
        let ly = articulation_to_ly(&art.name);
        if !ly.is_empty() {
            parts.push(ly);
        }
    }

    // Fermata
    if note.fermata.is_some() {
        parts.push("\\fermata");
    }

    // Ornaments
    for orn in &note.ornaments {
        // Tremolo is handled by tremolo_suffix(), not as an attachment
        if orn.name == "tremolo" {
            continue;
        }
        let ly = ornament_to_ly(&orn.name);
        if !ly.is_empty() {
            parts.push(ly);
        }
    }

    // Dynamics (note-attached)
    for dyn_mark in &note.dynamics {
        owned.push(format!("\\{}", dyn_mark.sign));
    }

    // Wedges (note-attached)
    for wedge in &note.wedges {
        let cmd = match wedge.wedge_type.as_str() {
            "crescendo" => "\\<",
            "diminuendo" => "\\>",
            "stop" => "\\!",
            _ => "",
        };
        if !cmd.is_empty() {
            parts.push(cmd);
        }
    }

    // Glissando (the style override is emitted as a prefix in emit_voice_elements)
    if note.glissando == Some(StartStop::Start) || note.slide == Some(StartStop::Start) {
        parts.push("\\glissando");
    }

    // Technicals (fingering, bow marks, etc.)
    for tech in &note.technicals {
        match tech.name.as_str() {
            "fingering" => {
                owned.push(format!("-{}", tech.value));
            }
            "up-bow" => parts.push("\\upbow"),
            "down-bow" => parts.push("\\downbow"),
            "open-string" => parts.push("\\open"),
            "snap-pizzicato" => parts.push("\\snappizzicato"),
            "harmonic" => parts.push("\\flageolet"),
            "stopped" => parts.push("-+"),
            "string" => {
                owned.push(format!("\\{}", tech.value));
            }
            _ => {}
        }
    }

    let mut result: String = parts.join("");
    for o in &owned {
        result.push_str(o);
    }
    result
}
