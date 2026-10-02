use tree_sitter::Node;

use crate::ir::articulation::{Placement, SlurEvent, StartStop, TieEvent};
use crate::ir::direction::{
    Barline, BarlineType, Direction, LayoutBreakType, OctaveShift, PedalEvent, RepeatDirection,
};
use crate::ir::duration::Frac;
use crate::ir::language::parse_pitch_name;
use crate::ir::language::PitchMode;
use crate::ir::measure::{Clef, KeyMode, KeySignature, TimeSignature, Transpose};
use crate::ir::note::{ArpeggioType, Chord, Note, Rest, VoiceElement};
use crate::ir::pitch::Pitch;

use super::apply::{
    apply_chord_attachments, apply_note_attachments, apply_rest_attachments, attach_articulation,
    attach_dynamic, attach_fermata,
};
use super::consume::{
    build_chord, consume_accidental_marks, consume_attachments, consume_duration,
    consume_duration_scale, consume_mark, consume_octave_marks, consume_override, consume_tempo,
    consume_tremolo, extract_scheme_string, extract_string_value, grace_one, is_dynamic_name,
    mark_text, parse_fraction, parse_grace_block, parse_ly_make_moment, parse_paper_block,
    previous, punct_text, skip_markup,
};
use super::merge::apply_tuplet_display;
use super::modifiers::{consume_relative, consume_repeat, consume_transpose};
use super::state::WalkState;
use crate::ir::timeline::Event;

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

/// Parse `#(skip-of-length VAR)` and return `VAR`. Tolerant of whitespace.
fn parse_skip_of_length(scheme_text: &str) -> Option<&str> {
    let inner = scheme_text
        .trim()
        .strip_prefix('#')?
        .trim_start()
        .strip_prefix('(')?
        .strip_suffix(')')?
        .trim();
    let var = inner.strip_prefix("skip-of-length")?.trim();
    (!var.is_empty()).then_some(var)
}

/// Walk an `expression_block` `{ ... }` containing music, in the input mode
/// the command before it sets: `\drummode` reads drum names, `\drums` is a
/// drum staff of them; `\chords` and `\figures` (`\figuremode`) are chord
/// names and figured bass, not notes; lyrics are not notes either.
pub(super) fn walk_music_block(state: &mut WalkState, block: Node) {
    // Past a bound, re-walks (unfolds, variables) end here.
    if state.stopped() || !state.enter_block() {
        return;
    }
    let mut prev = block.prev_sibling();
    while prev.is_some_and(|p| p.kind() == "comment") {
        prev = prev.and_then(|p| p.prev_sibling());
    }
    let mode = prev
        .filter(|p| p.kind() == "escaped_word")
        .map(|p| state.text(p));
    if let Some(octaves) = fixed_octaves(state, block) {
        // `\fixed c' { … }`: absolute pitches, an octave up per mark, even
        // inside `\relative`.
        let saved = (state.in_relative, state.relative_depth, state.fixed_octaves);
        state.in_relative = false;
        state.relative_depth = 0;
        state.fixed_octaves = octaves;
        walk_block_contents(state, block);
        (state.in_relative, state.relative_depth, state.fixed_octaves) = saved;
        state.walk_depth -= 1;
        return;
    }
    match mode {
        Some("\\drums") => {
            state.new_part("DrumStaff", "");
            add_percussion_clef(state);
            state.drum_mode += 1;
            walk_block_contents(state, block);
            state.drum_mode -= 1;
        }
        Some("\\drummode") => {
            state.drum_mode += 1;
            walk_block_contents(state, block);
            state.drum_mode -= 1;
        }
        // A chord-mode variable, walked again for its notes, stays notes.
        Some("\\chords") if state.quiet == 0 => {
            let entries = super::chord_mode::parse_chordmode_block(state, block);
            state.pending_harmonies.extend(entries);
        }
        Some("\\figures" | "\\figuremode") => {
            let entries = super::figured_bass::parse_figuremode_block(state, block);
            state.place_figures(entries);
        }
        Some("\\lyrics" | "\\lyricmode") => {}
        // Chords in a staff are their notes, which the walk reads as roots
        // only: no warnings about the chord names' suffixes.
        Some("\\chordmode" | "\\chords") => {
            state.quiet += 1;
            walk_block_contents(state, block);
            state.quiet -= 1;
        }
        _ => walk_block_contents(state, block),
    }
    state.walk_depth -= 1;
}

