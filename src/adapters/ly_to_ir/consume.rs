use num::rational::Ratio;
use tree_sitter::Node;

use crate::ir::articulation::Placement;
use crate::ir::direction::{Direction, TempoDirection};
use crate::ir::duration::{Duration, Frac};
use crate::ir::language::parse_pitch_name;
use crate::ir::note::{Chord, Note};
use crate::ir::pitch::{AccidentalDisplay, Pitch};
use crate::ir::score::PageLayout;

use super::state::WalkState;

/// Consume octave marks (' and ,) after a pitch symbol. Returns net marks.
pub(super) fn consume_octave_marks(state: &WalkState, children: &[Node], i: &mut usize) -> i32 {
    let mut marks = 0i32;
    while *i < children.len() {
        let node = children[*i];
        if node.kind() == "punctuation" {
            let child_text = punct_text(state, node);
            if child_text == "'" {
                marks += 1;
                *i += 1;
            } else if child_text == "," {
                marks -= 1;
                *i += 1;
            } else {
                break;
            }
        } else {
            break;
        }
    }
    marks
}

/// Consume accidental forcing marks (`!` = forced, `?` = cautionary) after a pitch.
/// These are LilyPond punctuation tokens that appear between octave marks and duration.
/// Returns the resulting `AccidentalDisplay`.
pub(super) fn consume_accidental_marks(
    state: &WalkState,
    children: &[Node],
    i: &mut usize,
) -> AccidentalDisplay {
    let mut display = AccidentalDisplay::None;
    while *i < children.len() {
        let node = children[*i];
        if node.kind() == "punctuation" {
            let text = punct_text(state, node);
            if text == "!" {
                display = AccidentalDisplay::Forced;
                *i += 1;
            } else if text == "?" {
                display = AccidentalDisplay::Cautionary;
                *i += 1;
            } else {
                break;
            }
        } else {
            break;
        }
    }
    display
}

/// Consume an optional duration (unsigned_integer + dot punctuation).
/// If no duration is found, returns the last used duration.
pub(super) fn consume_duration(
    state: &mut WalkState,
    children: &[Node],
    i: &mut usize,
) -> Duration {
    // Look for unsigned_integer
    if *i < children.len() && children[*i].kind() == "unsigned_integer" {
        let num_text = state.text(children[*i]).to_string();
        *i += 1;
        if let Ok(num) = num_text.parse::<u32>() {
            // Count dots
            let mut dots = 0u8;
            while *i < children.len() {
                let node = children[*i];
                if node.kind() == "punctuation" {
                    let ptext = punct_text(state, node);
                    if ptext == "." {
                        dots += 1;
                        *i += 1;
                    } else {
                        break;
                    }
                } else {
                    break;
                }
            }
            // Convert LilyPond number to fraction: 4 → 1/4, 2 → 1/2, 1 → 1/1
            let base = if num > 0 {
                Ratio::new(1i64, num as i64)
            } else {
                Ratio::new(1, 4)
            };
            let dur = Duration::dotted(base, dots);
            state.last_duration = dur.clone();
            return dur;
        }
    }
    state.last_duration.clone()
}

