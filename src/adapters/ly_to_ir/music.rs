use tree_sitter::Node;

use crate::ir::articulation::{Placement, SlurEvent, StartStop, TieEvent};
use crate::ir::direction::{
    Barline, BarlineType, Direction, LayoutBreakType, OctaveShift, PedalEvent,
};
use crate::ir::duration::Frac;
use crate::ir::language::parse_pitch_name;
use crate::ir::language::PitchMode;
use crate::ir::measure::{Clef, KeyMode, KeySignature, MeasureAttributes, TimeSignature};
use crate::ir::note::{ArpeggioType, Note, Rest, VoiceElement};

use super::apply::{
    apply_chord_attachments, apply_note_attachments, apply_rest_attachments, attach_articulation,
    attach_dynamic, attach_fermata,
};
use super::consume::{
    build_chord, consume_accidental_marks, consume_attachments, consume_duration,
    consume_duration_scale, consume_mark, consume_octave_marks, consume_override, consume_tempo,
    consume_tremolo, extract_scheme_string, extract_string_value, is_dynamic_name, parse_fraction,
    parse_grace_block, parse_ly_make_moment, parse_paper_block, punct_text,
};
use super::merge::apply_tuplet_display;
use super::modifiers::{consume_relative, consume_repeat, consume_transpose};
use super::state::WalkState;

/// If attachments contain `\rest`, convert the note to a pitched rest
/// (display-step + display-octave) and return it as a VoiceElement::Rest.
/// Otherwise return the note as VoiceElement::Note.
fn note_or_pitched_rest(note: Note, attachments: &[String]) -> VoiceElement {
    if attachments.iter().any(|a| a == "\\rest") {
        let mut rest = Rest::new(note.duration.clone());
        rest.display_step = Some(format!("{:?}", note.pitch.step));
        rest.display_octave = Some(note.pitch.octave);
        rest.voice = note.voice;
        rest.staff = note.staff;
        // Copy any dynamics/wedges from note
        rest.dynamics = note.dynamics.clone();
        rest.wedges = note.wedges.clone();
        VoiceElement::Rest(rest)
    } else {
        VoiceElement::Note(Box::new(note))
    }
}
use super::walk::{extract_named_context, walk_context_body, walk_parallel_music};
use super::{parse_clef_name, parse_key_mode, pitch_to_fifths};

/// Walk an `expression_block` `{ ... }` containing music.
pub(super) fn walk_music_block(state: &mut WalkState, block: Node) {
    let mut cursor = block.walk();
    let children: Vec<Node> = block.children(&mut cursor).collect();
    let mut i = 0;

    while i < children.len() {
        let node = children[i];
        match node.kind() {
            "symbol" => {
                let sym = state.text(node).to_string();
                i = handle_symbol(state, &children, i, &sym);
                continue;
            }
            "escaped_word" => {
                let text = state.text(node).to_string();
                i = handle_escaped_word(state, &children, i, &text);
                continue;
            }
            "dynamic" => {
                // Attach dynamic to most recent note
                let dyn_text = state.text(node).to_string();
                attach_dynamic(state, &dyn_text);
            }
            "chord" => {
                // < ... >
                let chord_node = node;
                i += 1;
                // Consume duration after chord
                let mut dur = consume_duration(state, &children, &mut i);
                // Apply *N/M duration scaling (factor carries forward, as
                // for notes)
                if let Some(scale) = consume_duration_scale(state, &children, &mut i) {
                    dur.base *= scale;
                    state.last_duration = dur.clone();
                }
                let attachments = consume_attachments(state, &children, &mut i);
                let chord = build_chord(state, chord_node, dur);
                let mut chord = chord;
                apply_chord_attachments(state, &mut chord, &attachments);
                state.push_voice_element(VoiceElement::Chord(chord));
                continue;
            }
            "punctuation" => {
                let punc = punct_text(state, node);
                handle_punctuation(state, &punc);
            }
            "expression_block" => {
                // Nested block: could be a voice or sub-expression
                walk_music_block(state, node);
            }
            "parallel_music" => {
                walk_parallel_music(state, node);
            }
            "named_context" => {
                let (context, name) = extract_named_context(state, node);
                i += 1;
                i = walk_context_body(state, &children, i, &context, &name);
                continue;
            }
            "fraction" => {
                // Standalone fraction (shouldn't appear without \time, but handle gracefully)
            }
            "{" | "}" | "<<" | ">>" | "comment" | "unsigned_integer" | "string" => {
                // Skip structural tokens and standalone numbers
            }
            _ => {}
        }
        i += 1;
    }
}

