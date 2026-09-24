//! Direct Music tree → LilyPond emission.
//!
//! Converts a [`MusicDocument`] to LilyPond source text without going through
//! the Layer 2 Score representation. This preserves structural information
//! (contexts, sequential/simultaneous blocks, variables) that would be lost
//! in a Score→lift round-trip.

use crate::ir::annotation::Annotation;
use crate::ir::articulation::{LyricSyllable, StartStop, SyllabicType};
use crate::ir::language::{PitchLanguage, PitchMode};
use crate::ir::music::{ContextType, Music, MusicDocument, RepeatType};
use crate::ir::pitch::Pitch;
use crate::ir::score::ScoreMetadata;

use super::maps::{
    articulation_to_ly, clef_to_ly, duration_to_ly, key_to_ly, ornament_to_ly, pitch_to_ly,
    tempo_to_ly, time_to_ly,
};

/// State tracked during Music tree emission.
struct EmitCtx {
    lang: PitchLanguage,
    mode: PitchMode,
    prev_pitch: Option<Pitch>,
    indent: usize,
    /// Inside a single staff, where `<< >>` of sequential blocks means voices.
    in_staff: bool,
}

impl EmitCtx {
    fn new(lang: PitchLanguage, mode: PitchMode) -> Self {
        Self {
            lang,
            mode,
            prev_pitch: None,
            indent: 0,
            in_staff: false,
        }
    }

    fn pad(&self) -> String {
        "  ".repeat(self.indent)
    }
}

/// Emit a complete MusicDocument as LilyPond source.
pub(super) fn emit_music_document(
    doc: &MusicDocument,
    version: &str,
    lang: PitchLanguage,
    mode: PitchMode,
) -> String {
    let mut lines: Vec<String> = Vec::new();

    // Preamble
    lines.push(format!("\\version \"{version}\""));
    lines.push(format!("\\language \"{}\"", lang.as_str()));
    lines.push(String::new());

    // Header
    emit_header(&doc.metadata, &mut lines);

    // Music content. Always emit absolute pitches: this emitter does not wrap
    // output in `\relative { }`, so emitting relative octave marks would be
    // misread on re-parse (octaves shift). Absolute is unambiguous and
    // round-trips. (`mode` is accepted for API symmetry but intentionally
    // overridden here.)
    let _ = mode;
    let mut ctx = EmitCtx::new(lang, PitchMode::Absolute);
    emit_music(&doc.music, &mut ctx, &mut lines);

    lines.push(String::new());
    lines.join("\n")
}

/// Emit metadata as a `\header` block.
fn emit_header(meta: &ScoreMetadata, lines: &mut Vec<String>) {
    let esc = super::helpers::escape_ly_string;
    let mut header_lines: Vec<String> = Vec::new();

    if let Some(ref title) = meta.title {
        header_lines.push(format!("  title = \"{}\"", esc(title)));
    }
    if let Some(ref composer) = meta.composer {
        header_lines.push(format!("  composer = \"{}\"", esc(composer)));
    }
    if let Some(ref subtitle) = meta.subtitle {
        header_lines.push(format!("  subtitle = \"{}\"", esc(subtitle)));
    }
    if let Some(ref arranger) = meta.arranger {
        header_lines.push(format!("  arranger = \"{}\"", esc(arranger)));
    }

    if !header_lines.is_empty() {
        lines.push("\\header {".to_string());
        lines.extend(header_lines);
        lines.push("}".to_string());
        lines.push(String::new());
    }
}

