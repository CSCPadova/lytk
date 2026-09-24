use tree_sitter::Node;

use crate::ir::duration::Frac;
use crate::ir::language::{parse_pitch_name, PitchLanguage, PitchMode};
use crate::ir::pitch::Pitch;

use super::chord_mode::parse_chordmode_block;
use super::consume::{
    block_contains_named_context, extract_string_value, parse_paper_block, parse_with_block,
    score_block_output_types,
};
use super::figured_bass::parse_figuremode_block;
use super::lyrics::{extract_lyricsto_voice, parse_lyric_block};
use super::merge::{assign_piano_direction_staff, part_is_dynamics_only};
use super::modifiers::{consume_relative, consume_transpose};
use super::music::walk_music_block;
use super::state::{PartBuild, VarDef, WalkState};
use super::timeline::{Event, Timeline};

/// Walk the `lilypond_program` root node.
pub(super) fn walk_program(state: &mut WalkState, root: Node) {
    let mut cursor = root.walk();
    let children: Vec<Node> = root.children(&mut cursor).collect();
    let mut i = 0;

    while i < children.len() {
        let node = children[i];
        match node.kind() {
            "escaped_word" => {
                let text = state.text(node);
                match text {
                    "\\version" => {
                        // Skip version string; \version "2.24.0" consumes next string
                        i += 1; // skip string
                    }
                    "\\language" => {
                        // \language "english"
                        if let Some(next) = children.get(i + 1) {
                            if next.kind() == "string" {
                                let lang_str = extract_string_value(state, *next);
                                state.language = PitchLanguage::from_str_loose(&lang_str)
                                    .unwrap_or(PitchLanguage::Nederlands);
                                i += 1; // skip string
                            }
                        }
                    }
                    "\\header" => {
                        // \header { ... }
                        if let Some(next) = children.get(i + 1) {
                            if next.kind() == "expression_block" {
                                walk_header(state, *next);
                                i += 1;
                            }
                        }
                    }
                    "\\score" => {
                        // \score { ... } — each score block becomes a separate movement
                        if let Some(next) = children.get(i + 1) {
                            if next.kind() == "expression_block" {
                                // Skip MIDI-only score blocks (have \midi but no \layout)
                                let (has_layout, has_midi) = score_block_output_types(state, *next);
                                if has_midi && !has_layout {
                                    i += 1; // skip the expression_block
                                    i += 1;
                                    continue;
                                }
                                // Each \score block is its own movement.
                                state.flush_voice();
                                let saved_parts = std::mem::take(&mut state.parts);
                                let saved_counter = state.part_counter;
                                let saved_pending_lyrics =
                                    std::mem::take(&mut state.pending_lyrics);
                                let saved_added_lyrics = std::mem::take(&mut state.added_lyrics);
                                let saved_pending_harmonies =
                                    std::mem::take(&mut state.pending_harmonies);
                                let saved_voice_map = std::mem::take(&mut state.voice_part_map);
                                state.part_counter = 0;
                                state.set_pos(Frac::from_integer(0));
                                // `\partial` is per-movement: reset so a pickup in
                                // one \score block doesn't leak into the next.
                                state.metadata.partial_duration = None;

                                walk_score_block(state, *next);
                                if let Some(score) = super::assemble_score(state) {
                                    state.completed_scores.push(score);
                                }

                                // Restore saved state
                                state.parts = saved_parts;
                                state.part_counter = saved_counter;
                                state.pending_lyrics = saved_pending_lyrics;
                                state.added_lyrics = saved_added_lyrics;
                                state.pending_harmonies = saved_pending_harmonies;
                                state.voice_part_map = saved_voice_map;

                                i += 1;
                            }
                        }
                    }
                    "\\relative" => {
                        // Top-level \relative c' { ... }
                        state.in_relative = true;
                        state.mode = PitchMode::Relative;
                        i += 1;
                        i = consume_relative(state, &children, i);
                        continue;
                    }
                    "\\transpose" => {
                        i += 1;
                        i = consume_transpose(state, &children, i);
                        continue;
                    }
                    "\\addlyrics" => {
                        // `\new Staff { … } \addlyrics { … }` at top level:
                        // the lyrics attach to that staff (and are not music).
                        if let Some(next) = children.get(i + 1) {
                            if next.kind() == "expression_block" {
                                let syllables = parse_lyric_block(state, *next);
                                state.flush_voice();
                                let uid = state.current_uid();
                                state.added_lyrics.push((uid, syllables));
                                i += 1;
                            }
                        }
                    }
                    "\\paper" => {
                        // Top-level \paper { ... }
                        if let Some(next) = children.get(i + 1) {
                            if next.kind() == "expression_block" {
                                parse_paper_block(state, *next);
                                i += 1;
                            }
                        }
                    }
                    _ => {
                        // Top-level escaped words we don't handle
                    }
                }
            }
            "expression_block" => {
                // Bare { ... } at top level: treat as a single anonymous part
                walk_music_block(state, node);
            }
            "parallel_music" => {
                // Top-level `<< ... >>` (e.g. emitted multi-staff music with no
                // explicit \score wrapper) is an implicit score. Walk it directly.
                walk_parallel_music(state, node);
            }
            "named_context" => {
                // Top-level `\new Staff { ... }` etc. — an implicit score.
                let (context, name) = extract_named_context(state, node);
                i += 1;
                i = walk_context_body(state, &children, i, &context, &name);
                continue;
            }
            "assignment_lhs" => {
                // Variable definition: name = { ... }
                // Extract the variable name from the assignment_lhs node
                let var_name = {
                    let mut c = node.walk();
                    let result = node
                        .children(&mut c)
                        .find(|n| n.kind() == "symbol")
                        .map(|n| state.text(n).to_string())
                        .unwrap_or_default();
                    result
                };
                if !var_name.is_empty() {
                    // Skip the "=" punctuation, then capture the body
                    if let Some(eq) = children.get(i + 1) {
                        if eq.kind() == "punctuation" && state.text(*eq) == "=" {
                            let mut j = i + 2;
                            // Skip \lyricmode, \notemode, \relative, \figuremode etc. before the expression_block
                            let mut is_lyricmode = false;
                            let mut is_figuremode = false;
                            let mut is_chordmode = false;
                            let mut is_markup = false;
                            while j < children.len() {
                                let candidate = children[j];
                                if candidate.kind() == "escaped_word" {
                                    let ew = state.text(candidate);
                                    if ew == "\\markup" || ew == "\\markuplist" {
                                        // Markup variable — skip the entire definition.
                                        // Consume \markup plus any trailing escaped_words
                                        // (markup formatters) and the following expression_block.
                                        j += 1;
                                        while j < children.len()
                                            && children[j].kind() == "escaped_word"
                                        {
                                            j += 1;
                                        }
                                        if j < children.len()
                                            && (children[j].kind() == "expression_block"
                                                || children[j].kind() == "string")
                                        {
                                            j += 1;
                                        }
                                        is_markup = true;
                                        break;
                                    }
                                    if ew == "\\lyricmode" || ew == "\\notemode" {
                                        is_lyricmode = ew == "\\lyricmode";
                                        j += 1;
                                        continue;
                                    }
                                    if ew == "\\relative" {
                                        state.in_relative = true;
                                        state.mode = PitchMode::Relative;
                                        j += 1;
                                        // Consume optional reference pitch and octave marks
                                        let mut octave_marks = 0i32;
                                        while j < children.len() {
                                            let n = children[j];
                                            if n.kind() == "symbol" {
                                                let sym = state.text(n).to_string();
                                                if let Some((step, alter)) =
                                                    parse_pitch_name(&sym, state.language)
                                                {
                                                    let mut rp = Pitch::with_alter(step, alter, 3);
                                                    j += 1;
                                                    while j < children.len() {
                                                        let m = children[j];
                                                        if m.kind() == "punctuation" {
                                                            let t = state.text(m);
                                                            if t == "'" {
                                                                octave_marks += 1;
                                                                j += 1;
                                                            } else if t == "," {
                                                                octave_marks -= 1;
                                                                j += 1;
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
                                                }
                                                break;
                                            } else if n.kind() == "punctuation" {
                                                let t = state.text(n);
                                                if t == "'" || t == "," {
                                                    j += 1;
                                                    continue;
                                                }
                                                break;
                                            } else {
                                                break;
                                            }
                                        }
                                        continue;
                                    }
                                    if ew == "\\figuremode" || ew == "\\figures" {
                                        is_figuremode = true;
                                        j += 1;
                                        continue;
                                    }
                                    if ew == "\\chordmode" || ew == "\\chords" {
                                        is_chordmode = true;
                                        j += 1;
                                        continue;
                                    }
                                }
                                break;
                            }
                            // For chordmode variables, also capture the harmonies
                            // (the block is still parsed as music below so that
                            // referencing it in a Voice context still yields notes).
                            if is_chordmode {
                                if let Some(blk) = children.get(j) {
                                    if blk.kind() == "expression_block" {
                                        let entries = parse_chordmode_block(state, *blk);
                                        state.harmony_definitions.insert(var_name.clone(), entries);
                                    }
                                }
                            }
                            if is_markup {
                                // Skip the whole markup definition; advance i past it.
                                i = j;
                                continue;
                            }
                            if let Some(next) = children.get(j) {
                                if next.kind() == "expression_block" && is_figuremode {
                                    // Parse figuremode block into figured bass entries
                                    let fb_measures = parse_figuremode_block(state, *next);
                                    state
                                        .definitions
                                        .insert(var_name, VarDef::FiguredBass(fb_measures));
                                    i = j + 1;
                                    continue;
                                } else if next.kind() == "expression_block" {
                                    if is_lyricmode {
                                        // Parse lyrics variable
                                        let lyrics = parse_lyric_block(state, *next);
                                        state.lyric_definitions.insert(var_name, lyrics);
                                    } else {
                                        // A block holding `\new Staff` defines parts;
                                        // anything else is positioned music.
                                        let has_named_context =
                                            block_contains_named_context(state, *next);
                                        let block = *next;
                                        let (def, ()) =
                                            capture_variable(state, has_named_context, |state| {
                                                walk_music_block(state, block)
                                            });
                                        state.definitions.insert(var_name, def);
                                    }
                                    i = j + 1; // skip to after block
                                    continue;
                                } else if next.kind() == "named_context" {
                                    // Variable is `name = \new Staff { ... }`
                                    let node = *next;
                                    let (def, end) = capture_variable(state, true, |state| {
                                        let (context, ctx_name) =
                                            extract_named_context(state, node);
                                        walk_context_body(
                                            state,
                                            &children,
                                            j + 1,
                                            &context,
                                            &ctx_name,
                                        )
                                    });
                                    state.definitions.insert(var_name, def);
                                    i = end;
                                    continue;
                                }
                            }
                        }
                    }
                }
            }
            _ => {}
        }
        i += 1;
    }
}

/// Walk a `\header { ... }` block.
pub(super) fn walk_header(state: &mut WalkState, block: Node) {
    let mut cursor = block.walk();
    let children: Vec<Node> = block.children(&mut cursor).collect();
    let mut i = 0;

    while i < children.len() {
        let node = children[i];
        if node.kind() == "assignment_lhs" {
            let key = {
                // assignment_lhs has a child symbol
                let mut c = node.walk();
                let result = node
                    .children(&mut c)
                    .find(|n| n.kind() == "symbol")
                    .map(|n| state.text(n).to_string())
                    .unwrap_or_default();
                result
            };

            // Skip the "=" punctuation
            // Then find the next string value
            let mut j = i + 1;
            while j < children.len() {
                let val_node = children[j];
                if val_node.kind() == "string" {
                    let val = extract_string_value(state, val_node);
                    match key.as_str() {
                        "title" => state.metadata.title = Some(val),
                        "subtitle" => state.metadata.subtitle = Some(val),
                        "composer" => state.metadata.composer = Some(val),
                        "arranger" => state.metadata.arranger = Some(val),
                        "poet" | "lyricist" => state.metadata.lyricist = Some(val),
                        k if !k.is_empty() => {
                            state.metadata.extra.insert(k.to_string(), val);
                        }
                        _ => {}
                    }
                    i = j;
                    break;
                } else if val_node.kind() == "assignment_lhs" || val_node.kind() == "}" {
                    break;
                }
                j += 1;
            }
        }
        i += 1;
    }
}

/// Walk a `\score { ... }` block.
pub(super) fn walk_score_block(state: &mut WalkState, block: Node) {
    let mut cursor = block.walk();
    let children: Vec<Node> = block.children(&mut cursor).collect();
    let mut i = 0;

    while i < children.len() {
        let node = children[i];
        match node.kind() {
            "named_context" => {
                // \new Staff ..., \new Voice ..., etc.
                let (context, name) = extract_named_context(state, node);
                i += 1;
                // Check what follows: \relative, expression_block, etc.
                i = walk_context_body(state, &children, i, &context, &name);
                continue; // walk_context_body already advanced i
            }
            "escaped_word" => {
                let text = state.text(node).to_string();
                match text.as_str() {
                    "\\relative" => {
                        state.in_relative = true;
                        state.mode = PitchMode::Relative;
                        i += 1;
                        // May be followed by a reference pitch, then expression_block
                        i = consume_relative(state, &children, i);
                        continue;
                    }
                    "\\transpose" => {
                        i += 1;
                        i = consume_transpose(state, &children, i);
                        continue;
                    }
                    "\\layout" | "\\midi" => {
                        // Skip these blocks
                        if let Some(next) = children.get(i + 1) {
                            if next.kind() == "expression_block" {
                                i += 1;
                            }
                        }
                    }
                    "\\addlyrics" => {
                        // `MUSIC \addlyrics { ... }` — lyrics attach to the music
                        // expression that immediately precedes. Flush the pending
                        // measure so the preceding notes land in the part, then
                        // attach the syllables to that (most recent) part.
                        if let Some(next) = children.get(i + 1) {
                            if next.kind() == "expression_block" {
                                let syllables = parse_lyric_block(state, *next);
                                state.flush_voice();
                                let uid = state.current_uid();
                                state.added_lyrics.push((uid, syllables));
                                i += 1;
                            }
                        }
                    }
                    "\\unfoldRepeats" => {
                        // Transparent wrapper — just skip the keyword
                    }
                    "\\paper" => {
                        // \paper { ... } inside \score
                        if let Some(next) = children.get(i + 1) {
                            if next.kind() == "expression_block" {
                                parse_paper_block(state, *next);
                                i += 1;
                            }
                        }
                    }
                    "\\header" => {
                        if let Some(next) = children.get(i + 1) {
                            if next.kind() == "expression_block" {
                                walk_header(state, *next);
                                i += 1;
                            }
                        }
                    }
                    "\\set" | "\\override" | "\\revert" => {
                        // \set Context.prop = value / \override / \revert
                        // Skip tokens until the next escaped_word, named_context,
                        // expression_block, or parallel_music.
                        i += 1;
                        while i < children.len() {
                            let peek = children[i].kind();
                            if matches!(
                                peek,
                                "escaped_word"
                                    | "named_context"
                                    | "expression_block"
                                    | "parallel_music"
                            ) {
                                break;
                            }
                            i += 1;
                        }
                        continue;
                    }
                    _ => {
                        let var_name = text.trim_start_matches('\\');
                        state.resolve_variable(var_name);
                    }
                }
            }
            "expression_block" => {
                // Music inside score block without explicit context
                walk_music_block(state, node);
            }
            "parallel_music" => {
                walk_parallel_music(state, node);
            }
            _ => {}
        }
        i += 1;
    }
}

/// Walk a `<< ... >>` parallel music block.
///
/// If the block contains `\\` separators (voice splitting within one staff),
/// delegates to `walk_parallel_music_voices` which parses each voice branch
/// independently and merges them measure-by-measure.
///
/// Otherwise delegates to `walk_parallel_music_staves` which handles
/// simultaneous staves/contexts (the original behaviour).
pub(super) fn walk_parallel_music(state: &mut WalkState, node: Node) {
    let mut cursor = node.walk();
    let children: Vec<Node> = node.children(&mut cursor).collect();

    let has_voice_separator = children
        .iter()
        .any(|c| c.kind() == "parallel_music_separator");

    if has_voice_separator {
        walk_parallel_music_voices(state, &children);
    } else {
        walk_parallel_music_staves(state, &children);
    }
}

/// Walk a `<< ... >>` block that contains `\\` voice separators.
///
/// Each branch starts where the block starts and writes its own voice lane
/// (branch *k* → voice *k*); the block ends where its longest branch ends.
fn walk_parallel_music_voices(state: &mut WalkState, children: &[Node]) {
    state.flush_voice();
    let start = state.pos;
    let saved_prev_pitch = state.prev_pitch;
    let saved_relative_ref = state.relative_ref;
    let saved_in_relative = state.in_relative;
    let saved_stem_direction = state.stem_direction.clone();
    let saved_voice_number = state.current_voice_number;
    let saved_last_duration = state.last_duration.clone();
    let saved_tuplet_stack = state.tuplet_stack.clone();
    let saved_auto_beam_off = state.auto_beam_off;

    // Split children at parallel_music_separator nodes into voice branches
    let mut branches: Vec<Vec<usize>> = vec![vec![]]; // indices into children
    for (idx, child) in children.iter().enumerate() {
        if child.kind() == "parallel_music_separator" {
            branches.push(vec![]);
        } else {
            branches.last_mut().unwrap().push(idx);
        }
    }
    branches.retain(|b| !b.is_empty());

    let mut end = start;
    for (voice_idx, branch_indices) in branches.iter().enumerate() {
        state.set_pos(start);
        // LilyPond relative mode: within << { v1 } \\ { v2 } >>, pitch context
        // flows sequentially — voice 2 starts from voice 1's last note, etc.
        // Only the first voice resets to saved_prev_pitch; subsequent voices
        // continue from the previous voice's ending pitch. This matches
        // python-ly / quickly's left-to-right sequential processing.
        if voice_idx == 0 {
            state.prev_pitch = saved_prev_pitch;
        }
        state.relative_ref = saved_relative_ref;
        state.in_relative = saved_in_relative;
        state.stem_direction = saved_stem_direction.clone();
        state.current_voice_number = (voice_idx + 1) as u8;
        state.last_duration = saved_last_duration.clone();
        state.tuplet_stack = saved_tuplet_stack.clone();
        state.auto_beam_off = saved_auto_beam_off;

        walk_voice_branch(state, children, branch_indices);
        state.flush_voice();
        end = end.max(state.pos);
    }

    // prev_pitch is NOT restored — it continues from the last voice's final
    // note (LilyPond/python-ly's left-to-right pitch processing).
    state.relative_ref = saved_relative_ref;
    state.in_relative = saved_in_relative;
    state.stem_direction = saved_stem_direction;
    state.current_voice_number = saved_voice_number;
    state.tuplet_stack = saved_tuplet_stack;
    state.auto_beam_off = saved_auto_beam_off;
    state.set_pos(end);
}

/// Walk the children of a single voice branch within `<< \\ >>`.
///
/// `branch_indices` contains indices into `children` for this voice's nodes.
fn walk_voice_branch(state: &mut WalkState, children: &[Node], branch_indices: &[usize]) {
    let mut bi = 0;
    while bi < branch_indices.len() {
        let ci = branch_indices[bi];
        let child = children[ci];
        match child.kind() {
            "named_context" => {
                let (context, name) = extract_named_context(state, child);
                // Collect the remaining children indices for context body consumption
                let remaining: Vec<Node> = branch_indices[bi + 1..]
                    .iter()
                    .map(|&idx| children[idx])
                    .collect();
                let consumed = walk_context_body(state, &remaining, 0, &context, &name);
                bi += 1 + consumed;
                continue;
            }
            "expression_block" => {
                walk_music_block(state, child);
            }
            "parallel_music" => {
                walk_parallel_music(state, child);
            }
            "escaped_word" => {
                let text = state.text(child).to_string();
                if text == "\\relative" {
                    state.in_relative = true;
                    state.mode = PitchMode::Relative;
                    // consume_relative expects a slice; build one from remaining branch nodes
                    let remaining: Vec<Node> = branch_indices[bi + 1..]
                        .iter()
                        .map(|&idx| children[idx])
                        .collect();
                    let consumed = consume_relative(state, &remaining, 0);
                    bi += 1 + consumed;
                    continue;
                } else if text == "\\transpose" {
                    let remaining: Vec<Node> = branch_indices[bi + 1..]
                        .iter()
                        .map(|&idx| children[idx])
                        .collect();
                    let consumed = consume_transpose(state, &remaining, 0);
                    bi += 1 + consumed;
                    continue;
                } else if text == "\\unfoldRepeats" {
                    // Transparent wrapper — just skip the keyword
                } else if matches!(text.as_str(), "\\set" | "\\override" | "\\revert") {
                    // Skip property assignments
                    bi += 1;
                    while bi < branch_indices.len() {
                        let peek = children[branch_indices[bi]].kind();
                        if matches!(
                            peek,
                            "escaped_word"
                                | "named_context"
                                | "expression_block"
                                | "parallel_music"
                        ) {
                            break;
                        }
                        bi += 1;
                    }
                    continue;
                } else {
                    let var_name = text.trim_start_matches('\\');
                    state.resolve_variable(var_name);
                }
            }
            _ => {}
        }
        bi += 1;
    }
}

/// Walk a `<< ... >>` block that does NOT contain `\\` separators: its children
/// (staves, voices, blocks, variables) all start where the block starts. Music
/// sharing a staff overlays by position, each branch in its own voice lane.
fn walk_parallel_music_staves(state: &mut WalkState, children: &[Node]) {
    let mut i = 0;
    // Track parts that existed before this block so we can apply deferred
    // \set properties to all parts created within it.
    let parts_before = state.parts.len();
    let mut deferred_props: Vec<(String, String)> = Vec::new();

    state.flush_voice();
    let start = state.pos;
    let lane = state.current_voice_number;
    let saved_origin = std::mem::replace(&mut state.origin, start);
    let mut end = start;
    // Every branch starts at the block's start, in the lane it was entered in.
    let begin_branch = |state: &mut WalkState| {
        state.set_pos(start);
        state.current_voice_number = lane;
    };

    while i < children.len() {
        let child = children[i];
        match child.kind() {
            "named_context" => {
                let (context, name) = extract_named_context(state, child);
                begin_branch(state);
                i = walk_context_body(state, children, i + 1, &context, &name);
                state.flush_voice();
                end = end.max(state.pos);
                continue;
            }
            "expression_block" | "parallel_music" => {
                begin_branch(state);
                if child.kind() == "expression_block" {
                    walk_music_block(state, child);
                } else {
                    walk_parallel_music(state, child);
                }
                state.flush_voice();
                end = end.max(state.pos);
            }
            "escaped_word" => {
                let text = state.text(child).to_string();
                if text == "\\relative" || text == "\\transpose" {
                    begin_branch(state);
                    if text == "\\relative" {
                        state.in_relative = true;
                        state.mode = PitchMode::Relative;
                        i = consume_relative(state, children, i + 1);
                    } else {
                        i = consume_transpose(state, children, i + 1);
                    }
                    state.flush_voice();
                    end = end.max(state.pos);
                    continue;
                } else if text == "\\unfoldRepeats" {
                    // Transparent wrapper — just skip the keyword
                } else if text == "\\set" {
                    // Handle \set property assignments (e.g. midiInstrument)
                    // AST: escaped_word(\set) assignment_lhs punctuation(=) value
                    i += 1; // skip past \set
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
                                let val_opt = if val_node.kind() == "string" {
                                    let v =
                                        crate::adapters::ly_to_ir::consume::extract_string_value(
                                            state, *val_node,
                                        );
                                    i += 1;
                                    Some(v)
                                } else if val_node.kind() == "embedded_scheme" {
                                    let scheme_text = state.text(*val_node).to_string();
                                    i += 1;
                                    crate::adapters::ly_to_ir::consume::extract_scheme_string(
                                        &scheme_text,
                                    )
                                } else {
                                    i += 1;
                                    None
                                };
                                if let Some(val) = val_opt {
                                    // Check if this is a grouping-context property
                                    // (e.g. PianoStaff.midiInstrument) that should propagate
                                    // to child staves created later in this block.
                                    let ctx_prefix = prop_text.split('.').next().unwrap_or("");
                                    let is_grouping = matches!(
                                        ctx_prefix,
                                        "PianoStaff" | "GrandStaff" | "StaffGroup" | "ChoirStaff"
                                    );
                                    if !is_grouping && !state.parts.is_empty() {
                                        // Staff-level \set — apply to existing current part
                                        crate::adapters::ly_to_ir::music::apply_set_property(
                                            state, &prop_text, &val,
                                        );
                                    } else {
                                        // Grouping-level or no part yet — defer
                                        deferred_props.push((prop_text, val));
                                    }
                                }
                            }
                        }
                    }
                    continue;
                } else if matches!(text.as_str(), "\\override" | "\\revert") {
                    // Skip property assignments
                    i += 1;
                    while i < children.len() {
                        let peek = children[i].kind();
                        if matches!(
                            peek,
                            "escaped_word"
                                | "named_context"
                                | "expression_block"
                                | "parallel_music"
                        ) {
                            break;
                        }
                        i += 1;
                    }
                    continue;
                } else if text == "\\addlyrics" {
                    // `STAFF \addlyrics { ... }` inside <<...>>: lyrics attach
                    // to the preceding staff. Without this case the lyric
                    // block fell through to the music walker and syllables
                    // that are valid pitch names became phantom notes.
                    if let Some(next) = children.get(i + 1) {
                        if next.kind() == "expression_block" {
                            let syllables = parse_lyric_block(state, *next);
                            state.flush_voice();
                            let uid = state.current_uid();
                            state.added_lyrics.push((uid, syllables));
                            i += 1;
                        }
                    }
                } else {
                    // A plain music variable is a simultaneous branch.
                    begin_branch(state);
                    state.resolve_variable(text.trim_start_matches('\\'));
                    state.flush_voice();
                    end = end.max(state.pos);
                }
            }
            _ => {}
        }
        i += 1;
    }

    // Apply deferred \set properties to all parts created in this block
    if !deferred_props.is_empty() {
        for pb in state.parts[parts_before..].iter_mut() {
            for (prop, val) in &deferred_props {
                let prop_name = prop.split('.').next_back().unwrap_or(prop);
                match prop_name {
                    "instrumentName" => pb.part.name = val.clone(),
                    "shortInstrumentName" => pb.part.abbreviation = val.clone(),
                    "midiInstrument" => pb.part.midi_instrument = val.clone(),
                    _ => {}
                }
            }
        }
    }

    state.origin = saved_origin;
    state.current_voice_number = lane;
    state.set_pos(end);
}