/// Consume an optional `*N` or `*N/M` duration scaling factor.
/// Returns the fractional scale (e.g. `*8/7` → 8/7, `*3` → 3/1).
/// Returns `None` if no multiplier is present.
/// Use this for notes/chords where `*N/M` scales the sounding duration.
///
/// Tree-sitter may produce `*N/M` as either:
/// - `punctuation("*")` `fraction("N/M")` (single fraction token), or
/// - `punctuation("*")` `unsigned_integer("N")` `punctuation("/")` `unsigned_integer("M")`
pub(super) fn consume_duration_scale(
    state: &WalkState,
    children: &[Node],
    i: &mut usize,
) -> Option<Frac> {
    if *i < children.len() && children[*i].kind() == "punctuation" {
        let ptext = punct_text(state, children[*i]);
        if ptext == "*" {
            *i += 1;
            // Case 1: fraction token (e.g. "8/7")
            if *i < children.len() && children[*i].kind() == "fraction" {
                let frac_text = state.text(children[*i]);
                *i += 1;
                if let Some((num, den)) = parse_fraction(frac_text) {
                    return Some(Frac::new(num as i64, den as i64));
                }
                return None;
            }
            // Case 2: unsigned_integer, optionally followed by / and unsigned_integer
            if *i < children.len() && children[*i].kind() == "unsigned_integer" {
                let num_text = state.text(children[*i]).to_string();
                *i += 1;
                let numer: i64 = num_text.parse().unwrap_or(1);
                // Check for fraction: *N/M as separate tokens
                if *i + 1 < children.len()
                    && children[*i].kind() == "punctuation"
                    && punct_text(state, children[*i]) == "/"
                {
                    *i += 1; // skip "/"
                    if *i < children.len() && children[*i].kind() == "unsigned_integer" {
                        let denom_text = state.text(children[*i]).to_string();
                        *i += 1;
                        let denom: i64 = denom_text.parse().unwrap_or(1);
                        return Some(Frac::new(numer, denom));
                    }
                }
                return Some(Frac::from_integer(numer));
            }
        }
    }
    None
}

/// Consume an optional tremolo suffix `:N` (e.g. `c4:32`) after a duration.
/// Returns the number of tremolo marks (0 if none found).
/// Formula: marks = log2(N / base_dur_denom), e.g. `:32` on a quarter note (4) → 3 marks.
pub(super) fn consume_tremolo(
    state: &WalkState,
    children: &[Node],
    i: &mut usize,
    base_dur: &Duration,
) -> u8 {
    if *i < children.len() && children[*i].kind() == "punctuation" {
        let ptext = punct_text(state, children[*i]);
        if ptext == ":" {
            *i += 1;
            if *i < children.len() && children[*i].kind() == "unsigned_integer" {
                let num_text = state.text(children[*i]).to_string();
                *i += 1;
                if let Ok(n) = num_text.parse::<u32>() {
                    // base_dur denominator: e.g. quarter = 1/4 → denom 4
                    let base_denom = *base_dur.base.denom() as u32;
                    if n > base_denom && base_denom > 0 {
                        let ratio = n / base_denom;
                        return (ratio as f64).log2() as u8;
                    }
                }
            }
        }
    }
    0
}

/// Consume post-note attachments: dynamics, ties, slurs, articulations, etc.
/// Returns a list of attachment tokens.
pub(super) fn consume_attachments(
    state: &WalkState,
    children: &[Node],
    i: &mut usize,
) -> Vec<String> {
    let mut attachments = Vec::new();
    while *i < children.len() {
        let node = children[*i];
        match node.kind() {
            "dynamic" => {
                let dyn_text = state.text(node);
                attachments.push(dyn_text.to_string());
                *i += 1;
            }
            "escaped_word" => {
                let text = state.text(node);
                if is_post_note_command(text) {
                    attachments.push(text.to_string());
                    *i += 1;
                } else {
                    break;
                }
            }
            "punctuation" => {
                let ptext = punct_text(state, node);
                match ptext.as_str() {
                    "(" | ")" | "~" | "[" | "]" => {
                        attachments.push(ptext);
                        *i += 1;
                    }
                    "^" | "_" | "-" => {
                        // Direction indicator: check what follows
                        if let Some(next) = children.get(*i + 1) {
                            if next.kind() == "escaped_word" && state.text(*next) == "\\markup" {
                                // \markup { "text" } text direction
                                if let Some(block) = children.get(*i + 2) {
                                    if block.kind() == "expression_block" {
                                        let text = extract_markup_text(state, *block);
                                        if !text.is_empty() {
                                            let placement = match ptext.as_str() {
                                                "^" => "above",
                                                "_" => "below",
                                                _ => "unspecified",
                                            };
                                            attachments.push(format!("text:{placement}:{text}"));
                                        }
                                        *i += 3;
                                        continue;
                                    }
                                }
                            } else if next.kind() == "escaped_word" {
                                // Direction + escaped command, e.g. ^\fermata
                                let ew = state.text(*next);
                                if is_post_note_command(ew) {
                                    attachments.push(ew.to_string());
                                    *i += 2;
                                    continue;
                                }
                            } else if next.kind() == "punctuation" {
                                // Shorthand articulation: -. -> -_ -^ -! -+ --
                                let short = punct_text(state, *next);
                                if let Some(art) = shorthand_articulation(&short) {
                                    attachments.push(art.to_string());
                                    *i += 2;
                                    continue;
                                }
                            } else if next.kind() == "unsigned_integer" {
                                // Fingering: -1, -2, -3, etc.
                                let finger = state.text(*next).to_string();
                                attachments.push(format!("finger:{finger}"));
                                *i += 2;
                                continue;
                            }
                        }
                        break;
                    }
                    _ => break,
                }
            }
            _ => break,
        }
    }
    attachments
}