/// Emit a Music node recursively.
fn emit_music(music: &Music, ctx: &mut EmitCtx, lines: &mut Vec<String>) {
    match music {
        Music::Sequential(children) => {
            emit_sequential(children, ctx, lines);
        }
        Music::Simultaneous(children) => {
            emit_simultaneous(children, ctx, lines);
        }
        Music::Context {
            context_type,
            name,
            content,
        } => {
            emit_context(context_type, name.as_deref(), content, ctx, lines);
        }
        Music::Note {
            pitch,
            duration,
            annotations,
        } => {
            let p = pitch_to_ly(pitch, ctx.lang, ctx.prev_pitch.as_ref(), ctx.mode);
            let d = duration_to_ly(duration);
            let a = annotations_to_ly(annotations);
            lines.push(format!("{}{p}{d}{a}", ctx.pad()));
            ctx.prev_pitch = Some(*pitch);
        }
        Music::Chord {
            pitches,
            duration,
            annotations,
        } => {
            let mut pitch_strs = Vec::new();
            for (pitch, _per_note_ann) in pitches {
                let p = pitch_to_ly(pitch, ctx.lang, ctx.prev_pitch.as_ref(), ctx.mode);
                pitch_strs.push(p);
                ctx.prev_pitch = Some(*pitch);
            }
            let d = duration_to_ly(duration);
            let a = annotations_to_ly(annotations);
            lines.push(format!("{}<{}>{d}{a}", ctx.pad(), pitch_strs.join(" ")));
        }
        Music::Rest {
            duration,
            is_measure_rest,
        } => {
            let d = duration_to_ly(duration);
            let prefix = if *is_measure_rest { "R" } else { "r" };
            lines.push(format!("{}{prefix}{d}", ctx.pad()));
        }
        Music::Skip { duration } => {
            let d = duration_to_ly(duration);
            lines.push(format!("{}s{d}", ctx.pad()));
        }
        Music::TimeSignature(ts) => {
            lines.push(format!("{}{}", ctx.pad(), time_to_ly(ts)));
        }
        Music::KeySignature(ks) => {
            lines.push(format!("{}{}", ctx.pad(), key_to_ly(ks, ctx.lang)));
        }
        Music::Clef(clef) => {
            lines.push(format!("{}{}", ctx.pad(), clef_to_ly(clef)));
        }
        Music::Tempo(tempo) => {
            let t = tempo_to_ly(tempo);
            if !t.is_empty() {
                lines.push(format!("{}{t}", ctx.pad()));
            }
        }
        Music::Barline(barline) => {
            emit_barline(barline, ctx, lines);
        }
        Music::Direction(dir) => {
            emit_direction(dir, ctx, lines);
        }
        Music::Grace { content, slash } => {
            let cmd = if *slash {
                "\\acciaccatura"
            } else {
                "\\appoggiatura"
            };
            // For single-note grace, emit inline
            if matches!(content.as_ref(), Music::Note { .. }) {
                let saved_indent = ctx.indent;
                ctx.indent = 0;
                let mut inner_lines = Vec::new();
                emit_music(content, ctx, &mut inner_lines);
                let inner = inner_lines.join("").trim().to_string();
                ctx.indent = saved_indent;
                lines.push(format!("{}{cmd} {inner}", ctx.pad()));
            } else {
                lines.push(format!("{}{cmd} {{", ctx.pad()));
                ctx.indent += 1;
                emit_music(content, ctx, lines);
                ctx.indent -= 1;
                lines.push(format!("{}}}", ctx.pad()));
            }
        }
        Music::Tuplet {
            normal,
            actual,
            content,
        } => {
            lines.push(format!("{}\\tuplet {}/{} {{", ctx.pad(), actual, normal));
            ctx.indent += 1;
            emit_music(content, ctx, lines);
            ctx.indent -= 1;
            lines.push(format!("{}}}", ctx.pad()));
        }
        Music::Repeat {
            repeat_type,
            count,
            body,
            alternatives,
        } => {
            let type_str = match repeat_type {
                RepeatType::Volta => "volta",
                RepeatType::Unfold => "unfold",
                RepeatType::Percent => "percent",
                RepeatType::Tremolo => "tremolo",
            };
            lines.push(format!("{}\\repeat {} {} {{", ctx.pad(), type_str, count));
            ctx.indent += 1;
            emit_music(body, ctx, lines);
            ctx.indent -= 1;
            lines.push(format!("{}}}", ctx.pad()));

            if !alternatives.is_empty() {
                lines.push(format!("{}\\alternative {{", ctx.pad()));
                ctx.indent += 1;
                for alt in alternatives {
                    emit_music(alt, ctx, lines);
                }
                ctx.indent -= 1;
                lines.push(format!("{}}}", ctx.pad()));
            }
        }
        Music::Variable { name: _, content } => {
            // Emit the variable content inline (the definition is handled separately)
            emit_music(content, ctx, lines);
        }
        Music::FiguredBass(fb) => {
            // Simplified figured bass emission
            let d = duration_to_ly(&fb.duration);
            let figs: Vec<String> = fb
                .figures
                .iter()
                .map(|f| {
                    let num = f.number.map(|n| n.to_string()).unwrap_or_default();
                    let alter = match (&f.prefix, &f.suffix) {
                        (_, Some(s)) | (Some(s), _) => match s.as_str() {
                            "sharp" | "cross" => "+",
                            "flat" => "-",
                            "natural" => "!",
                            _ => "",
                        },
                        _ => "",
                    };
                    format!("{num}{alter}")
                })
                .collect();
            lines.push(format!("{}<{}>{d}", ctx.pad(), figs.join(" ")));
        }
        Music::Harmony(harmony) => {
            // Simplified harmony emission
            let root = &harmony.root;
            let root_str = root.step.to_lowercase();
            lines.push(format!("{}{root_str}", ctx.pad()));
        }
        Music::Lyric(syl) => {
            lines.push(format!("{}{}", ctx.pad(), syl.text));
        }
    }
}

