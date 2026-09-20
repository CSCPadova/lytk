use tree_sitter::Node;

use crate::ir::duration::Duration;
use crate::ir::harmony::{Figure, FiguredBass};
use crate::ir::measure::Measure;

use super::state::WalkState;
use super::FiguredBassEntry;

/// Divisions per quarter note used when computing figured bass offsets.
/// Must match `DEFAULT_DIVISIONS` in `ir_to_mxml.rs`.
pub(super) const FIGURED_BASS_DIVISIONS: i64 = 4;

/// Parse a `\figuremode { ... }` expression block into a flat stream of figured bass entries.
///
/// Inside figuremode, `<6 4>` is a chord node containing figure numbers and accidentals.
/// `s` is a skip (spacer). `|` is a bar check. Durations follow the same syntax as notes.
/// The flat stream preserves duration information so that figures can be distributed
/// across measures during resolve by matching cumulative durations.
pub(super) fn parse_figuremode_block(state: &WalkState, block: Node) -> Vec<FiguredBassEntry> {
    let mut cursor = block.walk();
    let children: Vec<Node> = block.children(&mut cursor).collect();
    let mut i = 0;

    let mut entries: Vec<FiguredBassEntry> = Vec::new();
    let mut last_dur = Duration::quarter();

    while i < children.len() {
        let node = children[i];
        match node.kind() {
            "chord" => {
                // Parse figure group: <6 4>, <_+>, <6+>, <7 3+ 9>, etc.
                let figures = parse_figure_chord(state, node);
                i += 1;
                // Consume duration after chord
                let dur = consume_duration_stateless(
                    &children,
                    &mut i,
                    &mut last_dur,
                    state.source.as_bytes(),
                );
                entries.push(FiguredBassEntry::Figure(FiguredBass {
                    figures,
                    duration: dur,
                    parentheses: false,
                    offset: 0,
                }));
                continue;
            }
            "symbol" => {
                let sym = state.text(node);
                if sym == "s" {
                    // Spacer — skip with duration
                    i += 1;
                    let dur = consume_duration_stateless(
                        &children,
                        &mut i,
                        &mut last_dur,
                        state.source.as_bytes(),
                    );
                    let count = consume_multiplier_stateless(state, &children, &mut i);
                    for _ in 0..count {
                        entries.push(FiguredBassEntry::Skip(dur.clone()));
                    }
                    continue;
                }
            }
            "escaped_word" => {
                // Skip figuremode commands like \bassFigureExtendersOff
            }
            _ => {}
        }
        i += 1;
    }

    entries
}

/// Parse a single chord node inside figuremode into a list of figures.
///
/// The chord node children are:
/// - `<` / `>` structural tokens
/// - `unsigned_integer` for figure numbers (6, 4, 11, etc.)
/// - `punctuation` `_` for placeholder (no number)
/// - `punctuation` `+` / `-` for accidental modifiers on the preceding figure
fn parse_figure_chord(state: &WalkState, chord_node: Node) -> Vec<Figure> {
    let mut cursor = chord_node.walk();
    let children: Vec<Node> = chord_node.children(&mut cursor).collect();
    let mut figures: Vec<Figure> = Vec::new();

    let mut i = 0;
    while i < children.len() {
        let child = children[i];
        match child.kind() {
            "unsigned_integer" => {
                let num: u8 = state.text(child).parse().unwrap_or(0);
                // Consume any run of accidental modifiers immediately following.
                let mut j = i + 1;
                let suffix = consume_accidental_run(state, &children, &mut j);
                figures.push(Figure {
                    number: Some(num),
                    prefix: None,
                    suffix,
                });
                i = j;
                continue;
            }
            "punctuation" => {
                let text = state.text(child);
                if text == "_" {
                    // Placeholder figure — consume any following accidental run.
                    let mut j = i + 1;
                    let suffix = consume_accidental_run(state, &children, &mut j);
                    figures.push(Figure {
                        number: None,
                        prefix: None,
                        suffix,
                    });
                    i = j;
                    continue;
                }
                // Skip `<`, `>`, and other punctuation
            }
            _ => {}
        }
        i += 1;
    }

    figures
}

/// Consume a run of accidental-modifier punctuation following a figure number
/// or placeholder, advancing `idx`. Maps to MusicXML figured-bass suffix values:
/// `+`→sharp, `++`→double-sharp, `-`→flat, `--`→double-flat, `!`→natural.
fn consume_accidental_run(state: &WalkState, children: &[Node], idx: &mut usize) -> Option<String> {
    let mut plus = 0u32;
    let mut minus = 0u32;
    let mut natural = false;
    while let Some(next) = children.get(*idx) {
        if next.kind() != "punctuation" {
            break;
        }
        match state.text(*next) {
            "+" => plus += 1,
            "-" => minus += 1,
            "!" => natural = true,
            _ => break,
        }
        *idx += 1;
    }
    let s = match (plus, minus, natural) {
        (0, 0, true) => "natural",
        (1, 0, _) => "sharp",
        (n, 0, _) if n >= 2 => "double-sharp",
        (0, 1, _) => "flat",
        (0, n, _) if n >= 2 => "double-flat",
        _ => return None,
    };
    Some(s.to_string())
}