/// Handle a symbol node (pitch name, r, R, s, etc.).
/// Returns the next index to process.
fn handle_symbol(state: &mut WalkState, children: &[Node], i: usize, sym: &str) -> usize {
    let mut i = i + 1;

    match sym {
        "r" => {
            // Regular rest
            let dur = consume_duration(state, children, &mut i);
            let attachments = consume_attachments(state, children, &mut i);
            let mut rest = Rest::new(dur);
            apply_rest_attachments(&mut rest, &attachments);
            state.push_voice_element(VoiceElement::Rest(rest));
        }
        "R" => {
            // Whole-measure rest, possibly with *N or *N/M or *N/M*K multiplier
            let mut dur = consume_duration(state, children, &mut i);
            // Check for *N or *N/M multiplier
            let scale = consume_duration_scale(state, children, &mut i);
            // Check for second *K integer multiplier after fractional scale
            let repeat_count = if matches!(&scale, Some(f) if *f.denom() != 1) {
                consume_duration_scale(state, children, &mut i)
                    .filter(|f| *f.denom() == 1)
                    .map(|f| *f.numer() as u32)
                    .unwrap_or(1)
            } else {
                1
            };
            let attachments = consume_attachments(state, children, &mut i);
            match scale {
                Some(frac) if *frac.denom() != 1 => {
                    // Fractional multiplier (e.g. R1*3/4): scale the duration
                    dur.base *= frac;
                    let mut rest = Rest::measure_rest(dur.clone());
                    apply_rest_attachments(&mut rest, &attachments);
                    state.push_voice_element(VoiceElement::Rest(rest));
                    for _ in 1..repeat_count {
                        state.bar_check();
                        let rest = Rest::measure_rest(dur.clone());
                        state.push_voice_element(VoiceElement::Rest(rest));
                    }
                }
                Some(frac) => {
                    // Integer multiplier (e.g. R1*3): expand into N measure rests
                    let count = *frac.numer() as u32;
                    let mut rest = Rest::measure_rest(dur.clone());
                    apply_rest_attachments(&mut rest, &attachments);
                    state.push_voice_element(VoiceElement::Rest(rest));
                    for _ in 1..count {
                        state.bar_check();
                        let rest = Rest::measure_rest(dur.clone());
                        state.push_voice_element(VoiceElement::Rest(rest));
                    }
                }
                None => {
                    let mut rest = Rest::measure_rest(dur);
                    apply_rest_attachments(&mut rest, &attachments);
                    state.push_voice_element(VoiceElement::Rest(rest));
                }
            }
        }
        "s" => {
            // Spacer rest, possibly with *N or *N/M multiplier.
            // *N (integer): push N spacer rests of the base duration,
            //   letting auto-flush handle measure boundaries.
            // *N/M (fraction): scale the duration (e.g. s16*2/3 = 1/24).
            // *N/M*K (fraction + integer): scale duration then repeat K times
            //   (e.g. s1*3/4*3 = 3 spacer rests of 3/4 each).
            let mut dur = consume_duration(state, children, &mut i);
            let scale = consume_duration_scale(state, children, &mut i);
            // Check for a second *N integer multiplier after a fractional scale
            let repeat_count = if matches!(&scale, Some(f) if *f.denom() != 1) {
                consume_duration_scale(state, children, &mut i)
                    .filter(|f| *f.denom() == 1)
                    .map(|f| *f.numer() as u32)
                    .unwrap_or(1)
            } else {
                1
            };
            let attachments = consume_attachments(state, children, &mut i);
            match scale {
                Some(frac) if *frac.denom() != 1 => {
                    // Fractional multiplier: scale the duration
                    dur.base *= frac;
                    for _ in 0..repeat_count {
                        let mut rest = Rest::new(dur.clone());
                        rest.is_spacer = true;
                        apply_rest_attachments(&mut rest, &attachments);
                        state.push_voice_element(VoiceElement::Rest(rest));
                        if repeat_count > 1 {
                            state.bar_check();
                        }
                    }
                }
                Some(frac) => {
                    // Integer multiplier: push N spacer rests
                    let count = *frac.numer() as u32;
                    for _ in 0..count {
                        let mut rest = Rest::new(dur.clone());
                        rest.is_spacer = true;
                        state.push_voice_element(VoiceElement::Rest(rest));
                    }
                }
                None => {
                    let mut rest = Rest::new(dur);
                    rest.is_spacer = true;
                    state.push_voice_element(VoiceElement::Rest(rest));
                }
            }
            // Apply dynamic attachments to the spacer rest that was just pushed.
            // attach_dynamic / attach_articulation look at current_voice.last_mut()
            // which is the spacer rest.
            for att in &attachments {
                if is_dynamic_name(att)
                    || matches!(
                        att.as_str(),
                        "\\<" | "\\>" | "\\!" | "\\crescendo" | "\\decrescendo" | "\\dim"
                    )
                {
                    attach_dynamic(state, att);
                } else if att == "\\fermata" {
                    if let Some(VoiceElement::Rest(r)) = state.current_voice.last_mut() {
                        r.fermata = Some(crate::ir::articulation::Fermata {
                            shape: "normal".to_string(),
                            inverted: false,
                        });
                    }
                }
            }
        }
        _ => {
            // Try as pitch name
            if let Some((step, alter)) = parse_pitch_name(sym, state.language) {
                // Consume octave marks
                let octave_marks = consume_octave_marks(state, children, &mut i);
                // Consume accidental forcing marks (! = forced, ? = cautionary)
                let acc_display = consume_accidental_marks(state, children, &mut i);
                let mut dur = consume_duration(state, children, &mut i);
                // Apply *N/M duration scaling (e.g. a32*8/7). LilyPond
                // remembers the factor as part of the duration, so it must
                // carry forward to following durationless notes.
                if let Some(scale) = consume_duration_scale(state, children, &mut i) {
                    dur.base *= scale;
                    state.last_duration = dur.clone();
                }
                let tremolo = consume_tremolo(state, children, &mut i, &dur);
                let attachments = consume_attachments(state, children, &mut i);

                let mut pitch = state.resolve_pitch(step, alter, octave_marks);
                pitch.accidental = acc_display;
                let mut note = Note::new(pitch, dur);
                if tremolo > 0 {
                    note.tremolo_marks = tremolo;
                    note.ornaments.push(crate::ir::articulation::Ornament {
                        name: "tremolo".to_string(),
                        placement: Default::default(),
                    });
                }
                apply_note_attachments(state, &mut note, &attachments);
                state.push_voice_element(note_or_pitched_rest(note, &attachments));
            }
            // If not a pitch name, ignore (could be a context name etc.)
        }
    }
    i
}