/// Map a LilyPond shorthand articulation character to its long-form escaped command.
pub(super) fn shorthand_articulation(ch: &str) -> Option<&'static str> {
    match ch {
        "." => Some("\\staccato"),
        ">" => Some("\\accent"),
        "_" => Some("\\tenuto"),
        "^" => Some("\\marcato"),
        "!" => Some("\\staccatissimo"),
        "+" => Some("\\stopped"),
        "-" => Some("\\tenuto"),
        _ => None,
    }
}

/// Whether an escaped_word is a post-note attachment rather than a new command.
pub(super) fn is_post_note_command(text: &str) -> bool {
    matches!(
        text,
        "\\fermata"
            | "\\trill"
            | "\\mordent"
            | "\\prall"
            | "\\turn"
            | "\\reverseturn"
            | "\\shake"
            | "\\breathe"
            | "\\staccato"
            | "\\staccatissimo"
            | "\\tenuto"
            | "\\accent"
            | "\\marcato"
            | "\\portato"
            | "\\stopped"
            | "\\espressivo"
            | "\\glissando"
            | "\\arpeggio"
            | "\\upbow"
            | "\\downbow"
            | "\\flageolet"
            | "\\open"
            | "\\snappizzicato"
    ) || is_dynamic_name(text)
}

/// Get the text of a punctuation node (which may have a child).
pub(super) fn punct_text(state: &WalkState, node: Node) -> String {
    if node.child_count() > 0 {
        let mut c = node.walk();
        let result = node
            .children(&mut c)
            .next()
            .map(|n| state.text(n).to_string())
            .unwrap_or_default();
        result
    } else {
        state.text(node).to_string()
    }
}

/// Consume a `\tempo` command (various forms).
pub(super) fn consume_tempo(state: &mut WalkState, children: &[Node], mut i: usize) -> usize {
    let mut text_label: Option<String> = None;
    let mut beat_unit: Option<String> = None;
    let mut per_minute: Option<u32> = None;
    let mut dots: u8 = 0;

    // \tempo "Allegro" 4 = 120
    // \tempo 4. = 60
    // \tempo 4 = 120
    // \tempo "Allegro"
    while i < children.len() {
        let node = children[i];
        match node.kind() {
            "string" => {
                text_label = Some(extract_string_value(state, node));
                i += 1;
            }
            "unsigned_integer" => {
                let num_text = state.text(node);
                if beat_unit.is_none() {
                    beat_unit = Some(ly_number_to_beat_unit(num_text));
                    i += 1;
                } else {
                    // This is the BPM value
                    per_minute = num_text.parse().ok();
                    i += 1;
                    break;
                }
            }
            "punctuation" => {
                let pt = punct_text(state, node);
                if pt == "=" {
                    i += 1; // skip equals sign
                } else if pt == "." && beat_unit.is_some() && per_minute.is_none() {
                    dots += 1;
                    i += 1;
                } else {
                    break;
                }
            }
            _ => break,
        }
    }

    if text_label.is_some() || beat_unit.is_some() || per_minute.is_some() {
        let tempo_dir = TempoDirection {
            text: text_label,
            beat_unit,
            dots,
            per_minute: per_minute.map(|v| v as f64),
            placement: Placement::Unspecified,
        };
        let dir = Direction {
            placement: Placement::Above,
            tempo: Some(tempo_dir),
            ..Default::default()
        };
        let measure = state.ensure_measure();
        measure.directions.push(dir);
    }
    i
}

