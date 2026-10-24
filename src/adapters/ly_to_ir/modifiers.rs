use tree_sitter::Node;

use crate::ir::direction::{Barline, BarlineType, RepeatDirection};
use crate::ir::duration::Frac;
use crate::ir::language::{parse_pitch_name, PitchMode};
use crate::ir::note::{Note, VoiceElement};
use crate::ir::pitch::{Pitch, PitchStep};

use super::consume::consume_octave_marks;
use super::music::{walk_music_block, walk_one};
use super::state::WalkState;
use super::walk::walk_body;
use crate::ir::direction::EndingType;
use crate::ir::timeline::Event;

/// Consume `\relative`'s optional start pitch and the music after it, read
/// relative to that pitch; without one the first note is absolute, as with
/// LilyPond's `\relative f`. The music is whatever follows (`{ … }`,
/// `<< … >>`, `\new Staff …`, a variable, one note or chord): `walk_body`
/// or `walk_one` reads it. Afterwards the relative state is what it was
/// before: the music around a `\relative` is not relative because of it, and
/// an outer `\relative` goes on from its own previous pitch (LilyPond passes
/// the pitch through music it has already made relative). Returns the next
/// index.
pub(super) fn consume_relative(state: &mut WalkState, children: &[Node], mut i: usize) -> usize {
    let start = relative_start(state, children, &mut i);
    state.mode = PitchMode::Relative;
    let saved = (state.in_relative, state.relative_ref, state.prev_pitch);
    state.in_relative = true;
    state.relative_ref = Some(start);
    state.prev_pitch = Some(start);
    state.relative_depth += 1;
    let mut next = walk_body(state, children, i);
    if next == i {
        // `\relative c' e'`: one note or chord is music too
        next = walk_one(state, children, i);
    }
    state.relative_depth -= 1;
    (state.in_relative, state.relative_ref, state.prev_pitch) = saved;
    next
}

/// The start pitch of a `\relative` at `children[*i]`, moving `*i` past it
/// and its octave marks; without one, LilyPond's `f`, which makes the first
/// note absolute.
pub(super) fn relative_start(state: &WalkState, children: &[Node], i: &mut usize) -> Pitch {
    let mut start = Pitch::new(PitchStep::F, 3);
    if let Some(node) = children.get(*i).filter(|n| n.kind() == "symbol") {
        let sym = state.text(*node).to_string();
        if let Some((step, alter)) = parse_pitch_name(&sym, state.language) {
            *i += 1;
            let marks = consume_octave_marks(state, children, i);
            start = Pitch::with_alter(step, alter, 3 + marks);
        }
    }
    // Octave marks without a pitch before them mean nothing.
    while children
        .get(*i)
        .is_some_and(|n| n.kind() == "punctuation" && matches!(state.text(*n), "'" | ","))
    {
        *i += 1;
    }
    start
}

/// Consume `\repeat volta N { music } [\alternative { {..} {..} }]`.
/// Inserts forward/backward repeat barlines and ending markers for alternatives.
pub(super) fn consume_repeat(state: &mut WalkState, children: &[Node], mut i: usize) -> usize {
    // Parse repeat type (volta, unfold, etc.)
    let repeat_type = if i < children.len() && children[i].kind() == "symbol" {
        let t = state.text(children[i]).to_string();
        i += 1;
        t
    } else {
        return i;
    };

    // Parse repeat count
    // Past `u64` a count is unreadable: it counts as the most there can be,
    // which the unfold loop's budget then refuses.
    let count: u64 = if i < children.len() && children[i].kind() == "unsigned_integer" {
        let n = state.text(children[i]).parse().unwrap_or(u64::MAX);
        i += 1;
        n
    } else {
        2
    };
    let repeat_count = u8::try_from(count).unwrap_or(u8::MAX);

    if repeat_type == "tremolo" {
        return consume_tremolo_repeat(state, children, i, count);
    }

    // For unfold repeats, walk the body block `repeat_count` times — the music
    // is repeated literally (was previously walked only once, dropping N-1
    // copies).
    if repeat_type == "unfold" {
        if i < children.len() && children[i].kind() == "expression_block" {
            let body = children[i];
            // LilyPond makes the written music relative once and then copies
            // it: every pass starts from the same reference pitch, so copies
            // are identical instead of climbing an octave step per pass.
            // Each pass counts against the reading's budget, so an unfold of
            // an empty body cannot spin for billions of passes either.
            let reference = state.prev_pitch;
            for _ in 0..count.max(1) {
                if !state.spend(1) {
                    break;
                }
                state.prev_pitch = reference;
                walk_music_block(state, body);
            }
            return i + 1;
        }
        // `\relative`-wrapped or other body: fall back to a single pass.
        return consume_repeat_body(state, children, i);
    }

    // Forward repeat where the body starts.
    state.add_event(Event::LeftBarline(Barline {
        style: BarlineType::RepeatForward,
        repeat_direction: Some(RepeatDirection::Forward),
        repeat_times: Some(repeat_count),
        ..Default::default()
    }));

    // Walk the repeat body
    i = consume_repeat_body(state, children, i);

    // Check for \alternative
    if i < children.len() && children[i].kind() == "escaped_word" {
        let text = state.text(children[i]);
        if text == "\\alternative" {
            i += 1;
            // \alternative { { alt1 } { alt2 } }
            if i < children.len() && children[i].kind() == "expression_block" {
                let alt_block = children[i];
                i += 1;
                consume_alternatives(state, alt_block, repeat_count);
            }
            return i;
        }
    }

    // No alternative — the repeat closes where the body ends.
    state.add_event(Event::RightBarline(backward_repeat()));

    i
}