/// Handle an escaped_word node (\key, \time, \clef, \grace, etc.).
/// Returns the next index.
fn handle_escaped_word(state: &mut WalkState, children: &[Node], i: usize, text: &str) -> usize {
    let mut i = i + 1;

    match text {
        "\\key" => {
            // \key <pitch> \<mode>
            if let Some(pitch_node) = children.get(i) {
                if pitch_node.kind() == "symbol" {
                    let sym = state.text(*pitch_node);
                    if let Some((step, alter)) = parse_pitch_name(sym, state.language) {
                        i += 1;
                        // Next: \major, \minor, etc.
                        let mode = if let Some(mode_node) = children.get(i) {
                            if mode_node.kind() == "escaped_word" {
                                let m = state.text(*mode_node);
                                i += 1;
                                parse_key_mode(m)
                            } else {
                                KeyMode::Major
                            }
                        } else {
                            KeyMode::Major
                        };
                        let fifths = pitch_to_fifths(step, alter, mode) as i8;
                        let ks = KeySignature { fifths, mode };
                        let measure = state.ensure_measure();
                        if measure.attributes.is_none() {
                            measure.attributes = Some(MeasureAttributes::default());
                        }
                        measure.attributes.as_mut().unwrap().key = Some(ks);
                    }
                }
            }
        }
        "\\time" => {
            // \time <fraction>, or compound \time 3+2/8 which the grammar
            // splits into leading `<uint> +` pairs before the final fraction.
            let mut extra_beats: Vec<u32> = Vec::new();
            while i + 1 < children.len()
                && children[i].kind() == "unsigned_integer"
                && children[i + 1].kind() == "punctuation"
                && punct_text(state, children[i + 1]) == "+"
            {
                if let Ok(n) = state.text(children[i]).parse::<u32>() {
                    extra_beats.push(n);
                }
                i += 2;
            }
            if let Some(frac_node) = children.get(i) {
                if frac_node.kind() == "fraction" {
                    let frac_text = state.text(*frac_node);
                    if let Some((num, den)) = parse_fraction(frac_text) {
                        // If current voice or measure already has notes/rests,
                        // flush the measure first so \time starts a new bar
                        if state.elapsed_in_measure > Frac::from_integer(0) {
                            state.bar_check();
                        }
                        let beats = if extra_beats.is_empty() {
                            num.to_string()
                        } else {
                            extra_beats
                                .iter()
                                .chain(std::iter::once(&num))
                                .map(|b| b.to_string())
                                .collect::<Vec<_>>()
                                .join("+")
                        };
                        let total: u32 = extra_beats.iter().sum::<u32>() + num;
                        let ts = TimeSignature {
                            beats,
                            beat_type: den as u8,
                            symbol: None,
                        };
                        state.set_time_signature(total, den);
                        let measure = state.ensure_measure();
                        if measure.attributes.is_none() {
                            measure.attributes = Some(MeasureAttributes::default());
                        }
                        measure.attributes.as_mut().unwrap().time = Some(ts);
                    }
                    i += 1;
                }
            }
        }
        "\\clef" => {
            // \clef <symbol> or \clef "<string>"
            if let Some(clef_node) = children.get(i) {
                let clef_name = if clef_node.kind() == "symbol" {
                    let name = state.text(*clef_node).to_string();
                    i += 1;
                    name
                } else if clef_node.kind() == "string" {
                    let name = extract_string_value(state, *clef_node);
                    i += 1;
                    name
                } else {
                    String::new()
                };
                if !clef_name.is_empty() {
                    if let Some((sign, line, oct_change)) = parse_clef_name(&clef_name) {
                        let clef = Clef {
                            sign,
                            line,
                            octave_change: oct_change,
                        };
                        let measure = state.ensure_measure();
                        if measure.attributes.is_none() {
                            measure.attributes = Some(MeasureAttributes::default());
                        }
                        measure.attributes.as_mut().unwrap().clefs.insert(1, clef);
                    }
                }
            }
        }
        "\\tempo" => {
            // \tempo "text" dur = bpm  OR  \tempo dur = bpm  OR  \tempo "text"
            i = consume_tempo(state, children, i);
        }
        "\\grace" | "\\acciaccatura" | "\\appoggiatura" => {
            // \grace { notes }  OR  \grace note (single unbraced note)
            let is_slash = text == "\\acciaccatura";
            if let Some(next_node) = children.get(i) {
                if next_node.kind() == "expression_block" {
                    // Parse grace notes from the block
                    let grace_notes = parse_grace_block(state, *next_node);
                    for mut note in grace_notes {
                        note.is_grace = true;
                        note.grace_slash = is_slash;
                        state.push_voice_element(VoiceElement::Note(Box::new(note)));
                    }
                    i += 1;
                } else if next_node.kind() == "symbol" {
                    // Single unbraced grace note, e.g. \acciaccatura d''8
                    let sym = state.text(*next_node);
                    if let Some((step, alter)) = parse_pitch_name(sym, state.language) {
                        i += 1;
                        let octave_marks = consume_octave_marks(state, children, &mut i);
                        let dur = consume_duration(state, children, &mut i);
                        let attachments = consume_attachments(state, children, &mut i);
                        let pitch = state.resolve_pitch(step, alter, octave_marks);
                        let mut note = Note::new(pitch, dur);
                        apply_note_attachments(state, &mut note, &attachments);
                        note.is_grace = true;
                        note.grace_slash = is_slash;
                        state.push_voice_element(VoiceElement::Note(Box::new(note)));
                    }
                }
            }
        }
        "\\tuplet" | "\\times" => {
            // \tuplet actual/normal { notes }  OR  \times normal/actual { notes }
            if let Some(frac_node) = children.get(i) {
                if frac_node.kind() == "fraction" {
                    let frac_text = state.text(*frac_node);
                    if let Some((num, denom)) = frac_text.split_once('/') {
                        let n: u8 = num.parse().unwrap_or(1);
                        let d: u8 = denom.parse().unwrap_or(1);
                        let (actual, normal) = if text == "\\tuplet" {
                            (n, d)
                        } else {
                            (d, n) // \times has reversed fraction
                        };
                        i += 1;
                        if let Some(block) = children.get(i) {
                            if block.kind() == "expression_block" {
                                // Push tuplet ratio so notes created inside get
                                // the scaling applied immediately (for correct
                                // measure duration tracking).
                                state.tuplet_stack.push((actual, normal));
                                let before = state.current_voice.len();
                                walk_music_block(state, *block);
                                let after = state.current_voice.len();
                                state.tuplet_stack.pop();
                                // Apply tuplet display markers (start/stop brackets)
                                if after > before {
                                    apply_tuplet_display(
                                        &mut state.current_voice[before..after],
                                        actual,
                                    );
                                }
                                i += 1;
                            }
                        }
                    }
                }
            }
        }
        "\\relative" => {
            state.in_relative = true;
            state.mode = PitchMode::Relative;
            i = consume_relative(state, children, i);
        }
        "\\transpose" => {
            i = consume_transpose(state, children, i);
        }
        "\\fermata" => {
            // Attach fermata to most recent note/rest
            attach_fermata(state);
        }
        "\\breathe" => {
            // Attach breath mark as articulation to last note
            attach_articulation(state, "breath-mark");
        }
        "\\trill" => attach_articulation(state, "trill-mark"),
        "\\mordent" => attach_articulation(state, "mordent"),
        "\\prall" => attach_articulation(state, "inverted-mordent"),
        "\\turn" => attach_articulation(state, "turn"),
        "\\reverseturn" => attach_articulation(state, "inverted-turn"),
        "\\sustainOn" | "\\sustainOff" => {
            let pedal_type = if text == "\\sustainOn" {
                "start"
            } else {
                "stop"
            };
            // Pedal commands attach to the note they follow and occur at that
            // note's onset (LilyPond post-event semantics), not after its
            // duration has elapsed.
            let offset_frac = state.last_element_onset;
            let dir = Direction {
                pedal: Some(PedalEvent {
                    pedal_type: pedal_type.to_string(),
                    line: false,
                }),
                placement: Placement::Below,
                offset_frac,
                ..Default::default()
            };
            let measure = state.ensure_measure();
            measure.directions.push(dir);
        }
        "\\ottava" => {
            // \ottava #1, \ottava #-1, \ottava #0
            if let Some(scheme_node) = children.get(i) {
                if scheme_node.kind() == "embedded_scheme" {
                    let scheme_text = state.text(*scheme_node);
                    // Parse #N or #-N from the embedded scheme
                    let num_str = scheme_text.trim_start_matches('#');
                    if let Ok(n) = num_str.parse::<i32>() {
                        let (shift_type, size) = if n > 0 {
                            ("up", (n * 8) as i8)
                        } else if n < 0 {
                            ("down", (n.abs() * 8) as i8)
                        } else {
                            ("stop", 0i8)
                        };
                        let dir = Direction {
                            octave_shift: Some(OctaveShift {
                                shift_type: shift_type.to_string(),
                                size,
                            }),
                            ..Default::default()
                        };
                        let measure = state.ensure_measure();
                        measure.directions.push(dir);
                    }
                    i += 1;
                }
            }
        }
        "\\break" => {
            let dir = Direction {
                layout_break: Some(LayoutBreakType::System),
                ..Default::default()
            };
            // If no pending content, attach to last existing measure
            if state.current_measure.is_none() && state.current_voice.is_empty() {
                let part = state.ensure_part();
                if let Some(last) = part.measures.last_mut() {
                    last.directions.push(dir);
                }
            } else {
                let measure = state.ensure_measure();
                measure.directions.push(dir);
            }
        }
        "\\pageBreak" => {
            let dir = Direction {
                layout_break: Some(LayoutBreakType::Page),
                ..Default::default()
            };
            if state.current_measure.is_none() && state.current_voice.is_empty() {
                let part = state.ensure_part();
                if let Some(last) = part.measures.last_mut() {
                    last.directions.push(dir);
                }
            } else {
                let measure = state.ensure_measure();
                measure.directions.push(dir);
            }
        }
        "\\stemUp" => {
            state.stem_direction = "up".to_string();
        }
        "\\stemDown" => {
            state.stem_direction = "down".to_string();
        }
        "\\stemNeutral" => {
            state.stem_direction.clear();
        }
        "\\voiceOne" => {
            state.current_voice_number = 1;
            state.stem_direction = "up".to_string();
        }
        "\\voiceTwo" => {
            state.current_voice_number = 2;
            state.stem_direction = "down".to_string();
        }
        "\\voiceThree" => {
            state.current_voice_number = 3;
            state.stem_direction = "up".to_string();
        }
        "\\voiceFour" => {
            state.current_voice_number = 4;
            state.stem_direction = "down".to_string();
        }
        "\\oneVoice" => {
            state.current_voice_number = 1;
            state.stem_direction.clear();
        }
        "\\repeat" => {
            i = consume_repeat(state, children, i);
        }
        "\\bar" => {
            // \bar "||" or \bar "|."
            if let Some(bar_node) = children.get(i) {
                if bar_node.kind() == "string" {
                    let bar_text = extract_string_value(state, *bar_node);
                    let bar_type = match bar_text.as_str() {
                        "|." => BarlineType::Final,
                        "||" => BarlineType::Double,
                        "!" => BarlineType::Dashed,
                        ":|.|:" => BarlineType::RepeatBoth,
                        ":|." | ":|" => BarlineType::RepeatBackward,
                        "|:" | ".|:" => BarlineType::RepeatForward,
                        _ => BarlineType::Regular,
                    };
                    let barline = Barline {
                        style: bar_type,
                        ..Default::default()
                    };
                    i += 1;
                    // If no pending content (measure was just flushed by a preceding |),
                    // attach the barline to the last existing measure rather than creating
                    // a new empty measure. This prevents extra empty measures in variables
                    // like `playSilent` that use `\barRest | \bar "||" \break`.
                    if state.current_measure.is_none() && state.current_voice.is_empty() {
                        let part = state.ensure_part();
                        if let Some(last) = part.measures.last_mut() {
                            if last.right_barline.is_none() {
                                last.right_barline = Some(barline);
                            }
                        }
                    } else {
                        let measure = state.ensure_measure();
                        measure.right_barline = Some(barline);
                        // \bar acts as a measure boundary
                        state.bar_check();
                    }
                }
            }
        }
        "\\new" => {
            // Standalone \new (already handled in walk_score_block but may appear in blocks)
            if let Some(next) = children.get(i) {
                if next.kind() == "symbol" {
                    let context = state.text(*next).to_string();
                    i += 1;
                    i = walk_context_body(state, children, i, &context, "");
                }
            }
        }
        "\\layout" | "\\midi" => {
            // Skip these blocks
            if let Some(next) = children.get(i) {
                if next.kind() == "expression_block" {
                    i += 1;
                }
            }
        }
        "\\partial" => {
            // \partial <dur>  → anacrusis / pickup
            let dur = consume_duration(state, children, &mut i);
            state.metadata.partial_duration = Some(dur);
        }
        "\\afterGrace" => {
            // \afterGrace { notes }  OR  \afterGrace note
            if let Some(next_node) = children.get(i) {
                if next_node.kind() == "expression_block" {
                    let grace_notes = parse_grace_block(state, *next_node);
                    for mut note in grace_notes {
                        note.is_grace = true;
                        note.after_grace = true;
                        state.push_voice_element(VoiceElement::Note(Box::new(note)));
                    }
                    i += 1;
                } else if next_node.kind() == "symbol" {
                    let sym = state.text(*next_node);
                    if let Some((step, alter)) = parse_pitch_name(sym, state.language) {
                        i += 1;
                        let octave_marks = consume_octave_marks(state, children, &mut i);
                        let dur = consume_duration(state, children, &mut i);
                        let attachments = consume_attachments(state, children, &mut i);
                        let pitch = state.resolve_pitch(step, alter, octave_marks);
                        let mut note = Note::new(pitch, dur);
                        apply_note_attachments(state, &mut note, &attachments);
                        note.is_grace = true;
                        note.after_grace = true;
                        state.push_voice_element(VoiceElement::Note(Box::new(note)));
                    }
                }
            }
        }
        "\\arpeggioArrowUp" => {
            state.pending_arpeggio_type = Some(ArpeggioType::Up);
        }
        "\\arpeggioArrowDown" => {
            state.pending_arpeggio_type = Some(ArpeggioType::Down);
        }
        "\\arpeggioBracket" => {
            state.pending_arpeggio_type = Some(ArpeggioType::NonArpeggio);
        }
        "\\arpeggioNormal" => {
            state.pending_arpeggio_type = None;
        }
        "\\mark" => {
            // \mark \markup { \musicglyph "scripts.coda" }
            // \mark "D.C."  /  \mark "D.S. al Coda"
            i = consume_mark(state, children, i);
        }
        "\\once" => {
            // \once — usually followed by \override; just skip it
            // The \override handler will consume the property setting
        }
        "\\override" => {
            // \override Glissando.style = #'<style>
            i = consume_override(state, children, i);
        }
        "\\paper" => {
            // \paper { ... } — page layout
            if let Some(next) = children.get(i) {
                if next.kind() == "expression_block" {
                    parse_paper_block(state, *next);
                    i += 1;
                }
            }
        }
        "\\set" => {
            // \set Staff.instrumentName = "value"
            // Tree-sitter: assignment_lhs(property_expression(symbol, ".", symbol)) "=" string
            if let Some(lhs) = children.get(i) {
                if lhs.kind() == "assignment_lhs" {
                    let prop_text = state.text(*lhs).to_string();
                    i += 1;
                    // Skip "="
                    if let Some(eq) = children.get(i) {
                        if eq.kind() == "punctuation" && state.text(*eq) == "=" {
                            i += 1;
                        }
                    }
                    // Read value (string or scheme)
                    if let Some(val_node) = children.get(i) {
                        if val_node.kind() == "string" {
                            let val = extract_string_value(state, *val_node);
                            i += 1;
                            apply_set_property(state, &prop_text, &val);
                        } else if val_node.kind() == "embedded_scheme" {
                            // Handle \set Score.measureLength = #(ly:make-moment N D)
                            // Handle \set Staff.midiInstrument = #"flute"
                            let scheme_text = state.text(*val_node);
                            if prop_text.contains("measureLength") {
                                if let Some((num, den)) = parse_ly_make_moment(scheme_text) {
                                    state.set_time_signature(num, den);
                                }
                            } else if let Some(s) = extract_scheme_string(scheme_text) {
                                apply_set_property(state, &prop_text, &s);
                            }
                            i += 1;
                        } else {
                            // Skip unknown value types (scheme booleans, etc.)
                            i += 1;
                        }
                    }
                }
            }
        }
        "\\skip" => {
            // \skip <duration> — equivalent to spacer rest (s<duration>)
            let mut dur = consume_duration(state, children, &mut i);
            let scale = consume_duration_scale(state, children, &mut i);
            match scale {
                Some(frac) if *frac.denom() != 1 => {
                    dur.base *= frac;
                    let mut rest = Rest::new(dur);
                    rest.is_spacer = true;
                    state.push_voice_element(VoiceElement::Rest(rest));
                }
                Some(frac) => {
                    let count = *frac.numer() as u32;
                    for _ in 0..count {
                        let mut rest = Rest::new(dur.clone());
                        rest.is_spacer = true;
                        state.push_voice_element(VoiceElement::Rest(rest));
                    }
                }
                None => {
                    let mut rest = Rest::new(dur);
                    rest.is_spacer = true;
                    state.push_voice_element(VoiceElement::Rest(rest));
                }
            }
        }
        "\\autoBeamOff" => {
            state.auto_beam_off = true;
        }
        "\\autoBeamOn" => {
            state.auto_beam_off = false;
        }
        "\\melisma" => {
            state.melisma_active = true;
        }
        "\\melismaEnd" => {
            state.melisma_active = false;
        }
        "\\unset" | "\\cadenzaOn" | "\\cadenzaOff" | "\\dynamicUp" | "\\dynamicDown"
        | "\\dynamicNeutral" | "\\context" | "\\unfoldRepeats" => {
            // Skip these commands; some may consume the next token
            // \context within music blocks is handled by named_context at the
            // walk_music_block level, but if tree-sitter doesn't wrap it as
            // named_context, skip it here.
        }
        _ => {
            // Unknown escaped word — may be a variable reference or dynamic
            let var_name = text.trim_start_matches('\\');
            if !state.resolve_variable(var_name) && is_dynamic_name(text) {
                attach_dynamic(state, text);
            }
        }
    }
    i
}