/// Extract context type and context name from a `named_context` node.
/// For `\new Staff`, returns ("Staff", "").
/// For `\context Voice = "melodySop"`, returns ("Voice", "melodySop").
pub(super) fn extract_named_context<'a>(state: &WalkState<'a>, node: Node<'a>) -> (String, String) {
    state
        .context_reentry
        .set(state.text(node).trim_start().starts_with("\\context"));
    let mut context = String::new();
    let mut name = String::new();
    let mut cursor = node.walk();
    let children: Vec<_> = node.children(&mut cursor).collect();
    let mut found_symbol = false;
    let mut found_eq = false;
    for child in &children {
        match child.kind() {
            "symbol" if !found_symbol => {
                context = state.text(*child).to_string();
                found_symbol = true;
            }
            "punctuation" if found_symbol && state.text(*child) == "=" => {
                found_eq = true;
            }
            "string" if found_eq => {
                name = extract_string_value(state, *child);
            }
            _ => {}
        }
    }
    (context, name)
}

/// After parsing a `named_context` node, consume the body (which might be
/// `\relative { ... }`, `{ ... }`, `\with { ... } { ... }`, etc.).
/// Returns the next index to process.
pub(super) fn walk_context_body(
    state: &mut WalkState,
    children: &[Node],
    mut i: usize,
    context: &str,
    name: &str,
) -> usize {
    // Lyrics context: don't create a music part, parse lyrics instead
    if context == "Lyrics" {
        return walk_lyrics_context(state, children, i, name);
    }

    // Voice context: no new part — the voice's music goes into the current
    // staff (its own lane when it overlaps music already there).
    if context == "Voice" {
        i = skip_with_block(state, children, i);
        i = walk_body(state, children, i);
        if !name.is_empty() {
            let uid = state.current_uid();
            state.voice_part_map.insert(name.to_string(), uid);
        }
        return i;
    }

    // NullVoice: invisible notes that only carry lyric timing. Walk them for
    // their side effects, then drop them; lyrics sent to it land on the staff.
    if context == "NullVoice" {
        i = skip_with_block(state, children, i);
        state.flush_voice();
        let host = state.current_uid();
        let (pos, voice_start) = (state.pos, state.voice_start);
        let before = state.parts.len();
        state.new_part("NullVoice", "");
        state.pos = pos;
        state.voice_start = pos;
        i = walk_body(state, children, i);
        state.flush_voice();
        let end = state.pos;
        state.parts.truncate(before);
        state.pos = end;
        state.voice_start = voice_start.max(end);
        if !name.is_empty() {
            state.voice_part_map.insert(name.to_string(), host);
        }
        return i;
    }

    // Dynamics: a lane of spacers carrying dynamics, hairpins and pedals. It is
    // its own part until assembly folds it into the staff it belongs to.
    if context == "Dynamics" {
        i = skip_with_block(state, children, i);
        state.new_part(context, name);
        return walk_body(state, children, i);
    }

    // ChordNames context: parse chordmode harmonies (inline or via variable) and
    // queue them to be attached to the melody part once the score is assembled.
    // Does not create a part of its own.
    if context == "ChordNames" {
        // Skip optional \with { ... }
        while i < children.len() {
            let node = children[i];
            if node.kind() == "escaped_word" && state.text(node) == "\\with" {
                if let Some(next) = children.get(i + 1) {
                    if next.kind() == "expression_block" {
                        i += 2;
                        continue;
                    }
                }
            }
            break;
        }
        // Body: `\chordmode { ... }` | `\chords { ... }` | `{ ... }` | `\var`
        if let Some(node) = children.get(i) {
            match node.kind() {
                "escaped_word" => {
                    let text = state.text(*node).to_string();
                    if text == "\\chordmode" || text == "\\chords" {
                        i += 1;
                        if let Some(blk) = children.get(i) {
                            if blk.kind() == "expression_block" {
                                let entries = parse_chordmode_block(state, *blk);
                                state.pending_harmonies.extend(entries);
                                i += 1;
                            }
                        }
                    } else {
                        // Variable reference: pull harmonies if the variable was a chordmode.
                        let var_name = text.trim_start_matches('\\');
                        if let Some(entries) = state.harmony_definitions.get(var_name) {
                            let entries = entries.clone();
                            state.pending_harmonies.extend(entries);
                        }
                        i += 1;
                    }
                }
                "expression_block" => {
                    let entries = parse_chordmode_block(state, *node);
                    state.pending_harmonies.extend(entries);
                    i += 1;
                }
                _ => {
                    i += 1;
                }
            }
        }
        return i;
    }

    // Grouping contexts (ChoirStaff, StaffGroup, etc.) don't produce a part;
    // they just wrap inner staves. Walk their body like a score block.
    let is_grouping = matches!(
        context,
        "ChoirStaff" | "StaffGroup" | "GrandStaff" | "PianoStaff"
    );

    if is_grouping {
        let is_piano_staff = matches!(context, "PianoStaff" | "GrandStaff");
        let mut with_props = std::collections::HashMap::new();

        // Parse optional \with { ... }
        while i < children.len() {
            let node = children[i];
            if node.kind() == "escaped_word" && state.text(node) == "\\with" {
                if let Some(next) = children.get(i + 1) {
                    if next.kind() == "expression_block" {
                        with_props = parse_with_block(state, *next);
                        i += 2;
                        continue;
                    }
                }
            }
            break;
        }

        state.flush_voice();
        let parts_before = state.parts.len();

        // Consume the body block (parallel music or expression_block)
        if let Some(node) = children.get(i) {
            match node.kind() {
                "parallel_music" => {
                    walk_parallel_music(state, *node);
                    i += 1;
                }
                "expression_block" => {
                    walk_score_block(state, *node);
                    i += 1;
                }
                _ => {}
            }
        }

        // For PianoStaff/GrandStaff: combine the newly created parts into one
        // multi-staff part. The function counts only real Staff parts as staves
        // and folds any interleaved Dynamics contexts in as directions.
        state.flush_voice();
        if is_piano_staff && state.parts.len() > parts_before {
            merge_piano_staff_parts(state, parts_before, &with_props);
        }

        return i;
    }

    // Remember old relative state
    let was_relative = state.in_relative;
    let old_ref = state.relative_ref;
    let old_prev = state.prev_pitch;

    // `\context Staff` inside that staff continues it; `\new Staff` starts one.
    let reenter = state.context_reentry.take()
        && state
            .parts
            .last()
            .is_some_and(|pb| pb.context == context && (name.is_empty() || pb.part.name == name));
    if !reenter {
        state.new_part(context, name);
    }

    while i < children.len() {
        let node = children[i];
        match node.kind() {
            "escaped_word" => {
                let text = state.text(node).to_string();
                match text.as_str() {
                    "\\relative" => {
                        state.in_relative = true;
                        state.mode = PitchMode::Relative;
                        i += 1;
                        i = consume_relative(state, children, i);
                        continue;
                    }
                    "\\transpose" => {
                        i += 1;
                        i = consume_transpose(state, children, i);
                        continue;
                    }
                    "\\with" => {
                        // Parse \with { ... } for known properties
                        if let Some(next) = children.get(i + 1) {
                            if next.kind() == "expression_block" {
                                let props = parse_with_block(state, *next);
                                if let Some(part) = state.parts.last_mut().map(|pb| &mut pb.part) {
                                    if let Some(v) = props.get("instrumentName") {
                                        part.name = v.clone();
                                    }
                                    if let Some(v) = props.get("shortInstrumentName") {
                                        part.abbreviation = v.clone();
                                    }
                                    if let Some(v) = props.get("midiInstrument") {
                                        part.midi_instrument = v.clone();
                                    }
                                }
                                i += 2;
                                continue;
                            }
                        }
                    }
                    _ => {
                        let var_name = text.trim_start_matches('\\');
                        if state.resolve_variable(var_name) {
                            i += 1;
                        }
                        break;
                    }
                }
            }
            "expression_block" => {
                walk_music_block(state, node);
                i += 1;
                break;
            }
            "parallel_music" => {
                walk_parallel_music(state, node);
                i += 1;
                break;
            }
            _ => break,
        }
        i += 1;
    }

    // Restore relative state for other parts
    state.in_relative = was_relative;
    state.relative_ref = old_ref;
    state.prev_pitch = old_prev;

    i
}