/// Emit a sequential block `{ ... }`.
fn emit_sequential(children: &[Music], ctx: &mut EmitCtx, lines: &mut Vec<String>) {
    // For a sequential block that contains only leaf events, emit on fewer lines
    if children.is_empty() {
        lines.push(format!("{}{{ }}", ctx.pad()));
        return;
    }

    let all_leaves = children.iter().all(is_leaf);
    if all_leaves && children.len() <= 8 {
        // Compact single-line emission for short sequences
        let saved_indent = ctx.indent;
        ctx.indent = 0;
        let mut inner_lines = Vec::new();
        for child in children {
            emit_music(child, ctx, &mut inner_lines);
        }
        ctx.indent = saved_indent;
        let inner: Vec<&str> = inner_lines.iter().map(|l| l.trim()).collect();
        lines.push(format!("{}{{ {} }}", ctx.pad(), inner.join(" ")));
    } else {
        lines.push(format!("{}{{", ctx.pad()));
        ctx.indent += 1;
        for child in children {
            emit_music(child, ctx, lines);
        }
        ctx.indent -= 1;
        lines.push(format!("{}}}", ctx.pad()));
    }
}

/// Emit a simultaneous block `<< ... >>`.
fn emit_simultaneous(children: &[Music], ctx: &mut EmitCtx, lines: &mut Vec<String>) {
    if children.is_empty() {
        lines.push(format!("{}<<>>", ctx.pad()));
        return;
    }

    // Voices sharing a staff are separated with `\\`, which gives each its own
    // Voice context (LilyPond's idiom, and what the reader splits voices on).
    // Without it both streams land in one voice and re-parse mis-bars them.
    let voices = ctx.in_staff
        && children.len() > 1
        && children.iter().all(|c| matches!(c, Music::Sequential(_)));

    lines.push(format!("{}<<", ctx.pad()));
    ctx.indent += 1;
    for (i, child) in children.iter().enumerate() {
        if voices && i > 0 {
            lines.push(format!("{}\\\\", ctx.pad()));
        }
        emit_music(child, ctx, lines);
    }
    ctx.indent -= 1;
    lines.push(format!("{}>>", ctx.pad()));
}

/// Emit a context: `\new Type = "name" { content }`.
fn emit_context(
    ctx_type: &ContextType,
    name: Option<&str>,
    content: &Music,
    ctx: &mut EmitCtx,
    lines: &mut Vec<String>,
) {
    let type_name = ctx_type.ly_name();
    let name_part = match name {
        Some(n) => format!(" = \"{n}\""),
        None => String::new(),
    };

    let outer_in_staff = ctx.in_staff;
    ctx.in_staff = matches!(
        ctx_type,
        ContextType::Staff | ContextType::Voice | ContextType::TabStaff | ContextType::TabVoice
    );

    // Check if content is a simple sequential block
    match content {
        Music::Sequential(children) => {
            lines.push(format!("{}\\new {type_name}{name_part} {{", ctx.pad()));
            ctx.indent += 1;
            for child in children {
                emit_music(child, ctx, lines);
            }
            ctx.indent -= 1;
            lines.push(format!("{}}}", ctx.pad()));
        }
        _ => {
            lines.push(format!("{}\\new {type_name}{name_part}", ctx.pad()));
            ctx.indent += 1;
            emit_music(content, ctx, lines);
            ctx.indent -= 1;
        }
    }
    ctx.in_staff = outer_in_staff;

    // Emit lyrics that were attached to notes in this voice/staff. LilyPond's
    // `\addlyrics` block attaches to the immediately preceding context, so we
    // emit it right after the block at the same indentation. The lift pass wraps
    // single voices directly in a Staff (no Voice context), so we handle both.
    if matches!(ctx_type, ContextType::Voice | ContextType::Staff) {
        emit_addlyrics(content, ctx, lines);
    }
}