/// Consume duration tokens without mutating WalkState (for figuremode parsing).
/// Returns the duration found, updating `last_dur` for carry-forward.
fn consume_duration_stateless(
    children: &[Node],
    i: &mut usize,
    last_dur: &mut Duration,
    source: &[u8],
) -> Duration {
    // Look for unsigned_integer (duration value) followed by optional dots
    let mut dur_val: Option<u32> = None;
    let mut dots = 0u8;

    // Check for duration number
    if let Some(node) = children.get(*i) {
        if node.kind() == "unsigned_integer" {
            if let Ok(val) = node.utf8_text(source).unwrap_or("").parse::<u32>() {
                // Only valid duration values: 1, 2, 4, 8, 16, 32, 64, 128
                if matches!(val, 1 | 2 | 4 | 8 | 16 | 32 | 64 | 128) {
                    dur_val = Some(val);
                    *i += 1;
                }
            }
        }
    }

    // Consume dots
    while let Some(node) = children.get(*i) {
        if node.kind() == "punctuation" {
            if let Ok(text) = node.utf8_text(source) {
                if text == "." {
                    dots += 1;
                    *i += 1;
                    continue;
                }
            }
        }
        break;
    }

    if let Some(val) = dur_val {
        let mut dur = Duration::from_lilypond_number(val, 0).unwrap_or_else(Duration::quarter);
        dur.dots = dots;
        *last_dur = dur.clone();
        dur
    } else if dots > 0 {
        let mut dur = last_dur.clone();
        dur.dots = dots;
        *last_dur = dur.clone();
        dur
    } else {
        last_dur.clone()
    }
}

/// Consume a `*N` multiplier without mutating WalkState.
fn consume_multiplier_stateless(state: &WalkState, children: &[Node], i: &mut usize) -> u32 {
    if let Some(node) = children.get(*i) {
        if node.kind() == "punctuation" && state.text(*node) == "*" {
            *i += 1;
            if let Some(num_node) = children.get(*i) {
                if num_node.kind() == "unsigned_integer" {
                    if let Ok(count) = state.text(*num_node).parse::<u32>() {
                        *i += 1;
                        return count;
                    }
                }
            }
        }
    }
    1
}

/// Distribute a flat stream of figured bass entries across measures.
///
/// Walks the entries and measures in parallel, tracking cumulative duration.
/// When the accumulated duration fills a measure (based on the current time
/// signature), advances to the next measure. Figures land in whichever
/// measure their start time falls into.
///
/// Sets `FiguredBass.offset` to the figure's start position within its
/// measure in divisions (using `FIGURED_BASS_DIVISIONS` per quarter note).
/// This matches the MusicXML `<offset>` convention so round-trips preserve
/// alignment.
pub(super) fn distribute_figured_bass(measures: &mut [Measure], entries: &[FiguredBassEntry]) {
    use crate::ir::duration::Frac;

    if measures.is_empty() {
        return;
    }

    // Skip leading attribute-only measures (no voice content).
    // These will be merged into the first real-music measure by
    // merge_leading_attribute_measures during post-processing.
    let first_music = measures
        .iter()
        .position(|m| m.voices.iter().any(|v| !v.elements.is_empty()))
        .unwrap_or(0);

    // Track current time signature to know measure duration
    let mut measure_dur = Frac::new(4, 4); // default 4/4
    let mut measure_idx = first_music;
    let mut elapsed_in_measure = Frac::from_integer(0);

    // Update measure_dur from attributes up to and including the starting measure
    for m in &measures[..=measure_idx] {
        if let Some(ref attrs) = m.attributes {
            if let Some(ref ts) = attrs.time {
                measure_dur = ts.beats_fraction();
            }
        }
    }

    for entry in entries {
        // Advance to correct measure if we've exceeded current measure duration
        while elapsed_in_measure >= measure_dur && measure_idx + 1 < measures.len() {
            elapsed_in_measure -= measure_dur;
            measure_idx += 1;
            // Check if the new measure changes time signature
            if let Some(ref attrs) = measures[measure_idx].attributes {
                if let Some(ref ts) = attrs.time {
                    measure_dur = ts.beats_fraction();
                }
            }
        }

        match entry {
            FiguredBassEntry::Figure(fb) => {
                if measure_idx < measures.len() {
                    // Compute offset within the measure in divisions.
                    // actual_duration() is a fraction of a whole note;
                    // multiply by 4 to get quarter notes, then by divisions/quarter.
                    let offset_frac = elapsed_in_measure
                        * Frac::from_integer(4)
                        * Frac::from_integer(FIGURED_BASS_DIVISIONS);
                    let offset_divs = *offset_frac.numer() / *offset_frac.denom();
                    let mut fb_placed = fb.clone();
                    fb_placed.offset = offset_divs as i32;
                    measures[measure_idx].figured_bass.push(fb_placed);
                }
                elapsed_in_measure += fb.duration.actual_duration();
            }
            FiguredBassEntry::Skip(dur) => {
                elapsed_in_measure += dur.actual_duration();
            }
        }
    }
}