/// `\repeat tremolo N body`: one note or chord as a tremolo `N` times its
/// value long (`\repeat tremolo 8 c32` is `c4:32`), or two alternating, each
/// `N` times its value (`\repeat tremolo 4 { c16 e }`). The written value
/// gives the strokes. Any other body is played `N` times, with a warning.
fn consume_tremolo_repeat(state: &mut WalkState, children: &[Node], i: usize, count: u64) -> usize {
    let (start, start_len, run_start) = (state.pos, state.current_voice.len(), state.voice_start);
    let body_node = children.get(i).copied();
    let i = super::music::walk_one(state, children, i);
    // The body is still in the voice buffer unless something placed it.
    if state.voice_start != run_start || state.current_voice.len() < start_len {
        return i;
    }
    let mut body = state.current_voice.split_off(start_len);
    let timed: Vec<usize> = (0..body.len())
        .filter(|&k| {
            !matches!(body[k], VoiceElement::Rest(_)) && body[k].metric_duration() > zero()
        })
        .collect();
    let factor = Frac::from_integer(i64::try_from(count.max(1)).unwrap_or(i64::MAX).min(1 << 20));
    if (1..=2).contains(&timed.len()) && timed.len() == body.len() {
        let pair = timed.len() == 2;
        for (k, e) in body.iter_mut().enumerate() {
            let marks = beam_level(e);
            let tremolo = |n: &mut Note| {
                n.tremolo_marks = marks;
                n.two_note_tremolo = pair;
                n.tremolo_start = k == 0;
                if !pair {
                    n.ornaments.push(crate::ir::articulation::Ornament {
                        name: crate::ir::articulation::OrnamentType::Tremolo,
                        placement: Default::default(),
                    });
                }
            };
            match e {
                VoiceElement::Note(n) => {
                    n.duration.base *= factor;
                    tremolo(n);
                }
                VoiceElement::Chord(c) => {
                    c.duration.base *= factor;
                    for n in &mut c.notes {
                        n.duration.base *= factor;
                    }
                    tremolo(&mut c.notes[0]);
                }
                VoiceElement::Rest(_) => {}
            }
        }
    } else {
        if let Some(node) = body_node {
            state.warn(
                node,
                "unsupported-value",
                "a `\\repeat tremolo` of more than two notes is played out".to_string(),
            );
        }
        let copy = body.clone();
        for _ in 1..count.clamp(1, 1 << 12) {
            body.extend(copy.iter().cloned());
        }
    }
    state.pos = start;
    for e in body {
        state.pos += e.metric_duration();
        state.current_voice.push(e);
    }
    i
}

/// The beam count of an element's written value: a tremolo's strokes.
fn beam_level(e: &VoiceElement) -> u8 {
    match e {
        VoiceElement::Note(n) => crate::ir::beams::beam_level_for_duration(&n.duration),
        VoiceElement::Chord(c) => crate::ir::beams::beam_level_for_duration(&c.duration),
        VoiceElement::Rest(_) => 0,
    }
}

fn zero() -> Frac {
    Frac::from_integer(0)
}

/// Consume the body of a \repeat (expression_block, or \relative { } etc.)
fn consume_repeat_body(state: &mut WalkState, children: &[Node], mut i: usize) -> usize {
    while i < children.len() {
        let node = children[i];
        match node.kind() {
            "expression_block" => {
                walk_music_block(state, node);
                i += 1;
                return i;
            }
            "escaped_word" => {
                let text = state.text(node).to_string();
                i += 1;
                if text == "\\relative" {
                    // \repeat volta 2 \relative c' { ... }
                    return consume_relative(state, children, i);
                }
                // Unknown escaped word inside repeat header — skip
                return i;
            }
            _ => {
                i += 1;
            }
        }
    }
    i
}

