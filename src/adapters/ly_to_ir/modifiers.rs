use tree_sitter::Node;

use crate::ir::direction::{Barline, BarlineType, RepeatDirection};
use crate::ir::duration::Frac;
use crate::ir::language::{parse_pitch_name, PitchMode};
use crate::ir::pitch::Pitch;

use super::consume::consume_octave_marks;
use super::music::walk_music_block;
use super::state::WalkState;
use super::timeline::Event;

/// Consume optional reference pitch after `\relative`, then the music block.
/// Returns the next index.
pub(super) fn consume_relative(state: &mut WalkState, children: &[Node], mut i: usize) -> usize {
    // After \relative we may see:
    //   - a pitch symbol (reference pitch), octave marks, then expression_block
    //   - directly an expression_block
    let mut octave_marks = 0i32;

    while i < children.len() {
        let node = children[i];
        match node.kind() {
            "symbol" => {
                let sym = state.text(node).to_string();
                if let Some((step, alter)) = parse_pitch_name(&sym, state.language) {
                    // This is the reference pitch
                    let mut rp = Pitch::with_alter(step, alter, 3); // base octave
                    i += 1;
                    // Consume octave marks
                    while i < children.len() {
                        let n = children[i];
                        if n.kind() == "punctuation" {
                            let t = state.text(n);
                            if t == "'" {
                                octave_marks += 1;
                                i += 1;
                            } else if t == "," {
                                octave_marks -= 1;
                                i += 1;
                            } else {
                                break;
                            }
                        } else {
                            break;
                        }
                    }
                    rp.octave = 3 + octave_marks;
                    state.relative_ref = Some(rp);
                    state.prev_pitch = Some(rp);
                    continue;
                } else {
                    break;
                }
            }
            "punctuation" => {
                let t = state.text(node);
                if t == "'" || t == "," {
                    // Stray octave marks (shouldn't happen without a pitch before)
                    i += 1;
                    continue;
                }
                break;
            }
            "expression_block" => {
                // Found the music block
                if state.relative_ref.is_none() {
                    // No reference pitch given; default to middle C
                    state.relative_ref = Some(Pitch::new(crate::ir::pitch::PitchStep::C, 4));
                    state.prev_pitch = state.relative_ref;
                }
                walk_music_block(state, node);
                i += 1;
                return i;
            }
            _ => break,
        }
    }
    i
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
    let repeat_count: u8 = if i < children.len() && children[i].kind() == "unsigned_integer" {
        let n = state.text(children[i]).parse().unwrap_or(2);
        i += 1;
        n
    } else {
        2
    };

    // For unfold repeats, walk the body block `repeat_count` times — the music
    // is repeated literally (was previously walked only once, dropping N-1
    // copies).
    if repeat_type == "unfold" {
        if i < children.len() && children[i].kind() == "expression_block" {
            let body = children[i];
            for _ in 0..repeat_count.max(1) {
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
                    // consume the reference pitch and block
                    if i < children.len() && children[i].kind() == "symbol" {
                        let ref_sym = state.text(children[i]).to_string();
                        i += 1;
                        if let Some((step, alter)) = parse_pitch_name(&ref_sym, state.language) {
                            let oct_marks = consume_octave_marks(state, children, &mut i);
                            let ref_pitch = Pitch::with_alter(step, alter, 3 + oct_marks);
                            let old_relative = state.in_relative;
                            let old_prev = state.prev_pitch;
                            let old_ref = state.relative_ref;
                            state.in_relative = true;
                            state.prev_pitch = Some(ref_pitch);
                            state.relative_ref = Some(ref_pitch);
                            if i < children.len() && children[i].kind() == "expression_block" {
                                walk_music_block(state, children[i]);
                                i += 1;
                            }
                            state.in_relative = old_relative;
                            state.prev_pitch = old_prev;
                            state.relative_ref = old_ref;
                        }
                    }
                    return i;
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
    // where it ends, and every ending but the last also repeats back.
    if alts.is_empty() {
        // Degenerate `\alternative { }` — close the repeat like the
        // no-alternative case.
        state.add_event(Event::RightBarline(backward_repeat()));
        return;
    }
    let last = alts.len() - 1;
    for (k, (start, end, number)) in alts.into_iter().enumerate() {
        state.add_event_at(
            start,
            Event::LeftBarline(Barline {
                ending_number: Some(number),
                ending_type: Some("start".to_string()),
                ..Default::default()
            }),
        );
        let close = if k < last {
            backward_repeat()
        } else {
            Barline::default()
        };
        state.add_event_at(
            end,
            Event::RightBarline(Barline {
                ending_number: Some(number),
                ending_type: Some("stop".to_string()),
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
                    state.in_relative = true;
                    state.mode = PitchMode::Relative;
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