/// Recursively collect lyric syllables attached to notes/chords, grouped by
/// verse number, preserving order of appearance.
fn collect_lyrics(music: &Music, out: &mut std::collections::BTreeMap<u8, Vec<LyricSyllable>>) {
    let push_anns =
        |anns: &[Annotation], out: &mut std::collections::BTreeMap<u8, Vec<LyricSyllable>>| {
            for ann in anns {
                if let Annotation::Lyric(syl) = ann {
                    out.entry(syl.number).or_default().push(syl.clone());
                }
            }
        };
    match music {
        Music::Note { annotations, .. } => push_anns(annotations, out),
        Music::Chord { annotations, .. } => push_anns(annotations, out),
        Music::Sequential(children) | Music::Simultaneous(children) => {
            for c in children {
                collect_lyrics(c, out);
            }
        }
        // Nested contexts are a boundary: a deeper Staff/Voice emits its own
        // `\addlyrics`, so we must not collect through it (avoids double-counting
        // a PianoStaff's child staves).
        Music::Context { .. } => {}
        Music::Grace { content, .. } => collect_lyrics(content, out),
        Music::Tuplet { content, .. } => collect_lyrics(content, out),
        Music::Repeat {
            body, alternatives, ..
        } => {
            collect_lyrics(body, out);
            for alt in alternatives {
                collect_lyrics(alt, out);
            }
        }
        _ => {}
    }
}

/// Emit `\addlyrics { ... }` blocks for lyrics carried on a voice's notes.
fn emit_addlyrics(content: &Music, ctx: &EmitCtx, lines: &mut Vec<String>) {
    let mut by_verse = std::collections::BTreeMap::new();
    collect_lyrics(content, &mut by_verse);
    if by_verse.is_empty() {
        return;
    }

    for syllables in by_verse.values() {
        let mut tokens: Vec<String> = Vec::new();
        for syl in syllables {
            let text = escape_lyric(&syl.text);
            match syl.syllabic {
                SyllabicType::Begin | SyllabicType::Middle => {
                    tokens.push(text);
                    tokens.push("--".to_string());
                }
                SyllabicType::End | SyllabicType::Single => tokens.push(text),
            }
            if syl.extend {
                tokens.push("__".to_string());
            }
        }
        lines.push(format!(
            "{}\\addlyrics {{ {} }}",
            ctx.pad(),
            tokens.join(" ")
        ));
    }
}

/// Quote lyric text if it contains spaces or special characters.
fn escape_lyric(text: &str) -> String {
    if text.contains(' ') || text.contains('"') || text.contains('\\') {
        format!("\"{}\"", text.replace('\\', "\\\\").replace('"', "\\\""))
    } else {
        text.to_string()
    }
}

/// Emit a barline.
fn emit_barline(
    barline: &crate::ir::direction::Barline,
    ctx: &mut EmitCtx,
    lines: &mut Vec<String>,
) {
    use crate::ir::direction::BarlineType;
    let style = match barline.style {
        BarlineType::Regular => {
            lines.push(format!("{}|", ctx.pad()));
            return;
        }
        BarlineType::Double => "||",
        BarlineType::Final => "|.",
        BarlineType::RepeatForward => ".|:",
        BarlineType::RepeatBackward => ":|.",
        BarlineType::RepeatBoth => ":|.|:",
        BarlineType::Dashed => "!",
        BarlineType::Dotted => ";",
        BarlineType::Tick => "'",
        BarlineType::Short => ",",
        BarlineType::None => "",
    };
    if !style.is_empty() {
        lines.push(format!("{}\\bar \"{}\"", ctx.pad(), style));
    }
}