/// Convert LilyPond duration number to beat unit string.
pub(super) fn ly_number_to_beat_unit(num: &str) -> String {
    match num {
        "1" => "whole",
        "2" => "half",
        "4" => "quarter",
        "8" => "eighth",
        "16" => "16th",
        "32" => "32nd",
        "64" => "64th",
        _ => "quarter",
    }
    .to_string()
}

/// Extract the text content of a `string` node. Returns the unquoted value.
pub(super) fn extract_string_value(state: &WalkState, node: Node) -> String {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "string_fragment" {
            return state.text(child).to_string();
        }
    }
    String::new()
}

/// Extract text content from a markup expression_block like `{ "pizz." }`.
/// Walks children looking for string nodes and returns the concatenated text.
pub(super) fn extract_markup_text(state: &WalkState, block: Node) -> String {
    let mut result = String::new();
    let mut cursor = block.walk();
    for child in block.children(&mut cursor) {
        if child.kind() == "string" {
            let s = extract_string_value(state, child);
            if !result.is_empty() {
                result.push(' ');
            }
            result.push_str(&s);
        }
    }
    result
}

/// Parse a `\with { ... }` expression block and extract known properties.
/// Returns a map of property name -> value (e.g. "instrumentName" -> "Violin").
pub(super) fn parse_with_block(
    state: &WalkState,
    block: Node,
) -> std::collections::HashMap<String, String> {
    let mut props = std::collections::HashMap::new();
    let mut cursor = block.walk();
    let children: Vec<Node> = block.children(&mut cursor).collect();
    let mut i = 0;
    while i < children.len() {
        let child = children[i];
        if child.kind() == "assignment_lhs" || child.kind() == "symbol" {
            let key = state.text(child).to_string();
            // Strip context prefix like "Staff."
            let prop_name = key
                .rsplit_once('.')
                .map(|(_, k)| k)
                .unwrap_or(&key)
                .to_string();
            i += 1;
            // Skip "="
            if i < children.len()
                && children[i].kind() == "punctuation"
                && state.text(children[i]) == "="
            {
                i += 1;
            }
            // Read value
            if i < children.len() {
                let val_node = children[i];
                let val = if val_node.kind() == "string" {
                    extract_string_value(state, val_node)
                } else if val_node.kind() == "embedded_scheme" {
                    let t = state.text(val_node).to_string();
                    t.trim_start_matches("#\"")
                        .trim_end_matches('"')
                        .trim_start_matches("#'")
                        .trim_start_matches('#')
                        .to_string()
                } else {
                    state.text(val_node).to_string()
                };
                props.insert(prop_name, val);
                i += 1;
                continue;
            }
        }
        i += 1;
    }
    props
}

