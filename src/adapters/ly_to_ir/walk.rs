use std::collections::HashMap;

use tree_sitter::Node;

use crate::ir::duration::Frac;
use crate::ir::language::{parse_pitch_name, PitchLanguage, PitchMode};
use crate::ir::measure::MeasureAttributes;
use crate::ir::pitch::Pitch;
use crate::ir::score::{Score, ScoreChild};

use super::consume::{
    block_contains_named_context, extract_string_value, parse_paper_block,
    score_block_output_types,
};
use super::figured_bass::parse_figuremode_block;
use super::lyrics::{attach_lyrics_to_part, extract_lyricsto_voice, parse_lyric_block};
use super::merge::{
    merge_dynamics_parts, merge_leading_attribute_measures, merge_voice_measure_streams,
    propagate_first_tempo, renumber_voices_in_measures, synchronize_time_signatures,
};
use super::modifiers::{consume_relative, consume_transpose};
use super::music::walk_music_block;
use super::state::WalkState;
use super::VarDef;

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
                                state.language = match lang_str.as_str() {
                                    "english" => PitchLanguage::English,
                                    "deutsch" | "german" => PitchLanguage::Deutsch,
                                    "italiano" | "italian" => PitchLanguage::Italiano,
                                    "espanol" | "español" | "spanish" => PitchLanguage::Espanol,
                                    "français" | "francais" | "french" => PitchLanguage::Nederlands, // no dedicated French; default
                                    "portugues" | "português" | "portuguese" => {
                                        PitchLanguage::Portugues
                                    }
                                    "vlaams" | "flemish" => PitchLanguage::Vlaams,
                                    "norsk" | "norwegian" => PitchLanguage::Norsk,
                                    "suomi" | "finnish" => PitchLanguage::Suomi,
                                    "svenska" | "swedish" => PitchLanguage::Svenska,
                                    "catalan" => PitchLanguage::Catalan,
                                    _ => PitchLanguage::Nederlands,
                                };
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
                                // Save and reset parts for this score block
                                state.flush_measure();
                                let saved_parts = std::mem::take(&mut state.parts);
                                let saved_counter = state.part_counter;
                                let saved_pending_lyrics = std::mem::take(&mut state.pending_lyrics);
                                let saved_voice_map = std::mem::take(&mut state.voice_part_map);
                                state.part_counter = 0;
                                state.measure_num = 0;
                                state.elapsed_in_measure = Frac::from_integer(0);

                                walk_score_block(state, *next);
                                state.flush_measure();

                                // Attach pending lyrics
                                for (voice_name, syllables) in &state.pending_lyrics {
                                    if let Some(&part_idx) = state.voice_part_map.get(voice_name) {
                                        if let Some((_, part)) = state.parts.get_mut(part_idx) {
                                            attach_lyrics_to_part(part, syllables);
                                        }
                                    }
                                }

                                // Merge spacer-only parts (from \new Dynamics) into staff parts
                                merge_dynamics_parts(&mut state.parts);

                                // Build a Score from the parts created by this score block
                                if !state.parts.is_empty() {
                                    let mut score = Score::new();
                                    score.metadata = state.metadata.clone();
                                    score.metadata.pitch_mode = state.mode;
                                    score.metadata.pitch_language = Some(state.language);
                                    score.page_layout = state.page_layout.clone();
                                    for (_, part) in state.parts.drain(..) {
                                        score.children.push(ScoreChild::Part(part));
                                    }
                                    // Post-process: merge attribute-only measures first so indices align, then sync time sigs
                                    for part in score.parts_mut() {
                                        merge_leading_attribute_measures(part);
                                    }
                                    synchronize_time_signatures(&mut score);
                                    propagate_first_tempo(&mut score);
                                    state.completed_scores.push(score);
                                }

                                // Restore saved state
                                state.parts = saved_parts;
                                state.part_counter = saved_counter;
                                state.pending_lyrics = saved_pending_lyrics;
                                state.voice_part_map = saved_voice_map;
                                state.measure_num = 0;

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
                            while j < children.len() {
                                let candidate = children[j];
                                if candidate.kind() == "escaped_word" {
                                    let ew = state.text(candidate);
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
                                                if let Some((step, alter)) = parse_pitch_name(&sym, state.language) {
                                                    let mut rp = Pitch::with_alter(step, alter, 3);
                                                    j += 1;
                                                    while j < children.len() {
                                                        let m = children[j];
                                                        if m.kind() == "punctuation" {
                                                            let t = state.text(m);
                                                            if t == "'" { octave_marks += 1; j += 1; }
                                                            else if t == "," { octave_marks -= 1; j += 1; }
                                                            else { break; }
                                                        } else { break; }
                                                    }
                                                    rp.octave = 3 + octave_marks;
                                                    state.relative_ref = Some(rp);
                                                    state.prev_pitch = Some(rp);
                                                }
                                                break;
                                            } else if n.kind() == "punctuation" {
                                                let t = state.text(n);
                                                if t == "'" || t == "," { j += 1; continue; }
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
                                }
                                break;
                            }
                            if let Some(next) = children.get(j) {
                                if next.kind() == "expression_block" && is_figuremode {
                                    // Parse figuremode block into figured bass entries
                                    let fb_measures = parse_figuremode_block(state, *next);
                                    state.definitions.insert(var_name, VarDef::FiguredBass(fb_measures));
                                    i = j + 1;
                                    continue;
                                } else if next.kind() == "expression_block" {
                                    if is_lyricmode {
                                        // Parse lyrics variable
                                        let lyrics = parse_lyric_block(state, *next);
                                        state.lyric_definitions.insert(var_name, lyrics);
                                    } else {
                                        // Check if the block contains a \new Staff/PianoStaff
                                        let has_named_context =
                                            block_contains_named_context(state, *next);
                                        // Walk the block but capture the parts it creates
                                        let parts_before = state.parts.len();
                                        let old_measure_num = state.measure_num;
                                        let old_relative = state.in_relative;
                                        let old_mode = state.mode;
                                        let old_relative_ref = state.relative_ref;
                                        let old_prev_pitch = state.prev_pitch;
                                        let old_elapsed = state.elapsed_in_measure;
                                        let old_time_sig = state.current_time_sig;
                                        state.measure_num = 0;
                                        state.elapsed_in_measure = Frac::from_integer(0);
                                        walk_music_block(state, *next);
                                        state.flush_measure();
                                        // Restore state so variable definitions
                                        // don't leak context
                                        state.in_relative = old_relative;
                                        state.mode = old_mode;
                                        state.relative_ref = old_relative_ref;
                                        state.prev_pitch = old_prev_pitch;
                                        state.elapsed_in_measure = old_elapsed;
                                        state.current_time_sig = old_time_sig;
                                        // Extract newly created parts
                                        let new_parts: Vec<_> =
                                            state.parts.drain(parts_before..).collect();
                                        // Capture voice→part mappings created during this variable def
                                        let local_voice_map: HashMap<String, usize> = state
                                            .voice_part_map
                                            .iter()
                                            .filter(|(_, idx)| **idx >= parts_before)
                                            .map(|(name, idx)| (name.clone(), idx - parts_before))
                                            .collect();
                                        if !local_voice_map.is_empty() {
                                            state.var_voice_maps.insert(var_name.clone(), local_voice_map);
                                        }
                                        // Remove stale entries from voice_part_map
                                        state.voice_part_map.retain(|_, idx| *idx < parts_before);
                                        // If the block explicitly contained \new Staff,
                                        // store as full Parts to preserve metadata
                                        let def = if has_named_context {
                                            VarDef::Parts(new_parts)
                                        } else {
                                            let measures: Vec<_> = new_parts
                                                .into_iter()
                                                .flat_map(|(_, part)| part.measures)
                                                .collect();
                                            VarDef::Measures(measures, old_time_sig)
                                        };
                                        state.definitions.insert(var_name, def);
                                        state.measure_num = old_measure_num;
                                    }
                                    i = j + 1; // skip to after block
                                    continue;
                                } else if next.kind() == "named_context" {
                                    // Variable is `name = \new Staff { ... }`
                                    // The named_context is followed by expression_block
                                    let parts_before = state.parts.len();
                                    let old_measure_num = state.measure_num;
                                    let old_elapsed = state.elapsed_in_measure;
                                    let old_time_sig = state.current_time_sig;
                                    state.measure_num = 0;
                                    state.elapsed_in_measure = Frac::from_integer(0);
                                    let (context, ctx_name) =
                                        extract_named_context(state, *next);
                                    j += 1;
                                    j = walk_context_body(
                                        state, &children, j, &context, &ctx_name,
                                    );
                                    state.flush_measure();
                                    let new_parts: Vec<_> =
                                        state.parts.drain(parts_before..).collect();
                                    // Capture voice→part mappings created during this variable def
                                    let local_voice_map: HashMap<String, usize> = state
                                        .voice_part_map
                                        .iter()
                                        .filter(|(_, idx)| **idx >= parts_before)
                                        .map(|(name, idx)| (name.clone(), idx - parts_before))
                                        .collect();
                                    if !local_voice_map.is_empty() {
                                        state.var_voice_maps.insert(var_name.clone(), local_voice_map);
                                    }
                                    // Remove stale entries from voice_part_map
                                    state.voice_part_map.retain(|_, idx| *idx < parts_before);
                                    state
                                        .definitions
                                        .insert(var_name, VarDef::Parts(new_parts));
                                    state.measure_num = old_measure_num;
                                    state.elapsed_in_measure = old_elapsed;
                                    state.current_time_sig = old_time_sig;
                                    i = j;
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
                let result = node.children(&mut c)
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
                            if matches!(peek, "escaped_word" | "named_context"
                                | "expression_block" | "parallel_music")
                            {
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
/// Each voice branch is parsed independently into its own `Vec<Measure>`,
/// then corresponding measures are merged so that each output measure
/// contains multiple `Voice` objects.
fn walk_parallel_music_voices(state: &mut WalkState, children: &[Node]) {
    // 1. Save state that each voice branch needs to start from
    let saved_measure_num = state.measure_num;
    let saved_elapsed = state.elapsed_in_measure;
    let saved_current_measure = state.current_measure.take();
    let saved_current_voice = std::mem::take(&mut state.current_voice);
    let saved_time_sig = state.current_time_sig;
    let saved_prev_pitch = state.prev_pitch;
    let saved_relative_ref = state.relative_ref;
    let saved_in_relative = state.in_relative;
    let saved_stem_direction = state.stem_direction.clone();
    let saved_voice_number = state.current_voice_number;
    let saved_last_duration = state.last_duration.clone();
    let saved_tuplet_stack = state.tuplet_stack.clone();
    let saved_auto_beam_off = state.auto_beam_off;

    // 2. Split children at parallel_music_separator nodes into voice branches
    let mut branches: Vec<Vec<usize>> = vec![vec![]]; // indices into children
    for (idx, child) in children.iter().enumerate() {
        if child.kind() == "parallel_music_separator" {
            branches.push(vec![]);
        } else {
            branches.last_mut().unwrap().push(idx);
        }
    }
    // Remove empty branches
    branches.retain(|b| !b.is_empty());

    // 3. Parse each voice branch independently
    let mut voice_streams: Vec<Vec<crate::ir::measure::Measure>> = Vec::new();

    for (voice_idx, branch_indices) in branches.iter().enumerate() {
        // Reset state for this voice branch
        state.measure_num = saved_measure_num;
        state.elapsed_in_measure = Frac::from_integer(0);
        state.current_measure = None;
        state.current_voice = Vec::new();
        state.current_time_sig = saved_time_sig;
        state.prev_pitch = saved_prev_pitch;
        state.relative_ref = saved_relative_ref;
        state.in_relative = saved_in_relative;
        state.stem_direction = saved_stem_direction.clone();
        state.current_voice_number = (voice_idx + 1) as u8;
        state.last_duration = saved_last_duration.clone();
        state.tuplet_stack = saved_tuplet_stack.clone();
        state.auto_beam_off = saved_auto_beam_off;

        // Record where the part's measures start so we can drain what this branch adds
        let part_measures_before = state.ensure_part().measures.len();

        // Walk the branch children
        walk_voice_branch(state, children, branch_indices);

        // Flush remaining state
        state.flush_measure();

        // Extract the measures produced by this voice branch
        let part = state.ensure_part();
        let voice_measures: Vec<_> = part.measures.drain(part_measures_before..).collect();

        // Renumber voices in these measures
        let voice_measures = renumber_voices_in_measures(voice_measures, (voice_idx + 1) as u8);
        voice_streams.push(voice_measures);
    }

    // 4. Merge the per-voice measure streams
    let mut merged = merge_voice_measure_streams(&voice_streams);

    // 5. Restore state and flush pending content before appending merged measures
    state.current_measure = saved_current_measure;
    state.current_voice = saved_current_voice;
    state.current_time_sig = saved_time_sig;
    state.stem_direction = saved_stem_direction;
    state.current_voice_number = saved_voice_number;
    state.tuplet_stack = saved_tuplet_stack;
    state.auto_beam_off = saved_auto_beam_off;
    state.elapsed_in_measure = saved_elapsed;

    // If there's pending voice content, flush it before appending merged measures
    if !state.current_voice.is_empty() {
        state.flush_measure();
        state.elapsed_in_measure = Frac::from_integer(0);
    }

    // Transfer attributes (time sig, key, clef) from pending current_measure
    // to the first merged measure, so they appear at the right position.
    if let Some(ref mut cm) = state.current_measure {
        if let Some(ref cm_attrs) = cm.attributes {
            if !merged.is_empty() {
                let first = &mut merged[0];
                let fa = first
                    .attributes
                    .get_or_insert_with(MeasureAttributes::default);
                if cm_attrs.time.is_some() && fa.time.is_none() {
                    fa.time = cm_attrs.time.clone();
                }
                if cm_attrs.key.is_some() && fa.key.is_none() {
                    fa.key = cm_attrs.key;
                }
                if !cm_attrs.clefs.is_empty() && fa.clefs.is_empty() {
                    fa.clefs = cm_attrs.clefs.clone();
                }
            }
        }
        // Transfer directions from pending measure
        if !cm.directions.is_empty() && !merged.is_empty() {
            let dirs = std::mem::take(&mut cm.directions);
            let first = &mut merged[0];
            let mut existing = std::mem::take(&mut first.directions);
            first.directions = dirs;
            first.directions.append(&mut existing);
        }
        // Clear the pending measure (its content was transferred)
        state.current_measure = None;
    }

    // Advance measure_num past the merged measures
    state.measure_num = saved_measure_num + merged.len() as u32;

    let part = state.ensure_part();
    part.measures.extend(merged);
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

/// Walk a `<< ... >>` block that does NOT contain `\\` separators.
/// This handles simultaneous staves/contexts (\new Staff, \new Voice, etc.).
fn walk_parallel_music_staves(state: &mut WalkState, children: &[Node]) {
    let mut i = 0;

    while i < children.len() {
        let child = children[i];
        match child.kind() {
            "named_context" => {
                let (context, name) = extract_named_context(state, child);
                i += 1;
                i = walk_context_body(state, children, i, &context, &name);
                continue;
            }
            "expression_block" => {
                walk_music_block(state, child);
            }
            "escaped_word" => {
                let text = state.text(child).to_string();
                if text == "\\relative" {
                    state.in_relative = true;
                    state.mode = PitchMode::Relative;
                    i += 1;
                    i = consume_relative(state, children, i);
                    continue;
                } else if text == "\\transpose" {
                    i += 1;
                    i = consume_transpose(state, children, i);
                    continue;
                } else if text == "\\unfoldRepeats" {
                    // Transparent wrapper — just skip the keyword
                } else if matches!(text.as_str(), "\\set" | "\\override" | "\\revert") {
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
                } else {
                    let var_name = text.trim_start_matches('\\');
                    state.resolve_variable(var_name);
                }
            }
            _ => {}
        }
        i += 1;
    }
}

/// Extract context type and context name from a `named_context` node.
/// For `\new Staff`, returns ("Staff", "").
/// For `\context Voice = "melodySop"`, returns ("Voice", "melodySop").
pub(super) fn extract_named_context<'a>(state: &WalkState<'a>, node: Node<'a>) -> (String, String) {
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

    // Voice context: don't create a new part, but set voice name on current part
    if context == "Voice" {
        // Don't call new_part — we stay in the current Staff part
        // Just consume the body block
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
                    if text == "\\relative" {
                        state.in_relative = true;
                        state.mode = PitchMode::Relative;
                        i += 1;
                        i = consume_relative(state, children, i);
                        continue;
                    }
                    if text == "\\transpose" {
                        i += 1;
                        i = consume_transpose(state, children, i);
                        continue;
                    }
                    break;
                }
                _ => break,
            }
        }
        // Store the voice name for lyrics attachment
        if !name.is_empty() {
            let _part = state.ensure_part();
            // Store voice name → part index mapping
            let part_idx = state.parts.len().saturating_sub(1);
            state
                .voice_part_map
                .insert(name.to_string(), part_idx);
        }
        return i;
    }

    // Dynamics / NullVoice contexts: these carry layout-only information
    // (dynamics placement between staves, null voice for lyrics alignment).
    // Don't create a new part; parse the body into the current part context
    // so that spacer-rest merging can attach directions to the correct staff.
    if context == "Dynamics" || context == "NullVoice" {
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
        // Parse body into current part (no new_part call)
        if let Some(node) = children.get(i) {
            match node.kind() {
                "expression_block" => {
                    walk_music_block(state, *node);
                    i += 1;
                }
                "parallel_music" => {
                    walk_parallel_music(state, *node);
                    i += 1;
                }
                "escaped_word" => {
                    let text = state.text(*node).to_string();
                    let var_name = text.trim_start_matches('\\');
                    if state.resolve_variable(var_name) {
                        i += 1;
                    }
                }
                _ => {}
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
        return i;
    }

    // Remember old relative state
    let was_relative = state.in_relative;
    let old_ref = state.relative_ref;
    let old_prev = state.prev_pitch;

    state.new_part(context, name);

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
                        // Skip \with { ... }
                        if let Some(next) = children.get(i + 1) {
                            if next.kind() == "expression_block" {
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
                        state.pending_lyrics.insert(voice.clone(), syllables.clone());
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