/// Parse `\alternative { { alt1 } { alt2 } ... }` and mark endings.
fn consume_alternatives(state: &mut WalkState, alt_block: Node, _repeat_count: u8) {
    // Walk through children more carefully to handle \relative alternatives
    let children: Vec<Node> = {
        let mut c = Vec::new();
        let mut cursor2 = alt_block.walk();
        if cursor2.goto_first_child() {
            loop {
                c.push(cursor2.node());
                if !cursor2.goto_next_sibling() {
                    break;
                }
            }
        }
        c
    };

    // Parse alternatives — each is either an expression_block or \relative <pitch> { }
    let mut ending_num: u8 = 1;
    let mut ci = 0;
    // (start, end, number) of each non-empty alternative.
    let mut alts: Vec<(Frac, Frac, u8)> = Vec::new();
    while ci < children.len() {
        let node = children[ci];
        match node.kind() {
            "expression_block" => {
                alts.extend(walk_alternative_block(state, node, ending_num));
                ending_num += 1;
                ci += 1;
            }
            "escaped_word" => {
                let text = state.text(node).to_string();
                ci += 1;
                if text == "\\relative" {
                    // \relative <pitch> { ... }
                    if ci < children.len() && children[ci].kind() == "symbol" {
                        let ref_sym = state.text(children[ci]).to_string();
                        ci += 1;
                        if let Some((step, alter)) = parse_pitch_name(&ref_sym, state.language) {
                            let oct_marks = consume_octave_marks(state, &children, &mut ci);
                            let ref_pitch = Pitch::with_alter(step, alter, 3 + oct_marks);
                            let old_relative = state.in_relative;
                            let old_prev = state.prev_pitch;
                            let old_ref = state.relative_ref;
                            state.in_relative = true;
                            state.prev_pitch = Some(ref_pitch);
                            state.relative_ref = Some(ref_pitch);
                            if ci < children.len() && children[ci].kind() == "expression_block" {
                                alts.extend(walk_alternative_block(
                                    state,
                                    children[ci],
                                    ending_num,
                                ));
                                ending_num += 1;
                                ci += 1;
                            }
                            state.in_relative = old_relative;
                            state.prev_pitch = old_prev;
                            state.relative_ref = old_ref;
                        }
                    }
                }
            }
            _ => {
                ci += 1;
            }
        }
    }

    // Each alternative is a volta: an ending opens where it starts and closes
    // where it ends, and every ending but the last (empty ones count) also
    // repeats back.
    let total = ending_num - 1;
    if alts.is_empty() || total == 1 {
        // `\alternative { }`, or a lone alternative played on every pass:
        // close the repeat like the no-alternative case.
        state.add_event(Event::RightBarline(backward_repeat()));
        return;
    }
    for (start, end, number) in alts {
        state.add_event_at(
            start,
            Event::LeftBarline(Barline {
                ending_number: Some(number),
                ending_type: Some(EndingType::Start),
                ..Default::default()
            }),
        );
        let close = if number < total {
            backward_repeat()
        } else {
            Barline::default()
        };
        state.add_event_at(
            end,
            Event::RightBarline(Barline {
                ending_number: Some(number),
                ending_type: Some(EndingType::Stop),
                ..close
            }),
        );
    }
}

fn backward_repeat() -> Barline {
    Barline {
        style: BarlineType::RepeatBackward,
        repeat_direction: Some(RepeatDirection::Backward),
        ..Default::default()
    }
}

/// Walk one alternative block; returns its (start, end, number) when it has music.
fn walk_alternative_block(
    state: &mut WalkState,
    block: Node,
    ending_num: u8,
) -> Option<(Frac, Frac, u8)> {
    let start = state.pos;
    walk_music_block(state, block);
    (state.pos > start).then_some((start, state.pos, ending_num))
}

/// Consume `\transpose <from> <to> { music }`.
/// Parses two pitches (from and to), pushes the transposition interval onto the
/// stack, walks the music block, then pops the interval.
pub(super) fn consume_transpose(state: &mut WalkState, children: &[Node], mut i: usize) -> usize {
    // Parse first pitch (from)
    let from = consume_transpose_pitch(state, children, &mut i);
    // Parse second pitch (to)
    let to = consume_transpose_pitch(state, children, &mut i);

    let (from, to) = match (from, to) {
        (Some(f), Some(t)) => (f, t),
        _ => return i, // couldn't parse both pitches — bail
    };

    state.transpose_stack.push((from, to));

    // Now look for the music block or escaped variable reference
    while i < children.len() {
        let node = children[i];
        match node.kind() {
            "expression_block" => {
                walk_music_block(state, node);
                i += 1;
                break;
            }
            "escaped_word" => {
                let text = state.text(node).to_string();
                i += 1;
                // Could be \relative, a variable ref, or another command
                if text == "\\relative" {
                    i = consume_relative(state, children, i);
                } else if text == "\\transpose" {
                    i = consume_transpose(state, children, i);
                } else {
                    let var_name = text.trim_start_matches('\\');
                    state.resolve_variable(var_name);
                }
                break;
            }
            _ => {
                i += 1;
            }
        }
    }

    state.transpose_stack.pop();
    i
}

/// Helper: consume a single pitch (symbol + octave marks) for \transpose arguments.
fn consume_transpose_pitch(state: &WalkState, children: &[Node], i: &mut usize) -> Option<Pitch> {
    // Skip non-symbol tokens (whitespace, punctuation)
    while *i < children.len() {
        let node = children[*i];
        if node.kind() == "symbol" {
            let sym = state.text(node).to_string();
            if let Some((step, alter)) = parse_pitch_name(&sym, state.language) {
                *i += 1;
                let octave_marks = consume_octave_marks(state, children, i);
                return Some(Pitch::with_alter(step, alter, 3 + octave_marks));
            } else {
                break;
            }
        } else {
            *i += 1;
        }
    }
    None
}