/// Consume a `\mark` command with its argument.
/// Handles:
///   `\mark \markup { \musicglyph "scripts.coda" }`   → coda
///   `\mark \markup { \musicglyph "scripts.segno" }`  → segno
///   `\mark "D.C."` / `\mark "D.S. al Coda"` etc.     → da_capo / dal_segno
pub(super) fn consume_mark(state: &mut WalkState, children: &[Node], mut i: usize) -> usize {
    if i >= children.len() {
        return i;
    }
    let node = children[i];
    if node.kind() == "string" {
        // \mark "D.C." or \mark "D.S. al Coda"
        let text = extract_string_value(state, node);
        i += 1;
        let dir = if text.starts_with("D.S.") {
            Direction {
                dal_segno: Some(text),
                ..Default::default()
            }
        } else if text.starts_with("D.C.") {
            Direction {
                da_capo: Some(text),
                ..Default::default()
            }
        } else {
            // Generic text mark — ignore for now
            return i;
        };
        let measure = state.ensure_measure();
        measure.directions.push(dir);
    } else if node.kind() == "escaped_word" && state.text(node) == "\\markup" {
        // \mark \markup { ... }
        i += 1;
        if let Some(block) = children.get(i) {
            if block.kind() == "expression_block" {
                // Walk the markup block looking for \musicglyph "scripts.coda" etc.
                let block_text = state.text(*block);
                let dir = if block_text.contains("scripts.coda") {
                    Some(Direction {
                        coda: true,
                        ..Default::default()
                    })
                } else if block_text.contains("scripts.segno") {
                    Some(Direction {
                        segno: true,
                        ..Default::default()
                    })
                } else {
                    None
                };
                if let Some(d) = dir {
                    let measure = state.ensure_measure();
                    measure.directions.push(d);
                }
                i += 1;
            }
        }
    }
    i
}

/// Consume a `\override` command.
/// Recognises `Glissando.style = #'<style>` and sets pending state.
pub(super) fn consume_override(state: &mut WalkState, children: &[Node], mut i: usize) -> usize {
    // Collect tokens to build property path and value
    // Expected pattern: <symbol>.<symbol> = <scheme_value>
    // We read the raw text span from the first symbol through a few tokens.
    let start_i = i;
    let mut symbols: Vec<String> = Vec::new();
    let mut value = String::new();
    let mut saw_eq = false;

    // Consume up to ~10 tokens looking for the pattern
    let limit = (i + 10).min(children.len());
    while i < limit {
        let node = children[i];
        match node.kind() {
            "symbol" if !saw_eq => {
                symbols.push(state.text(node).to_string());
                i += 1;
            }
            "punctuation" => {
                let pt = punct_text(state, node);
                if pt == "." && !saw_eq {
                    i += 1; // skip dot in property path
                } else if pt == "=" {
                    saw_eq = true;
                    i += 1;
                } else {
                    break;
                }
            }
            _ if saw_eq => {
                // Whatever follows the "=" is the value — grab its raw text
                value = state.text(node).to_string();
                i += 1;
                break;
            }
            _ => break,
        }
    }

    // Check for known Glissando.style overrides
    if symbols.len() >= 2 && symbols[0] == "Glissando" && symbols[1] == "style" && saw_eq {
        // value is e.g. "#'dashed-line", "#'dotted-line", "#'trill"
        let style = value
            .trim_start_matches("#'")
            .trim_start_matches("#\u{2018}"); // curly quote edge case
        match style {
            "dashed-line" => {
                state.pending_glissando_style = Some("dashed".to_string());
            }
            "dotted-line" => {
                state.pending_glissando_style = Some("dotted".to_string());
            }
            "trill" => {
                state.pending_slide = true;
            }
            _ => {
                state.pending_glissando_style = Some(style.to_string());
            }
        }
    }

    // If we didn't consume anything useful, at least advance past start
    if i == start_i {
        // Skip unknown override — try to jump past expression_block or next statement
        while i < children.len() {
            let node = children[i];
            if node.kind() == "escaped_word" || node.kind() == "symbol" {
                break;
            }
            i += 1;
        }
    }
    i
}

/// Check if an expression_block directly contains a `named_context` child
/// (i.e., `\new Staff`, `\new ChoirStaff`, etc., but NOT `\new Voice` or `\new Lyrics`).
pub(super) fn block_contains_named_context(state: &WalkState, block: Node) -> bool {
    let mut cursor = block.walk();
    for child in block.children(&mut cursor) {
        if child.kind() == "named_context" {
            let (context, _) = super::walk::extract_named_context(state, child);
            // Only part-creating or grouping contexts count
            if matches!(
                context.as_str(),
                "Staff" | "ChoirStaff" | "StaffGroup" | "GrandStaff" | "PianoStaff"
            ) {
                return true;
            }
        }
    }
    false
}