/// The octaves of `\fixed <pitch>` right before `block`, if it follows one:
/// the pitch's octave marks.
fn fixed_octaves(state: &WalkState, block: Node) -> Option<i32> {
    let mut marks = 0;
    let mut at = block.prev_sibling();
    while let Some(n) = at.filter(|n| n.kind() == "punctuation") {
        marks += match state.text(n) {
            "'" => 1,
            "," => -1,
            _ => return None,
        };
        at = n.prev_sibling();
    }
    let pitch = at.filter(|n| n.kind() == "symbol")?;
    let fixed = pitch.prev_sibling()?;
    (state.text(fixed) == "\\fixed").then_some(marks)
}

/// A percussion clef where the current part is (a drum staff).
pub(super) fn add_percussion_clef(state: &mut WalkState) {
    if let Some((sign, line, octave_change)) = parse_clef_name("percussion") {
        let clef = Clef {
            sign,
            line,
            octave_change,
        };
        state.add_event(Event::Clef(1, clef));
    }
}

fn walk_block_contents(state: &mut WalkState, block: Node) {
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
                i = handle_chord(state, &children, i);
                continue;
            }
            // The rest of a chord's name (`:maj7/e`) is not notes; a text
            // script after it (`_"…"`) is the chord's.
            "punctuation" if state.quiet > 0 && matches!(state.text(node), ":" | "/") => {
                let mut end = node.end_byte();
                i += 1;
                while let Some(n) = children
                    .get(i)
                    .filter(|n| n.start_byte() == end && !matches!(state.text(**n), "^" | "_"))
                {
                    end = n.end_byte();
                    i += 1;
                }
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
            "embedded_scheme" => {
                // `#(skip-of-length VAR)` emits a spacer the same length as
                // music variable VAR (used to align a parallel cadenza voice).
                // Any other embedded scheme at the music level is ignored.
                let text = state.text(node).to_string();
                if let Some(var) = parse_skip_of_length(&text) {
                    if let Some(dur) = state.variable_total_duration(var) {
                        if dur > Frac::from_integer(0) {
                            let mut rest = Rest::new(crate::ir::duration::Duration::new(dur));
                            rest.is_spacer = true;
                            state.push_voice_element(VoiceElement::Rest(rest));
                        }
                    }
                }
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

/// Handle a chord `< … >` with its duration and attachments at
/// `children[i]`. Returns the next index.
fn handle_chord(state: &mut WalkState, children: &[Node], i: usize) -> usize {
    let chord_node = children[i];
    let mut i = i + 1;
    let mut dur = consume_duration(state, children, &mut i);
    // Apply *N/M duration scaling (factor carries forward, as for notes)
    if let Some(scale) = consume_duration_scale(state, children, &mut i) {
        dur.base *= scale;
        state.last_duration = dur.clone();
    }
    let attachments = consume_attachments(state, children, &mut i);
    let mut chord = build_chord(state, chord_node, dur);
    if chord.notes.is_empty() {
        // `<>`: no time, only its marks, at this moment.
        let mut carrier = Note::new(Pitch::default(), chord.duration.clone());
        apply_note_attachments(state, &mut carrier, &attachments);
        let marks = carrier
            .dynamics
            .into_iter()
            .map(|d| Direction {
                placement: d.placement,
                dynamic: Some(d),
                ..Direction::default()
            })
            .chain(carrier.wedges.into_iter().map(|w| Direction {
                wedge: Some(w),
                ..Direction::default()
            }))
            .chain(carrier.text_directions.into_iter().map(|t| Direction {
                placement: t.placement,
                text: Some(t),
                ..Direction::default()
            }));
        for d in marks {
            state.add_event(Event::direction(d));
        }
        return i;
    }
    apply_chord_attachments(state, &mut chord, &attachments);
    // Remember the chord's pitches for the `q` repeat shorthand.
    state.last_chord_pitches = chord.notes.iter().map(|n| n.pitch).collect();
    state.push_voice_element(VoiceElement::Chord(chord));
    i
}

/// Read one note, chord or block of music at `children[i]` the usual way
/// (the music a command applies to). Returns the next index.
pub(super) fn walk_one(state: &mut WalkState, children: &[Node], i: usize) -> usize {
    let Some(node) = children.get(i) else {
        return i;
    };
    match node.kind() {
        "symbol" => {
            let sym = state.text(*node).to_string();
            handle_symbol(state, children, i, &sym)
        }
        "chord" => handle_chord(state, children, i),
        "expression_block" => {
            walk_music_block(state, *node);
            i + 1
        }
        _ => i,
    }
}

/// Push a grace group (`\grace`, `\acciaccatura`, `\appoggiatura`,
/// `\afterGrace`): every note marked as a grace of that kind.
fn push_graces(state: &mut WalkState, graces: Vec<VoiceElement>, slash: bool, after: bool) {
    for mut e in graces {
        let notes: Vec<&mut Note> = match &mut e {
            VoiceElement::Note(n) => vec![n.as_mut()],
            VoiceElement::Chord(c) => c.notes.iter_mut().collect(),
            VoiceElement::Rest(_) => continue,
        };
        for n in notes {
            n.is_grace = true;
            n.grace_slash = slash;
            n.after_grace = after;
        }
        state.push_voice_element(e);
    }
}

/// Handle a symbol node (pitch name, r, R, s, etc.).
/// Returns the next index to process.
fn handle_symbol(state: &mut WalkState, children: &[Node], i: usize, sym: &str) -> usize {
    let (node, prev) = (children[i], previous(children, i));
    let mut i = i + 1;
    if prev.is_some_and(|p| state.text(p) == "\\fixed") {
        // `\fixed`'s pitch, not a note: the block after it reads it.
        consume_octave_marks(state, children, &mut i);
        return i;
    }

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
                        if state.stopped() {
                            break;
                        }
                        let mut rest = Rest::new(dur.clone());
                        rest.is_spacer = true;
                        apply_rest_attachments(&mut rest, &attachments);
                        state.push_voice_element(VoiceElement::Rest(rest));
                    }
                }
                Some(frac) => {
                    // Integer multiplier: push N spacer rests
                    let count = *frac.numer() as u32;
                    for _ in 0..count {
                        if state.stopped() {
                            break;
                        }
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
        "q" => {
            // Chord repetition: `q` repeats the pitches of the most recent chord
            // with its own (possibly defaulted) duration. Articulations/ties are
            // attached to the q-chord itself, not copied from the original.
            let mut dur = consume_duration(state, children, &mut i);
            if let Some(scale) = consume_duration_scale(state, children, &mut i) {
                dur.base *= scale;
                state.last_duration = dur.clone();
            }
            let attachments = consume_attachments(state, children, &mut i);
            if state.last_chord_pitches.is_empty() {
                // No prior chord to repeat — emit a spacer to keep timing intact.
                let mut rest = Rest::new(dur);
                rest.is_spacer = true;
                state.push_voice_element(VoiceElement::Rest(rest));
            } else {
                let notes: Vec<Note> = state
                    .last_chord_pitches
                    .iter()
                    .map(|p| Note::new(*p, dur.clone()))
                    .collect();
                if state.in_relative {
                    state.prev_pitch = state.last_chord_pitches.first().cloned();
                }
                let mut chord = Chord::new(dur, notes);
                apply_chord_attachments(state, &mut chord, &attachments);
                state.push_voice_element(VoiceElement::Chord(chord));
            }
        }
        _ if state.drum_mode > 0 => match state.drum_pitch(sym) {
            // A drum note: absolute, and outside `\relative` and `\transpose`.
            Some(pitch) => {
                let mut dur = consume_duration(state, children, &mut i);
                if let Some(scale) = consume_duration_scale(state, children, &mut i) {
                    dur.base *= scale;
                    state.last_duration = dur.clone();
                }
                let tremolo = consume_tremolo(state, children, &mut i, &dur);
                let attachments = consume_attachments(state, children, &mut i);
                let mut note = Note::new(pitch, dur);
                note.tremolo_marks = tremolo;
                apply_note_attachments(state, &mut note, &attachments);
                state.push_voice_element(note_or_pitched_rest(note, &attachments));
            }
            None => state.warn(
                node,
                "unrecognized-token",
                format!("`{sym}` is not a drum name"),
            ),
        },
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
            } else if !prev.is_some_and(|p| {
                matches!(p.kind(), "escaped_word" | "embedded_scheme" | "string")
                    || (p.kind() == "punctuation" && state.text(p) == "=")
            }) {
                // Not a command's argument (a context name, a `\repeat`
                // type…) either: a word the walk does not read.
                let language = state.language.as_str();
                state.warn(
                    node,
                    "unrecognized-token",
                    format!("`{sym}` is not a {language} note name"),
                );
            }
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
                        state.add_event(Event::Key(ks));
                    }
                }
            }
        }
        "\\time" => {
            // \time <fraction>, or compound \time 3+2/8 which the grammar
            // splits into leading `<uint> +` pairs before the final fraction.
            // A beat structure before the fraction (`\time 3,2 5/8`) only
            // groups the beams: the signature is the fraction.
            while i + 1 < children.len()
                && children[i].kind() == "unsigned_integer"
                && children[i + 1].kind() == "punctuation"
                && punct_text(state, children[i + 1]) == ","
            {
                i += 2;
            }
            if i + 1 < children.len()
                && children[i].kind() == "unsigned_integer"
                && children[i + 1].kind() == "fraction"
                && previous(children, i).is_some_and(|p| state.text(p) == ",")
            {
                i += 1;
            }
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
            if let Some(&frac_node) = children.get(i) {
                if frac_node.kind() == "fraction" {
                    let frac_text = state.text(frac_node);
                    // A zero term is no time signature; a denominator the IR's
                    // `u8` cannot hold (`\time 3/256`) is dropped: truncated,
                    // it would become 0.
                    let parsed = match parse_fraction(frac_text) {
                        Some((num, den)) if num == 0 || den == 0 => {
                            // LilyPond, too, only warns and ignores it.
                            state.warn(
                                frac_node,
                                "unsupported-value",
                                format!("`\\time {frac_text}` has a zero term: ignored"),
                            );
                            None
                        }
                        parsed => {
                            let parsed = parsed.and_then(|(n, d)| Some((n, u8::try_from(d).ok()?)));
                            if parsed.is_none() {
                                state.warn(
                                    frac_node,
                                    "unsupported-value",
                                    format!(
                                        "`\\time {frac_text}`: lytk reads denominators up to \
                                         255; dropped"
                                    ),
                                );
                            }
                            parsed
                        }
                    };
                    if let Some((num, den)) = parsed {
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
                        let ts = TimeSignature {
                            beats,
                            beat_type: den,
                            symbol: None,
                        };
                        state.add_event(Event::Time(ts));
                    }
                    i += 1;
                }
            }
        }
        "\\transposition" => {
            // `\transposition bes`: the (absolute) pitch a written c' sounds.
            if let Some((step, alter)) = children
                .get(i)
                .filter(|n| n.kind() == "symbol")
                .and_then(|n| parse_pitch_name(state.text(*n), state.language))
            {
                i += 1;
                let marks = consume_octave_marks(state, children, &mut i);
                let sounding = Pitch::with_alter(step, alter, 3 + marks);
                state.add_event(Event::Transpose(Transpose::from_sounding_c(&sounding)));
            }
        }
        "\\compoundMeter" => {
            if let Some(ts) = children
                .get(i)
                .filter(|n| n.kind() == "embedded_scheme")
                .and_then(|n| compound_meter(state.text(*n)))
            {
                state.add_event(Event::Time(ts));
                i += 1;
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
                        state.add_event(Event::Clef(1, clef));
                    }
                }
            }
        }
        "\\tempo" => {
            // \tempo "text" dur = bpm  OR  \tempo dur = bpm  OR  \tempo "text"
            i = consume_tempo(state, children, i);
        }
        "\\grace" | "\\slashedGrace" | "\\acciaccatura" | "\\appoggiatura" => {
            // \grace { music }  OR  \grace note  OR  \grace <chord>
            let is_slash = matches!(text, "\\acciaccatura" | "\\slashedGrace");
            let mut graces = if children
                .get(i)
                .is_some_and(|n| n.kind() == "expression_block")
            {
                i += 1;
                parse_grace_block(state, children[i - 1])
            } else {
                grace_one(state, children, &mut i)
            };
            // An acciaccatura or appoggiatura is slurred to its main note:
            // read as the slur it is, so every format keeps it.
            if matches!(text, "\\acciaccatura" | "\\appoggiatura") {
                if let Some(first) = graces.iter_mut().find_map(|e| match e {
                    VoiceElement::Note(n) => Some(n.as_mut()),
                    VoiceElement::Chord(c) => c.notes.first_mut(),
                    VoiceElement::Rest(_) => None,
                }) {
                    first.slurs.push(SlurEvent {
                        slur_type: StartStop::Start,
                        number: 1,
                        placement: Placement::Unspecified,
                    });
                    state.grace_slur_to_main = true;
                }
            }
            push_graces(state, graces, is_slash, false);
        }
        "\\tuplet" | "\\times" => {
            // \tuplet actual/normal { notes }  OR  \times normal/actual { notes }
            if let Some(&frac_node) = children.get(i) {
                if frac_node.kind() == "fraction" {
                    let frac_text = state.text(frac_node);
                    if let Some((num, denom)) = frac_text.split_once('/') {
                        // A ratio with a 0 or a term beyond the IR's `u8`
                        // (`\tuplet 0/2`, `\times 2/0`, `\tuplet 300/2`) scales
                        // nothing: its music is read unscaled.
                        let ratio = match (num.parse::<u64>(), denom.parse::<u64>()) {
                            (Ok(0), _) | (_, Ok(0)) => {
                                state.error(
                                    frac_node,
                                    "invalid-ratio",
                                    format!("`{text} {frac_text}` has a zero term: read unscaled"),
                                );
                                None
                            }
                            (n, d) => {
                                let term =
                                    |t: Result<u64, _>| t.ok().and_then(|t| u8::try_from(t).ok());
                                let ratio = term(n).zip(term(d));
                                if ratio.is_none() {
                                    state.warn(
                                        frac_node,
                                        "unsupported-value",
                                        format!(
                                            "`{text} {frac_text}`: lytk reads tuplet terms up to \
                                             255; read unscaled"
                                        ),
                                    );
                                }
                                ratio
                            }
                        };
                        let ratio = ratio.map(|(n, d)| {
                            if text == "\\tuplet" {
                                (n, d)
                            } else {
                                (d, n) // \times has reversed fraction
                            }
                        });
                        i += 1;
                        // Optional group-duration argument, e.g. `\tuplet 3/2 4 { … }`
                        // (the `4` tells LilyPond the span of each tuplet group for
                        // beaming; it does not change the ratio). Skip it — and any
                        // trailing dots — so the music block is still found.
                        if children
                            .get(i)
                            .is_some_and(|n| n.kind() == "unsigned_integer")
                        {
                            i += 1;
                            while children
                                .get(i)
                                .is_some_and(|n| n.kind() == "punctuation" && state.text(*n) == ".")
                            {
                                i += 1;
                            }
                        }
                        if let Some(block) = children.get(i) {
                            if block.kind() == "expression_block" {
                                if let Some((actual, normal)) = ratio {
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
                                } else {
                                    walk_music_block(state, *block);
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
            let dir = Direction {
                pedal: Some(PedalEvent {
                    pedal_type: pedal_type.to_string(),
                    line: false,
                }),
                placement: Placement::Below,
                ..Default::default()
            };
            let at = state.last_element_onset;
            state.add_event_at(at, Event::direction(dir));
        }
        "\\ottava" => {
            // \ottava #1, \ottava #-1, \ottava #0
            if let Some(scheme_node) = children.get(i) {
                if scheme_node.kind() == "embedded_scheme" {
                    let scheme_text = state.text(*scheme_node);
                    // Parse #N or #-N from the embedded scheme
                    let num_str = scheme_text.trim_start_matches('#');
                    if let Ok(n) = num_str.parse::<i64>() {
                        let dir = Direction {
                            octave_shift: Some(OctaveShift::from_octaves(n)),
                            ..Default::default()
                        };
                        state.add_event(Event::direction(dir));
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
            state.add_event(Event::direction(dir));
        }
        "\\pageBreak" => {
            let dir = Direction {
                layout_break: Some(LayoutBreakType::Page),
                ..Default::default()
            };
            state.add_event(Event::direction(dir));
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
        "\\slurUp" => state.slur_placement = Placement::Above,
        "\\slurDown" => state.slur_placement = Placement::Below,
        "\\slurNeutral" => state.slur_placement = Placement::Unspecified,
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
                    i += 1;
                    // `\bar ""` is an invisible bar line — only a place a line
                    // may break (e.g. inside a cadenza). It makes no bar.
                    if bar_text.is_empty() {
                        return i;
                    }
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
                    // A start-repeat sign opens the bar after it; every other
                    // bar line closes the bar before it. Post-events after it
                    // belong here, not to the note before it.
                    state.flush_voice();
                    if barline.style == BarlineType::RepeatForward {
                        state.add_event(Event::LeftBarline(Barline {
                            repeat_direction: Some(RepeatDirection::Forward),
                            ..barline
                        }));
                    } else {
                        state.add_event(Event::RightBarline(barline));
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
                    state.context_reentry.set(false);
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
            // \partial <dur>[*N[/M]]  → anacrusis / pickup
            let mut dur = consume_duration(state, children, &mut i);
            if let Some(scale) = consume_duration_scale(state, children, &mut i) {
                dur.base *= scale;
            }
            state.add_event(Event::Partial(dur.actual_duration()));
            if state.pos == Frac::from_integer(0) {
                state.metadata.partial_duration = Some(dur);
            }
        }
        "\\afterGrace" => {
            // `\afterGrace [FRACTION] MAIN { GRACES }`: MAIN is music as
            // usual; the graces after it are sung at its end.
            if children.get(i).is_some_and(|n| n.kind() == "fraction") {
                i += 1;
            }
            i = walk_one(state, children, i);
            match children.get(i).map(|n| n.kind()) {
                Some("expression_block") => {
                    let graces = parse_grace_block(state, children[i]);
                    push_graces(state, graces, false, true);
                    i += 1;
                }
                Some("symbol") | Some("chord") => {
                    let graces = grace_one(state, children, &mut i);
                    push_graces(state, graces, false, true);
                }
                _ => {}
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
        "\\textMark" | "\\textEndMark" | "\\jump" | "\\sectionLabel" | "\\fine" => {
            // LilyPond 2.24's text marks: a section label is a rehearsal
            // mark; the others are text above the staff.
            let mark = if text == "\\fine" {
                Some("Fine".to_string())
            } else {
                let t = mark_text(state, children, i);
                i = match children.get(i).map(|n| n.kind()) {
                    Some("string") => i + 1,
                    Some("escaped_word") => skip_markup(state, children, i + 1),
                    _ => i,
                };
                t
            };
            if let Some(t) = mark {
                let dir = if text == "\\sectionLabel" {
                    Direction {
                        rehearsal: Some(crate::ir::direction::RehearsalMark { text: t }),
                        ..Default::default()
                    }
                } else {
                    Direction {
                        placement: Placement::Above,
                        text: Some(crate::ir::direction::TextDirection {
                            text: t,
                            placement: Placement::Above,
                            font_style: None,
                            font_weight: None,
                        }),
                        ..Default::default()
                    }
                };
                state.add_event(Event::direction(dir));
            }
        }
        "\\segnoMark" | "\\codaMark" => {
            // `\segnoMark \default` / `\codaMark 2`: the sign.
            if matches!(
                children.get(i).map(|n| n.kind()),
                Some("unsigned_integer" | "embedded_scheme")
            ) || children
                .get(i)
                .is_some_and(|n| state.text(*n) == "\\default")
            {
                i += 1;
            }
            let dir = Direction {
                segno: text == "\\segnoMark",
                coda: text == "\\codaMark",
                ..Default::default()
            };
            state.add_event(Event::direction(dir));
        }
        "\\once" => {
            // `\once \stemUp` and its kind apply to the next note only;
            // `\once \override` goes on to the \override handler.
            let next = children.get(i).map(|n| state.text(*n));
            let stem = match next {
                Some("\\stemUp") => Some("up"),
                Some("\\stemDown") => Some("down"),
                Some("\\stemNeutral") => Some(""),
                _ => None,
            };
            if let Some(stem) = stem {
                let before = std::mem::replace(&mut state.stem_direction, stem.to_string());
                state.once_stem = Some(before);
                i += 1;
            }
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
                            if prop_text.ends_with("autoBeaming") {
                                // `##f` turns automatic beams off, `##t` on.
                                state.auto_beam_off = scheme_text.trim_start_matches('#') == "f";
                            } else if prop_text.contains("measureLength") {
                                match parse_ly_make_moment(scheme_text) {
                                    Some((_, 0)) => state.error(
                                        *val_node,
                                        "invalid-ratio",
                                        "a measure length with a zero denominator: dropped"
                                            .to_string(),
                                    ),
                                    Some((0, _)) => state.warn(
                                        *val_node,
                                        "unsupported-value",
                                        "a measure length of zero: dropped".to_string(),
                                    ),
                                    Some((num, den)) => {
                                        let len = Frac::new(num as i64, den as i64);
                                        state.add_event(Event::MeasureLength(len));
                                    }
                                    None => {}
                                }
                            } else if let Some(s) = extract_scheme_string(scheme_text) {
                                apply_set_property(state, &prop_text, &s);
                            }
                            i += 1;
                        } else if matches!(state.text(*val_node), "\\markup" | "\\markuplist") {
                            i = skip_markup(state, children, i + 1);
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
                        if state.stopped() {
                            break;
                        }
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
        // Score-wide free time: the span becomes one senza-misura bar.
        "\\cadenzaOn" => state.add_event(Event::CadenzaOn),
        "\\cadenzaOff" => state.add_event(Event::CadenzaOff),
        "\\unset" | "\\dynamicUp" | "\\dynamicDown" | "\\dynamicNeutral" | "\\context"
        | "\\unfoldRepeats" => {
            // Skip these commands; some may consume the next token
            // \context within music blocks is handled by named_context at the
            // walk_music_block level, but if tree-sitter doesn't wrap it as
            // named_context, skip it here.
        }
        "\\markup" | "\\markuplist" => {
            // Text standing on its own (as a `\tempo` or `\set` value): not music.
            i = skip_markup(state, children, i);
        }
        "\\language" => {
            if let Some(&next) = children.get(i).filter(|n| n.kind() == "string") {
                state.set_language(next);
                i += 1;
            }
        }
        _ => {
            // Unknown escaped word — may be a variable reference or dynamic
            let var_name = text.trim_start_matches('\\');
            if !state.resolve_variable(var_name) {
                if is_dynamic_name(text) {
                    attach_dynamic(state, text);
                } else {
                    state.unread_command(children[i - 1], var_name);
                }
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
        // A bar check only checks; bars come from the meter. It does end the
        // run, so a following post-event (`| \p`) lands here rather than on
        // the note before the bar line.
        "|" => state.flush_voice(),
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

/// `\compoundMeter #'((3 2 8))` as `3+2/8`; groups over several
/// denominators (`#'((3 8) (2 4))`) add up over the smallest value.
fn compound_meter(scheme: &str) -> Option<TimeSignature> {
    let groups: Vec<Vec<u32>> = scheme
        .split('(')
        .map(|g| g.split(')').next().unwrap_or(""))
        .map(|g| {
            g.split_whitespace()
                .filter_map(|n| n.parse().ok())
                .collect()
        })
        .filter(|g: &Vec<u32>| g.len() >= 2)
        .collect();
    let den = groups.iter().filter_map(|g| g.last().copied()).max()?;
    let mut beats = Vec::new();
    for g in &groups {
        let (d, counts) = g.split_last()?;
        if *d == 0 || den % d != 0 {
            return None;
        }
        beats.extend(counts.iter().map(|c| (c * (den / d)).to_string()));
    }
    Some(TimeSignature {
        beats: beats.join("+"),
        beat_type: u8::try_from(den).ok()?,
        symbol: None,
    })
}