/// Handle `\context Lyrics = "name" \lyricmode { \lyricsto "voice" ... }`
/// or `\new Lyrics \lyricsto "voice" \variable`
/// Consumes tokens after the named_context and stores lyrics for later attachment.
fn walk_lyrics_context(
    state: &mut WalkState,
    children: &[Node],
    mut i: usize,
    _name: &str,
) -> usize {
    // After named_context(Lyrics), we may see:
    //   1. \lyricmode { \lyricsto "voiceName" ... }
    //   2. \lyricsto "voiceName" \variable
    let mut is_lyricmode = false;
    let mut lyricsto_voice: Option<String> = None;

    while i < children.len() {
        let node = children[i];
        match node.kind() {
            "escaped_word" => {
                let text = state.text(node);
                if text == "\\lyricmode" {
                    is_lyricmode = true;
                    i += 1;
                    continue;
                }
                if text == "\\lyricsto" {
                    // \lyricsto "voiceName" — consume voice name
                    i += 1;
                    if let Some(name_node) = children.get(i) {
                        if name_node.kind() == "string" {
                            lyricsto_voice = Some(extract_string_value(state, *name_node));
                            i += 1;
                        }
                    }
                    continue;
                }
                // Could be a variable reference like \Itesto
                if let Some(voice) = &lyricsto_voice {
                    let var_name = text.trim_start_matches('\\');
                    if let Some(syllables) = state.lyric_definitions.get(var_name) {
                        state
                            .pending_lyrics
                            .insert(voice.clone(), syllables.clone());
                    }
                }
                i += 1;
                break;
            }
            "expression_block" => {
                if is_lyricmode || lyricsto_voice.is_some() {
                    // Parse the lyric block and find the \lyricsto voice name
                    let voice_name = if lyricsto_voice.is_some() {
                        lyricsto_voice.clone()
                    } else {
                        extract_lyricsto_voice(state, node)
                    };
                    let syllables = parse_lyric_block(state, node);
                    if let Some(voice) = voice_name {
                        state.pending_lyrics.insert(voice, syllables);
                    }
                }
                i += 1;
                break;
            }
            _ => break,
        }
    }
    i
}