/// Check if a `\score { ... }` expression_block contains `\layout` and/or `\midi`.
/// Returns (has_layout, has_midi).
pub(super) fn score_block_output_types(state: &WalkState, block: Node) -> (bool, bool) {
    let mut has_layout = false;
    let mut has_midi = false;
    let mut cursor = block.walk();
    for child in block.children(&mut cursor) {
        if child.kind() == "escaped_word" {
            let text = state.text(child);
            if text == "\\layout" {
                has_layout = true;
            } else if text == "\\midi" {
                has_midi = true;
            }
        }
    }
    (has_layout, has_midi)
}

/// Parse a `\paper { ... }` block and populate `state.page_layout`.
pub(super) fn parse_paper_block(state: &mut WalkState, block: Node) {
    let mut layout = state.page_layout.take().unwrap_or(PageLayout {
        page_height: None,
        page_width: None,
        left_margin: None,
        right_margin: None,
        top_margin: None,
        bottom_margin: None,
        system_distance: None,
        top_system_distance: None,
        staff_size: None,
    });

    let mut cursor = block.walk();
    let children: Vec<Node> = block.children(&mut cursor).collect();
    let mut i = 0;

    while i < children.len() {
        let node = children[i];
        if node.kind() == "assignment_lhs" {
            // Extract the property name (may be compound like
            // "system-system-spacing.basic-distance")
            let key = state.text(node).trim().to_string();
            // Skip "=" punctuation and find the value
            let mut j = i + 1;
            while j < children.len() {
                let val_node = children[j];
                if val_node.kind() == "punctuation" && punct_text(state, val_node) == "=" {
                    j += 1;
                    continue;
                }
                // The value could be an unsigned_integer, a number with \cm suffix,
                // or a scheme expression like #15.0
                // Grab the raw text of the remaining tokens on this "line"
                let raw = state.text(val_node).to_string();
                // Try to extract a float — strip \cm, \mm, #, etc.
                let cleaned = raw
                    .replace("\\cm", "")
                    .replace("\\mm", "")
                    .replace("\\in", "")
                    .trim_start_matches('#')
                    .trim()
                    .to_string();
                if let Ok(v) = cleaned.parse::<f64>() {
                    match key.as_str() {
                        "paper-height" => layout.page_height = Some(v),
                        "paper-width" => layout.page_width = Some(v),
                        "left-margin" => layout.left_margin = Some(v),
                        "right-margin" => layout.right_margin = Some(v),
                        "top-margin" => layout.top_margin = Some(v),
                        "bottom-margin" => layout.bottom_margin = Some(v),
                        _ => {
                            if key.contains("system-system-spacing") {
                                layout.system_distance = Some(v);
                            } else if key.contains("top-system-spacing") {
                                layout.top_system_distance = Some(v);
                            }
                        }
                    }
                }
                i = j;
                break;
            }
        }
        i += 1;
    }

    state.page_layout = Some(layout);
}

/// Parse a fraction string like "4/4" into (numerator, denominator).
pub(super) fn parse_fraction(text: &str) -> Option<(u32, u32)> {
    let parts: Vec<&str> = text.split('/').collect();
    if parts.len() == 2 {
        let num = parts[0].parse::<u32>().ok()?;
        let den = parts[1].parse::<u32>().ok()?;
        Some((num, den))
    } else {
        None
    }
}

/// Parse `#(ly:make-moment N D)` from a Scheme expression text.
/// Returns (numerator, denominator) if successful.
pub(super) fn parse_ly_make_moment(text: &str) -> Option<(u32, u32)> {
    // Text looks like: #(ly:make-moment 3 4) or #(ly:make-moment 3/4)
    let inner = text.trim_start_matches('#').trim();
    let inner = inner.strip_prefix('(')?.strip_suffix(')')?;
    let inner = inner.trim();
    if !inner.starts_with("ly:make-moment") {
        return None;
    }
    let args = inner.strip_prefix("ly:make-moment")?.trim();
    // Try "N D" format first
    let parts: Vec<&str> = args.split_whitespace().collect();
    if parts.len() == 2 {
        let num = parts[0].parse::<u32>().ok()?;
        let den = parts[1].parse::<u32>().ok()?;
        return Some((num, den));
    }
    // Try "N/D" format
    if let Some((num, den)) = parse_fraction(args) {
        return Some((num, den));
    }
    None
}