/// Emit a standalone direction.
fn emit_direction(
    dir: &crate::ir::direction::Direction,
    ctx: &mut EmitCtx,
    lines: &mut Vec<String>,
) {
    // Pedal markings
    if let Some(ref pedal) = dir.pedal {
        let cmd = match pedal.pedal_type.as_str() {
            "start" => "\\sustainOn",
            "stop" => "\\sustainOff",
            "change" => "\\sustainOff\\sustainOn",
            _ => "",
        };
        if !cmd.is_empty() {
            lines.push(format!("{}{cmd}", ctx.pad()));
        }
    }

    // Dynamic markings
    if let Some(ref dyn_mark) = dir.dynamic {
        lines.push(format!("{}\\{}", ctx.pad(), dyn_mark.sign));
    }

    // Wedges
    if let Some(ref wedge) = dir.wedge {
        let cmd = match wedge.wedge_type.as_str() {
            "crescendo" => "\\<",
            "diminuendo" => "\\>",
            "stop" => "\\!",
            _ => "",
        };
        if !cmd.is_empty() {
            lines.push(format!("{}{cmd}", ctx.pad()));
        }
    }

    // Text directions
    if let Some(ref text) = dir.text {
        if !text.text.is_empty() {
            lines.push(format!("{}^\"{}\"", ctx.pad(), text.text));
        }
    }
}