/// Merge the parts created inside a PianoStaff/GrandStaff into a single
/// multi-staff part. `parts_before` is the index of the first new part.
///
/// Only real **Staff** parts become staves; interleaved **Dynamics** contexts
/// (spacer-only parts carrying dynamics/pedal/markup) are folded into the
/// combined part as directions, regardless of their position. This is
/// essential for scores like repeats.ly where a `\new Dynamics` lane appears
/// before/between the staves.
///
/// Everything is positioned, so the staves simply overlay: each staff's lanes
/// are renumbered past the staves before it and its elements and clefs are
/// stamped with its staff number.
fn merge_piano_staff_parts(
    state: &mut WalkState,
    parts_before: usize,
    with_props: &std::collections::HashMap<String, String>,
) {
    let window: Vec<PartBuild> = state.parts.drain(parts_before..).collect();
    let (mut dynamics, staves): (Vec<PartBuild>, Vec<PartBuild>) =
        window.into_iter().partition(part_is_dynamics_only);

    if staves.is_empty() {
        // Degenerate PianoStaff with no real staves — restore the parts.
        state.parts.extend(dynamics);
        return;
    }

    let num_staves = staves.len() as u8;
    let mut staves = staves.into_iter();
    let first = staves.next().unwrap();
    let mut base = PartBuild {
        tl: Timeline::default(),
        ..first.clone()
    };
    let mut lane_offset = 0u8;
    for (k, mut pb) in std::iter::once(first).chain(staves).enumerate() {
        pb.tl.fold_spacer_lanes();
        let top_lane = pb.tl.lanes.keys().copied().max().unwrap_or(0);
        base.tl.absorb(pb.tl.into_staff(k as u8 + 1, lane_offset));
        lane_offset = lane_offset.saturating_add(top_lane);
        if pb.uid != base.uid {
            state.part_alias.insert(pb.uid, base.uid);
        }
    }
    for d in &mut dynamics {
        d.tl.fold_spacer_lanes();
        base.tl.events.append(&mut d.tl.events);
        state.part_alias.insert(d.uid, base.uid);
    }
    base.part.staves = num_staves;

    // Apply \with properties, or derive name from the staff.
    if let Some(v) = with_props.get("instrumentName") {
        base.part.name = v.clone();
    } else if !base.part.name.is_empty() {
        // Strip trailing " N" suffix (e.g. "Piano 1" → "Piano")
        let name = base.part.name.trim_end();
        if let Some(pos) = name.rfind(' ') {
            let suffix = &name[pos + 1..];
            if suffix.chars().all(|c| c.is_ascii_digit()) {
                base.part.name = name[..pos].to_string();
            }
        }
    }
    if let Some(v) = with_props.get("shortInstrumentName") {
        base.part.abbreviation = v.clone();
    }
    if let Some(v) = with_props.get("midiInstrument") {
        base.part.midi_instrument = v.clone();
    }

    // Assign each direction to a staff with the right placement for a piano
    // grand staff (pedal below the bottom staff; dynamics/hairpins below the
    // top staff). Note-attached dynamics are emitted separately.
    for (_, ev) in &mut base.tl.events {
        if let Event::Direction(dir) = ev {
            assign_piano_direction_staff(dir, num_staves);
        }
    }

    // Put the combined part back where the window started.
    state.parts.insert(parts_before, base);
}