/// Extract a string from a scheme expression like `#"flute"`.
/// Returns `None` if the expression doesn't contain a quoted string.
pub(super) fn extract_scheme_string(text: &str) -> Option<String> {
    let inner = text.trim_start_matches('#').trim();
    if inner.starts_with('"') && inner.ends_with('"') && inner.len() >= 2 {
        Some(inner[1..inner.len() - 1].to_string())
    } else {
        None
    }
}

/// Build a Chord from a `chord` node (< ... >).
///
/// LilyPond relative-mode chord semantics (from quickly `relative.py`):
///   - Within a chord, each note is relative to the **previous** note (stack).
///   - After the chord, `prev_pitch` resets to the chord's **first** pitch.
pub(super) fn build_chord(state: &mut WalkState, chord_node: Node, dur: Duration) -> Chord {
    let mut notes = Vec::new();
    let mut cursor = chord_node.walk();
    let children: Vec<Node> = chord_node.children(&mut cursor).collect();
    let mut i = 0;
    let mut first_pitch: Option<Pitch> = None;

    while i < children.len() {
        let child = children[i];
        if child.kind() == "symbol" {
            let sym = state.text(child);
            if let Some((step, alter)) = parse_pitch_name(sym, state.language) {
                i += 1;
                let octave_marks = consume_octave_marks(state, &children, &mut i);
                let acc_display = consume_accidental_marks(state, &children, &mut i);
                let mut pitch = state.resolve_pitch(step, alter, octave_marks);
                pitch.accidental = acc_display;
                if first_pitch.is_none() {
                    first_pitch = Some(pitch);
                }
                let note = Note::new(pitch, dur.clone());
                notes.push(note);
                continue;
            }
        }
        i += 1;
    }
    // After a chord, the next note is relative to the chord's first pitch.
    if let Some(fp) = first_pitch {
        if state.in_relative {
            state.prev_pitch = Some(fp);
        }
    }
    Chord::new(dur, notes)
}

/// Parse grace notes from a `{ ... }` block.
pub(super) fn parse_grace_block(state: &mut WalkState, block: Node) -> Vec<Note> {
    let mut notes = Vec::new();
    let mut cursor = block.walk();
    let children: Vec<Node> = block.children(&mut cursor).collect();
    let mut i = 0;

    while i < children.len() {
        let child = children[i];
        if child.kind() == "symbol" {
            let sym = state.text(child);
            if let Some((step, alter)) = parse_pitch_name(sym, state.language) {
                i += 1;
                let octave_marks = consume_octave_marks(state, &children, &mut i);
                let acc_display = consume_accidental_marks(state, &children, &mut i);
                let dur = consume_duration(state, &children, &mut i);
                let mut pitch = state.resolve_pitch(step, alter, octave_marks);
                pitch.accidental = acc_display;
                let note = Note::new(pitch, dur);
                notes.push(note);
                continue;
            }
        }
        i += 1;
    }
    notes
}

/// Whether a `\xxx` string is a known dynamic marking.
pub(super) fn is_dynamic_name(text: &str) -> bool {
    matches!(
        text,
        "\\ppppp"
            | "\\pppp"
            | "\\ppp"
            | "\\pp"
            | "\\p"
            | "\\mp"
            | "\\mf"
            | "\\f"
            | "\\ff"
            | "\\fff"
            | "\\ffff"
            | "\\fffff"
            | "\\fp"
            | "\\sf"
            | "\\sfz"
            | "\\sff"
            | "\\sp"
            | "\\spp"
            | "\\rfz"
            | "\\fz"
    )
}