/// Apply a `\set Context.property = "value"` command to the current part.
pub(super) fn apply_set_property(state: &mut WalkState, property: &str, value: &str) {
    // property is like "Staff.instrumentName" or "Staff.midiInstrument"
    let prop_name = property.split('.').next_back().unwrap_or(property);
    match prop_name {
        "instrumentName" => {
            let part = state.ensure_part();
            part.name = value.to_string();
        }
        "shortInstrumentName" => {
            let part = state.ensure_part();
            part.abbreviation = value.to_string();
        }
        "midiInstrument" => {
            let part = state.ensure_part();
            part.midi_instrument = value.to_string();
        }
        _ => {} // Ignore other properties
    }
}

/// Handle punctuation tokens: |, (, ), ~, ', ,
pub(super) fn handle_punctuation(state: &mut WalkState, punc: &str) {
    match punc {
        "|" => {
            state.bar_check();
        }
        "(" => {
            // Slur start: attach to most recent note or chord
            let target = match state.current_voice.last_mut() {
                Some(VoiceElement::Note(note)) => Some(note.as_mut()),
                Some(VoiceElement::Chord(chord)) => chord.notes.first_mut(),
                _ => None,
            };
            if let Some(note) = target {
                note.slurs.push(SlurEvent {
                    slur_type: StartStop::Start,
                    number: 1,
                    placement: Placement::Unspecified,
                });
            }
        }
        ")" => {
            // Slur stop: attach to most recent note or chord
            let target = match state.current_voice.last_mut() {
                Some(VoiceElement::Note(note)) => Some(note.as_mut()),
                Some(VoiceElement::Chord(chord)) => chord.notes.first_mut(),
                _ => None,
            };
            if let Some(note) = target {
                note.slurs.push(SlurEvent {
                    slur_type: StartStop::Stop,
                    number: 1,
                    placement: Placement::Unspecified,
                });
            }
        }
        "~" => {
            // Tie: attach to most recent note or chord
            let target = match state.current_voice.last_mut() {
                Some(VoiceElement::Note(note)) => Some(note.as_mut()),
                Some(VoiceElement::Chord(chord)) => chord.notes.first_mut(),
                _ => None,
            };
            if let Some(note) = target {
                note.ties.push(TieEvent {
                    tie_type: StartStop::Start,
                });
            }
        }
        _ => {}
    }
}