/// Skip an optional `\with { ... }` block at `i`.
fn skip_with_block(state: &WalkState, children: &[Node], mut i: usize) -> usize {
    while i + 1 < children.len()
        && children[i].kind() == "escaped_word"
        && state.text(children[i]) == "\\with"
        && children[i + 1].kind() == "expression_block"
    {
        i += 2;
    }
    i
}

/// Walk a context's body — `{ … }`, `<< … >>`, `\relative`/`\transpose …`, or
/// a variable — into the current part. Returns the index after it.
fn walk_body(state: &mut WalkState, children: &[Node], i: usize) -> usize {
    let Some(&node) = children.get(i) else {
        return i;
    };
    match node.kind() {
        "expression_block" => {
            walk_music_block(state, node);
            i + 1
        }
        "parallel_music" => {
            walk_parallel_music(state, node);
            i + 1
        }
        "escaped_word" => {
            let text = state.text(node).to_string();
            match text.as_str() {
                "\\relative" => {
                    state.in_relative = true;
                    state.mode = PitchMode::Relative;
                    consume_relative(state, children, i + 1)
                }
                "\\transpose" => consume_transpose(state, children, i + 1),
                _ if state.resolve_variable(text.trim_start_matches('\\')) => i + 1,
                _ => i,
            }
        }
        _ => i,
    }
}