/// Convert annotations to LilyPond suffix string.
fn annotations_to_ly(annotations: &[Annotation]) -> String {
    let mut parts: Vec<String> = Vec::new();

    for ann in annotations {
        match ann {
            Annotation::Articulation(art) => {
                let ly = articulation_to_ly(&art.name);
                if !ly.is_empty() {
                    parts.push(ly.to_string());
                }
            }
            Annotation::Ornament(orn) => {
                let ly = ornament_to_ly(&orn.name);
                if !ly.is_empty() {
                    parts.push(ly.to_string());
                }
            }
            Annotation::Technical(tech) => match tech.name.as_str() {
                "fingering" => parts.push(format!("-{}", tech.value)),
                "up-bow" => parts.push("\\upbow".to_string()),
                "down-bow" => parts.push("\\downbow".to_string()),
                "open-string" => parts.push("\\open".to_string()),
                "snap-pizzicato" => parts.push("\\snappizzicato".to_string()),
                "harmonic" => parts.push("\\flageolet".to_string()),
                "stopped" => parts.push("-+".to_string()),
                _ => {}
            },
            Annotation::Dynamic(dyn_mark) => {
                parts.push(format!("\\{}", dyn_mark.sign));
            }
            Annotation::Wedge(wedge) => {
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
            Annotation::SlurStart { .. } => parts.push("(".to_string()),
            Annotation::SlurStop { .. } => parts.push(")".to_string()),
            Annotation::TieStart => parts.push("~".to_string()),
            Annotation::TieStop => {} // ties are implied
            Annotation::BeamStart => parts.push("[".to_string()),
            Annotation::BeamStop => parts.push("]".to_string()),
            Annotation::Fermata(_) => parts.push("\\fermata".to_string()),
            Annotation::Arpeggio(_) => parts.push("\\arpeggio".to_string()),
            Annotation::Glissando(StartStop::Start) => parts.push("\\glissando".to_string()),
            Annotation::Glissando(_) => {}
            Annotation::Tremolo { marks } => {
                let denom = 1u32 << (marks + 2); // marks=1 → :8, marks=2 → :16, etc.
                parts.push(format!(":{denom}"));
            }
            Annotation::PedalStart => parts.push("\\sustainOn".to_string()),
            Annotation::PedalStop => parts.push("\\sustainOff".to_string()),
            Annotation::PedalChange => {
                parts.push("\\sustainOff\\sustainOn".to_string());
            }
            Annotation::Text(text) => {
                if !text.text.is_empty() {
                    parts.push(format!("^\"{}\"", text.text));
                }
            }
            Annotation::Fingering(f) => parts.push(format!("-{f}")),
            Annotation::Lyric(_) => {} // lyrics handled separately
            Annotation::OctaveShift(os) => {
                let n = os.size;
                if os.shift_type == "up" || os.shift_type == "down" {
                    let dir = if n > 0 { n } else { -n };
                    parts.push(format!("\\ottava #{dir}"));
                } else {
                    parts.push("\\ottava #0".to_string());
                }
            }
        }
    }

    parts.join("")
}

/// Check if a Music node is a leaf (not a container).
fn is_leaf(music: &Music) -> bool {
    matches!(
        music,
        Music::Note { .. }
            | Music::Chord { .. }
            | Music::Rest { .. }
            | Music::Skip { .. }
            | Music::TimeSignature(_)
            | Music::KeySignature(_)
            | Music::Clef(_)
            | Music::Tempo(_)
            | Music::Barline(_)
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::annotation::Annotation;
    use crate::ir::articulation::{Articulation, DynamicMark, Placement};
    use crate::ir::duration::Duration;
    use crate::ir::language::{PitchLanguage, PitchMode};
    use crate::ir::measure::{Clef, ClefSign, KeyMode, KeySignature, TimeSignature};
    use crate::ir::music::{ContextType, Music, MusicDocument, RepeatType};
    use crate::ir::pitch::{Pitch, PitchStep};
    use crate::ir::score::ScoreMetadata;

    fn make_doc(music: Music) -> MusicDocument {
        MusicDocument {
            metadata: ScoreMetadata::default(),
            music,
        }
    }

    fn note(step: PitchStep, octave: i32) -> Music {
        Music::Note {
            pitch: Pitch::new(step, octave),
            duration: Duration::quarter(),
            annotations: vec![],
        }
    }

    #[test]
    fn test_emit_single_note() {
        let doc = make_doc(note(PitchStep::C, 4));
        let ly = emit_music_document(
            &doc,
            "2.24.0",
            PitchLanguage::Nederlands,
            PitchMode::Absolute,
        );
        assert!(ly.contains("c'4"), "expected c'4 in: {ly}");
    }

    #[test]
    fn test_emit_sequential() {
        let doc = make_doc(Music::Sequential(vec![
            note(PitchStep::C, 4),
            note(PitchStep::D, 4),
            note(PitchStep::E, 4),
        ]));
        let ly = emit_music_document(
            &doc,
            "2.24.0",
            PitchLanguage::Nederlands,
            PitchMode::Absolute,
        );
        assert!(ly.contains("c'4"), "expected c': {ly}");
        assert!(ly.contains("d'4"), "expected d': {ly}");
        assert!(ly.contains("e'4"), "expected e': {ly}");
    }

    #[test]
    fn test_emit_simultaneous() {
        let doc = make_doc(Music::Simultaneous(vec![
            Music::Sequential(vec![note(PitchStep::C, 4)]),
            Music::Sequential(vec![note(PitchStep::E, 4)]),
        ]));
        let ly = emit_music_document(
            &doc,
            "2.24.0",
            PitchLanguage::Nederlands,
            PitchMode::Absolute,
        );
        assert!(ly.contains("<<"), "expected << in: {ly}");
        assert!(ly.contains(">>"), "expected >> in: {ly}");
    }

    #[test]
    fn test_emit_context() {
        let doc = make_doc(Music::Context {
            context_type: ContextType::Staff,
            name: Some("rh".to_string()),
            content: Box::new(Music::Sequential(vec![note(PitchStep::C, 4)])),
        });
        let ly = emit_music_document(
            &doc,
            "2.24.0",
            PitchLanguage::Nederlands,
            PitchMode::Absolute,
        );
        assert!(
            ly.contains("\\new Staff = \"rh\""),
            "expected \\new Staff = \"rh\" in: {ly}"
        );
    }

    #[test]
    fn test_emit_rest_and_skip() {
        let doc = make_doc(Music::Sequential(vec![
            Music::Rest {
                duration: Duration::half(),
                is_measure_rest: false,
            },
            Music::Rest {
                duration: Duration::whole(),
                is_measure_rest: true,
            },
            Music::Skip {
                duration: Duration::quarter(),
            },
        ]));
        let ly = emit_music_document(
            &doc,
            "2.24.0",
            PitchLanguage::Nederlands,
            PitchMode::Absolute,
        );
        assert!(ly.contains("r2"), "expected r2 in: {ly}");
        assert!(ly.contains("R1"), "expected R1 in: {ly}");
        assert!(ly.contains("s4"), "expected s4 in: {ly}");
    }

    #[test]
    fn test_emit_chord() {
        let doc = make_doc(Music::Chord {
            pitches: vec![
                (Pitch::new(PitchStep::C, 4), vec![]),
                (Pitch::new(PitchStep::E, 4), vec![]),
                (Pitch::new(PitchStep::G, 4), vec![]),
            ],
            duration: Duration::half(),
            annotations: vec![],
        });
        let ly = emit_music_document(
            &doc,
            "2.24.0",
            PitchLanguage::Nederlands,
            PitchMode::Absolute,
        );
        assert!(ly.contains("<c' e' g'>2"), "expected <c' e' g'>2 in: {ly}");
    }

    #[test]
    fn test_emit_key_time_clef() {
        let doc = make_doc(Music::Sequential(vec![
            Music::KeySignature(KeySignature {
                fifths: -3,
                mode: KeyMode::Minor,
            }),
            Music::TimeSignature(TimeSignature {
                beats: "3".to_string(),
                beat_type: 4,
                symbol: None,
            }),
            Music::Clef(Clef {
                sign: ClefSign::F,
                line: 4,
                octave_change: 0,
            }),
            note(PitchStep::C, 3),
        ]));
        let ly = emit_music_document(
            &doc,
            "2.24.0",
            PitchLanguage::Nederlands,
            PitchMode::Absolute,
        );
        assert!(ly.contains("\\key"), "expected \\key in: {ly}");
        assert!(ly.contains("\\minor"), "expected \\minor in: {ly}");
        assert!(ly.contains("\\time 3/4"), "expected \\time 3/4 in: {ly}");
        assert!(ly.contains("\\clef"), "expected \\clef in: {ly}");
    }

    #[test]
    fn test_emit_grace_notes() {
        let doc = make_doc(Music::Sequential(vec![
            Music::Grace {
                content: Box::new(note(PitchStep::D, 5)),
                slash: true,
            },
            note(PitchStep::C, 5),
        ]));
        let ly = emit_music_document(
            &doc,
            "2.24.0",
            PitchLanguage::Nederlands,
            PitchMode::Absolute,
        );
        assert!(
            ly.contains("\\acciaccatura"),
            "expected \\acciaccatura in: {ly}"
        );
    }

    #[test]
    fn test_emit_tuplet() {
        let doc = make_doc(Music::Tuplet {
            normal: 2,
            actual: 3,
            content: Box::new(Music::Sequential(vec![
                note(PitchStep::C, 4),
                note(PitchStep::D, 4),
                note(PitchStep::E, 4),
            ])),
        });
        let ly = emit_music_document(
            &doc,
            "2.24.0",
            PitchLanguage::Nederlands,
            PitchMode::Absolute,
        );
        assert!(
            ly.contains("\\tuplet 3/2"),
            "expected \\tuplet 3/2 in: {ly}"
        );
    }

    #[test]
    fn test_emit_repeat_volta() {
        let doc = make_doc(Music::Repeat {
            repeat_type: RepeatType::Volta,
            count: 2,
            body: Box::new(Music::Sequential(vec![note(PitchStep::C, 4)])),
            alternatives: vec![
                Music::Sequential(vec![note(PitchStep::D, 4)]),
                Music::Sequential(vec![note(PitchStep::E, 4)]),
            ],
        });
        let ly = emit_music_document(
            &doc,
            "2.24.0",
            PitchLanguage::Nederlands,
            PitchMode::Absolute,
        );
        assert!(
            ly.contains("\\repeat volta 2"),
            "expected \\repeat volta 2 in: {ly}"
        );
        assert!(
            ly.contains("\\alternative"),
            "expected \\alternative in: {ly}"
        );
    }

    #[test]
    fn test_emit_annotations() {
        let doc = make_doc(Music::Note {
            pitch: Pitch::new(PitchStep::C, 4),
            duration: Duration::quarter(),
            annotations: vec![
                Annotation::Articulation(Articulation {
                    name: "staccato".to_string(),
                    placement: Placement::default(),
                }),
                Annotation::Dynamic(DynamicMark {
                    sign: "f".to_string(),
                    placement: Placement::default(),
                }),
                Annotation::SlurStart {
                    number: 1,
                    placement: Placement::default(),
                },
            ],
        });
        let ly = emit_music_document(
            &doc,
            "2.24.0",
            PitchLanguage::Nederlands,
            PitchMode::Absolute,
        );
        assert!(ly.contains("\\f"), "expected \\f dynamic in: {ly}");
        assert!(ly.contains("("), "expected slur start in: {ly}");
    }

    #[test]
    fn test_emit_header() {
        let mut doc = make_doc(note(PitchStep::C, 4));
        doc.metadata.title = Some("My Title".to_string());
        doc.metadata.composer = Some("J.S. Bach".to_string());
        let ly = emit_music_document(
            &doc,
            "2.24.0",
            PitchLanguage::Nederlands,
            PitchMode::Absolute,
        );
        assert!(ly.contains("\\header {"), "expected \\header in: {ly}");
        assert!(
            ly.contains("title = \"My Title\""),
            "expected title in: {ly}"
        );
        assert!(
            ly.contains("composer = \"J.S. Bach\""),
            "expected composer in: {ly}"
        );
    }

    #[test]
    fn test_emit_version_and_language() {
        let doc = make_doc(note(PitchStep::C, 4));
        let ly = emit_music_document(&doc, "2.24.0", PitchLanguage::English, PitchMode::Absolute);
        assert!(
            ly.contains("\\version \"2.24.0\""),
            "expected version in: {ly}"
        );
        assert!(
            ly.contains("\\language \"english\""),
            "expected english language in: {ly}"
        );
    }

    #[test]
    fn test_emit_barline() {
        use crate::ir::direction::{Barline, BarlineType};
        let doc = make_doc(Music::Sequential(vec![
            note(PitchStep::C, 4),
            Music::Barline(Barline {
                style: BarlineType::Double,
                ..Default::default()
            }),
            note(PitchStep::D, 4),
        ]));
        let ly = emit_music_document(
            &doc,
            "2.24.0",
            PitchLanguage::Nederlands,
            PitchMode::Absolute,
        );
        assert!(
            ly.contains("\\bar \"||\""),
            "expected \\bar \"||\" in: {ly}"
        );
    }

    #[test]
    fn test_emit_piano_staff() {
        let doc = make_doc(Music::Context {
            context_type: ContextType::PianoStaff,
            name: None,
            content: Box::new(Music::Simultaneous(vec![
                Music::Context {
                    context_type: ContextType::Staff,
                    name: Some("rh".to_string()),
                    content: Box::new(Music::Sequential(vec![note(PitchStep::C, 5)])),
                },
                Music::Context {
                    context_type: ContextType::Staff,
                    name: Some("lh".to_string()),
                    content: Box::new(Music::Sequential(vec![note(PitchStep::C, 3)])),
                },
            ])),
        });
        let ly = emit_music_document(
            &doc,
            "2.24.0",
            PitchLanguage::Nederlands,
            PitchMode::Absolute,
        );
        assert!(
            ly.contains("\\new PianoStaff"),
            "expected PianoStaff in: {ly}"
        );
        assert!(
            ly.contains("\\new Staff = \"rh\""),
            "expected rh Staff in: {ly}"
        );
        assert!(
            ly.contains("\\new Staff = \"lh\""),
            "expected lh Staff in: {ly}"
        );
    }

    #[test]
    fn test_round_trip_ly_music_ly() {
        // Parse LilyPond → Music tree → emit LilyPond, check semantic equivalence
        use crate::adapters::ly_to_ir::LyToIrAdapter;
        use crate::adapters::{FromMusicAdapter, ToMusicAdapter};

        let input = r#"{ c'4 d'4 e'4 f'4 }"#;
        let parser = LyToIrAdapter::new();
        let doc = parser.convert_str_to_music(input).unwrap();

        let emitter = super::super::IrToLyAdapter::new();
        let output = emitter.convert_music(&doc).unwrap();

        // All four notes should survive the round trip
        assert!(output.contains("c'"), "c' missing from: {output}");
        assert!(output.contains("d'"), "d' missing from: {output}");
        assert!(output.contains("e'"), "e' missing from: {output}");
        assert!(output.contains("f'"), "f' missing from: {output}");
    }

    #[test]
    fn test_round_trip_with_key_and_time() {
        use crate::adapters::ly_to_ir::LyToIrAdapter;
        use crate::adapters::{FromMusicAdapter, ToMusicAdapter};

        let input = r#"{ \key g \major \time 3/4 g'4 a' b' }"#;
        let parser = LyToIrAdapter::new();
        let doc = parser.convert_str_to_music(input).unwrap();

        let emitter = super::super::IrToLyAdapter::new();
        let output = emitter.convert_music(&doc).unwrap();

        assert!(output.contains("\\key"), "\\key missing from: {output}");
        assert!(output.contains("\\major"), "\\major missing from: {output}");
        assert!(
            output.contains("\\time 3/4"),
            "\\time 3/4 missing from: {output}"
        );
    }
}