/// Walk a variable's body in isolation, from position 0, and capture what it
/// defines: the parts it built (`as_parts`), or its music as a timeline.
fn capture_variable<R>(
    state: &mut WalkState,
    as_parts: bool,
    walk: impl FnOnce(&mut WalkState) -> R,
) -> (VarDef, R) {
    state.flush_voice();
    let saved_parts = std::mem::take(&mut state.parts);
    let saved_voices = std::mem::take(&mut state.voice_part_map);
    let saved_pos = (state.pos, state.voice_start, state.origin);
    let (old_relative, old_mode, old_ref, old_prev) = (
        state.in_relative,
        state.mode,
        state.relative_ref,
        state.prev_pitch,
    );
    let main_lane = state.current_voice_number;
    state.pos = Frac::from_integer(0);
    state.voice_start = state.pos;
    state.origin = state.pos;

    let result = walk(state);
    state.flush_voice();
    let len = state.pos;

    let parts = std::mem::replace(&mut state.parts, saved_parts);
    let voices = std::mem::replace(&mut state.voice_part_map, saved_voices);
    (state.pos, state.voice_start, state.origin) = saved_pos;
    // Restore state so variable definitions don't leak context
    state.in_relative = old_relative;
    state.mode = old_mode;
    state.relative_ref = old_ref;
    state.prev_pitch = old_prev;
    state.current_voice_number = main_lane;

    let def = if as_parts {
        let index = voices
            .into_iter()
            .filter_map(|(v, uid)| Some((v, parts.iter().position(|p| p.uid == uid)?)))
            .collect();
        VarDef::Parts(parts, index)
    } else {
        let mut tl = Timeline::default();
        for pb in parts {
            tl.absorb(pb.tl);
        }
        VarDef::Music {
            tl,
            len,
            main_lane,
            voices: voices.into_keys().collect(),
        }
    };
    (def, result)
}
