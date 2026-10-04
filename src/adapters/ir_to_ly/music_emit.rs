//! Direct Music tree → LilyPond emission.
//!
//! Converts a [`MusicDocument`] to LilyPond source text without going through
//! the Layer 2 Score representation. This preserves structural information
//! (contexts, sequential/simultaneous blocks, variables) that would be lost
//! in a Score→lift round-trip.

use crate::ir::annotation::Annotation;
use crate::ir::articulation::{Placement, StartStop};
use crate::ir::duration::Frac;
use crate::ir::harmony::Harmony;
use crate::ir::language::{PitchLanguage, PitchMode};
use crate::ir::music::{ContextType, Music, MusicDocument, RepeatType};
use crate::ir::pitch::Pitch;

use super::maps::{
    articulation_to_ly, clef_to_ly, duration_to_ly, key_to_ly, length_to_ly, ornament_to_ly,
    pitch_to_ly, tempo_to_ly, time_to_ly,
};

/// State tracked during Music tree emission.
struct EmitCtx {
    lang: PitchLanguage,
    mode: PitchMode,
    prev_pitch: Option<Pitch>,
    indent: usize,
    /// Inside a single staff, where `<< >>` of sequential blocks means voices.
    in_staff: bool,
    /// `\autoBeamOff` in force in the current voice.
    auto_beam_off: bool,
    /// `\mark \default` marks written so far.
    marks: u32,
}

impl EmitCtx {
    fn new(lang: PitchLanguage, mode: PitchMode) -> Self {
        Self {
            lang,
            mode,
            prev_pitch: None,
            indent: 0,
            in_staff: false,
            auto_beam_off: false,
            marks: 0,
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
    lines.extend(super::helpers::header_block(&doc.metadata));

    // Music content. Always emit absolute pitches: this emitter does not wrap
    // output in `\relative { }`, so emitting relative octave marks would be
    // misread on re-parse (octaves shift). Absolute is unambiguous and
    // round-trips. (`mode` is accepted for API symmetry but intentionally
    // overridden here.)
    let _ = mode;
    let mut ctx = EmitCtx::new(lang, PitchMode::Absolute);
    let music = match &doc.metadata.partial_duration {
        // The opening pickup: `\partial` in the first staff.
        Some(d) => std::borrow::Cow::Owned(with_pickup(&doc.music, d)),
        None => std::borrow::Cow::Borrowed(&doc.music),
    };
    // Chord symbols go in a ChordNames line alongside the music.
    let mut chords = Vec::new();
    chord_symbols(&music, Frac::from_integer(0), &mut chords);
    if chords.is_empty() {
        emit_music(&music, &mut ctx, &mut lines);
    } else {
        lines.push("<<".to_string());
        ctx.indent += 1;
        lines.push(format!("{}\\new ChordNames \\chordmode {{", ctx.pad()));
        let tokens = super::parts::chord_line(chords, music.written_length(), lang);
        for row in tokens.chunks(8) {
            lines.push(format!("{}  {}", ctx.pad(), row.join(" ")));
        }
        lines.push(format!("{}}}", ctx.pad()));
        emit_music(&music, &mut ctx, &mut lines);
        lines.push(">>".to_string());
    }

    lines.push(String::new());
    lines.join("\n")
}

/// The chord symbols in `m` and where they sound, from `at`, as printed
/// (repeats once).
fn chord_symbols<'a>(m: &'a Music, at: Frac, out: &mut Vec<(Frac, &'a Harmony)>) {
    match m {
        Music::Harmony(h) => out.push((at, h)),
        Music::Sequential(items) => {
            let mut t = at;
            for item in items {
                chord_symbols(item, t, out);
                t += item.written_length();
            }
        }
        Music::Simultaneous(items) => items.iter().for_each(|i| chord_symbols(i, at, out)),
        Music::Repeat {
            body, alternatives, ..
        } => {
            chord_symbols(body, at, out);
            let mut t = at + body.written_length();
            for alt in alternatives {
                chord_symbols(alt, t, out);
                t += alt.written_length();
            }
        }
        Music::Context { content, .. }
        | Music::Variable { content, .. }
        | Music::Tuplet { content, .. } => chord_symbols(content, at, out),
        _ => {}
    }
}

/// The music with a `\partial` opening its first staff, after the leading
/// `\time`/`\key`/`\clef` (a `\time` after `\partial` would reset it).
fn with_pickup(music: &Music, d: &crate::ir::duration::Duration) -> Music {
    fn insert(m: &mut Music, d: &crate::ir::duration::Duration) -> bool {
        match m {
            Music::Sequential(items) => {
                // Before the first music, or inside it when that is itself a
                // container (the staves of a score, a first multi-voice bar).
                let at = items
                    .iter()
                    .position(|c| {
                        !matches!(
                            c,
                            Music::TimeSignature(_)
                                | Music::KeySignature(_)
                                | Music::Clef(_)
                                | Music::Tempo(_)
                        )
                    })
                    .unwrap_or(items.len());
                let container = items.get(at).is_some_and(|c| {
                    matches!(
                        c,
                        Music::Context { .. }
                            | Music::Simultaneous(_)
                            | Music::Sequential(_)
                            | Music::Variable { .. }
                    )
                });
                // The music may already open with its `\partial`.
                let has = matches!(items.get(at), Some(Music::Partial(_)));
                if !(has || container && insert(&mut items[at], d)) {
                    items.insert(at, Music::Partial(d.clone()));
                }
                true
            }
            Music::Simultaneous(items) => items.iter_mut().any(|c| insert(c, d)),
            Music::Context { content, .. } | Music::Variable { content, .. } => insert(content, d),
            _ => false,
        }
    }
    let mut m = music.clone();
    if !insert(&mut m, d) {
        m = Music::Sequential(vec![Music::Partial(d.clone()), m]);
    }
    m
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
            // A new context auto-beams until told otherwise.
            let saved = std::mem::take(&mut ctx.auto_beam_off);
            emit_context(context_type, name.as_deref(), content, ctx, lines);
            ctx.auto_beam_off = saved;
        }
        Music::Note {
            pitch,
            duration,
            annotations,
        } => {
            beam_mode(annotations, ctx, lines);
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
            if let Some((_, lead)) = pitches.first() {
                beam_mode(lead, ctx, lines);
            }
            // Ties on every note are the chord's (`<g b>~`); on some, inside
            // it (`<g~ b>`).
            let tied = |a: &[Annotation]| a.contains(&Annotation::TieStart);
            let all_tied = !pitches.is_empty() && pitches.iter().all(|(_, a)| tied(a));
            let mut pitch_strs = Vec::new();
            for (pitch, per_note) in pitches {
                let p = pitch_to_ly(pitch, ctx.lang, ctx.prev_pitch.as_ref(), ctx.mode);
                let tie = if !all_tied && tied(per_note) { "~" } else { "" };
                pitch_strs.push(format!("{p}{tie}"));
                ctx.prev_pitch = Some(*pitch);
            }
            let d = duration_to_ly(duration);
            let mut a = annotations_to_ly(annotations);
            if all_tied && !tied(annotations) {
                a.push('~');
            }
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
        Music::Transposition(t) => {
            // What a written c' sounds (absolute, outside `\relative`).
            let p = pitch_to_ly(&t.sounding_c(), ctx.lang, None, PitchMode::Absolute);
            lines.push(format!("{}\\transposition {p}", ctx.pad()));
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
        Music::Partial(d) => {
            lines.push(format!("{}\\partial {}", ctx.pad(), length_to_ly(d)));
        }
        Music::Direction(dir) => {
            emit_direction(dir, ctx, lines);
        }
        Music::Grace { content, slash } => {
            // No slur implied: one the source has is written as a slur.
            let cmd = if *slash { "\\slashedGrace" } else { "\\grace" };
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
                // A group's notes go straight in the braces (`{ { … } }`
                // would be a second block).
                lines.push(format!("{}{cmd} {{", ctx.pad()));
                ctx.indent += 1;
                match content.as_ref() {
                    Music::Sequential(notes) => {
                        for m in notes {
                            emit_music(m, ctx, lines);
                        }
                    }
                    other => emit_music(other, ctx, lines),
                }
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
        // A chord symbol belongs in a `ChordNames` context, which this
        // writer doesn't build: inline it would be a note.
        Music::Harmony(_) => {}
        Music::Lyric(syl) => {
            lines.push(format!("{}{}", ctx.pad(), syl.text));
        }
    }
}

/// Emit a sequential block `{ ... }`.
/// Grace notes in a row become one group: LilyPond aborts on two grace
/// commands in a row (`is_grace_fixup_sane`). The group keeps the first's kind.
fn group_graces(children: &[Music]) -> Vec<Music> {
    let mut out: Vec<Music> = Vec::with_capacity(children.len());
    for m in children {
        let Music::Grace { content, .. } = m else {
            out.push(m.clone());
            continue;
        };
        let notes = |c: &Music| match c {
            Music::Sequential(v) => v.clone(),
            other => vec![other.clone()],
        };
        match out.last_mut() {
            Some(Music::Grace { content: group, .. }) => {
                // Moved, not cloned: a long run of graces stays linear.
                let mut all = match std::mem::replace(&mut **group, Music::Sequential(Vec::new())) {
                    Music::Sequential(v) => v,
                    other => vec![other],
                };
                all.extend(notes(content));
                **group = Music::Sequential(all);
            }
            _ => out.push(m.clone()),
        }
    }
    out
}

fn emit_sequential(children: &[Music], ctx: &mut EmitCtx, lines: &mut Vec<String>) {
    let grouped = group_graces(children);
    let children = grouped.as_slice();
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

/// `\autoBeamOff`/`\autoBeamOn` before a note whose beaming the source
/// did or didn't decide, when that changes.
fn beam_mode(annotations: &[Annotation], ctx: &mut EmitCtx, lines: &mut Vec<String>) {
    let off = annotations.contains(&Annotation::NoAutoBeam);
    if off != ctx.auto_beam_off {
        let cmd = if off { "\\autoBeamOff" } else { "\\autoBeamOn" };
        lines.push(format!("{}{cmd}", ctx.pad()));
        ctx.auto_beam_off = off;
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
    let saved = ctx.auto_beam_off;
    if voices && children.iter().any(has_lyrics) {
        // The voice with lyrics goes on through the block (every `\\` voice
        // is new, and lyrics skip it); the others are new voices.
        let lead = lyric_branch(children);
        let order = std::iter::once(lead).chain((0..children.len()).filter(|&i| i != lead));
        for (k, i) in order.enumerate() {
            let cmd = ["\\voiceOne", "\\voiceTwo", "\\voiceThree", "\\voiceFour"][k.min(3)];
            let new = if k > 0 { "\\new Voice " } else { "" };
            lines.push(format!("{}{new}{{ {cmd}", ctx.pad()));
            ctx.indent += 1;
            if k > 0 {
                ctx.auto_beam_off = false;
            }
            for child in &group_graces(children[i].children()) {
                emit_music(child, ctx, lines);
            }
            ctx.indent -= 1;
            lines.push(format!("{}}}", ctx.pad()));
        }
        ctx.auto_beam_off = saved;
        ctx.indent -= 1;
        lines.push(format!("{}>>", ctx.pad()));
        lines.push(format!("{}\\oneVoice", ctx.pad()));
        return;
    }
    for (i, child) in children.iter().enumerate() {
        if voices && i > 0 {
            lines.push(format!("{}\\\\", ctx.pad()));
        }
        // Each `\\` voice is a new context.
        if voices {
            ctx.auto_beam_off = false;
        }
        emit_music(child, ctx, lines);
    }
    ctx.auto_beam_off = saved;
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
        Some(n) => format!(" = \"{}\"", super::helpers::escape_ly_string(n)),
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
            for child in &group_graces(children) {
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

/// A note's or chord's annotations (a chord's lyrics are on its first note).
fn lead_annotations(m: &Music) -> Option<&[Annotation]> {
    match m {
        Music::Note { annotations, .. } => Some(annotations),
        Music::Chord {
            pitches,
            annotations,
            ..
        } => Some(match pitches.first() {
            Some((_, a)) if a.iter().any(|x| matches!(x, Annotation::Lyric(_))) => a,
            _ => annotations,
        }),
        _ => None,
    }
}

/// Whether music (not looking into other contexts) has lyrics.
fn has_lyrics(m: &Music) -> bool {
    match m {
        Music::Context { .. } => false,
        Music::Note { .. } | Music::Chord { .. } => {
            lead_annotations(m).is_some_and(|a| a.iter().any(|x| matches!(x, Annotation::Lyric(_))))
        }
        other => other.children().iter().any(has_lyrics) || other.inner().is_some_and(has_lyrics),
    }
}

/// The branch of a block of voices that carries the lyrics: the first with
/// some, else the first.
fn lyric_branch(children: &[Music]) -> usize {
    children.iter().position(has_lyrics).unwrap_or(0)
}

/// The notes a context's lyrics are sung on, in order, as the music is
/// written: in a block of voices the one with lyrics; not grace notes, not
/// rests, not other contexts.
fn sung<'a>(m: &'a Music, out: &mut Vec<&'a [Annotation]>) {
    match m {
        Music::Note { .. } | Music::Chord { .. } => out.extend(lead_annotations(m)),
        Music::Sequential(items) => items.iter().for_each(|c| sung(c, out)),
        Music::Simultaneous(items) if items.iter().all(|c| matches!(c, Music::Sequential(_))) => {
            if let Some(branch) = items.get(lyric_branch(items)) {
                sung(branch, out);
            }
        }
        Music::Tuplet { content, .. } | Music::Variable { content, .. } => sung(content, out),
        Music::Repeat {
            body, alternatives, ..
        } => {
            sung(body, out);
            alternatives.iter().for_each(|a| sung(a, out));
        }
        _ => {}
    }
}

/// Emit `\addlyrics { … }` for the lyrics on a context's notes: one per
/// verse, a token per sung note (`_` where the verse has no syllable),
/// melismata as the notes say (`ignoreMelismata`).
fn emit_addlyrics(content: &Music, ctx: &EmitCtx, lines: &mut Vec<String>) {
    let mut notes = Vec::new();
    sung(content, &mut notes);
    let verses: std::collections::BTreeSet<u8> = notes
        .iter()
        .flat_map(|a| a.iter())
        .filter_map(|x| match x {
            Annotation::Lyric(s) => Some(s.number),
            _ => None,
        })
        .collect();
    for verse in verses {
        let mut tokens: Vec<String> = vec!["\\set ignoreMelismata = ##t".to_string()];
        let name = notes.iter().flat_map(|a| a.iter()).find_map(|x| match x {
            Annotation::Lyric(s) if s.number == verse => s.name.as_ref(),
            _ => None,
        });
        if let Some(name) = name {
            let name = super::helpers::escape_ly_string(name);
            tokens.push(format!("\\set stanza = \"{name}\""));
        }
        for a in &notes {
            let syl = a.iter().find_map(|x| match x {
                Annotation::Lyric(s) if s.number == verse => Some(s),
                _ => None,
            });
            tokens.push(syl.map_or_else(|| "_".to_string(), super::lyrics::syllable_to_ly));
        }
        while tokens.last().is_some_and(|t| t == "_") {
            tokens.pop();
        }
        lines.push(format!(
            "{}\\addlyrics {{ {} }}",
            ctx.pad(),
            tokens.join(" ")
        ));
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
    use super::helpers::escape_ly_string;
    use crate::ir::direction::LayoutBreakType;
    let pad = ctx.pad();
    // Commands, at this moment.
    if let Some(r) = &dir.rehearsal {
        lines.push(format!(
            "{pad}{}",
            super::helpers::rehearsal_to_ly(&r.text, &mut ctx.marks)
        ));
    }
    if dir.segno {
        lines.push(format!(
            "{pad}\\mark \\markup {{ \\musicglyph \"scripts.segno\" }}"
        ));
    }
    if dir.coda {
        lines.push(format!(
            "{pad}\\mark \\markup {{ \\musicglyph \"scripts.coda\" }}"
        ));
    }
    for jump in dir.da_capo.iter().chain(&dir.dal_segno) {
        lines.push(format!("{pad}\\mark \"{}\"", escape_ly_string(jump)));
    }
    if let Some(oct) = &dir.octave_shift {
        if matches!(oct.shift_type.as_str(), "up" | "down" | "stop") {
            lines.push(format!("{pad}\\ottava #{}", oct.octaves()));
        }
    }
    if let Some(lb) = &dir.layout_break {
        lines.push(format!(
            "{pad}{}",
            match lb {
                LayoutBreakType::System => "\\break",
                LayoutBreakType::Page => "\\pageBreak",
                LayoutBreakType::Section => "\\section",
            }
        ));
    }
    // Marks on a moment rather than a note: `<>` carries them here (written
    // after the note before, they were that note's).
    let mut post = String::new();
    if let Some(pedal) = &dir.pedal {
        post.push_str(match pedal.pedal_type.as_str() {
            "start" => "\\sustainOn",
            "stop" => "\\sustainOff",
            "change" => "\\sustainOff\\sustainOn",
            _ => "",
        });
    }
    if let Some(dyn_mark) = &dir.dynamic {
        post.push_str(&super::maps::dynamic_to_ly(&dyn_mark.sign));
    }
    if let Some(wedge) = &dir.wedge {
        post.push_str(match wedge.wedge_type.as_str() {
            "crescendo" => "\\<",
            "diminuendo" => "\\>",
            "stop" => "\\!",
            _ => "",
        });
    }
    if let Some(text) = dir.text.as_ref().filter(|t| !t.text.is_empty()) {
        let at = if dir.placement == Placement::Below || text.placement == Placement::Below {
            '_'
        } else {
            '^'
        };
        post.push_str(&format!(
            "{at}\\markup {{ \"{}\" }}",
            escape_ly_string(&text.text)
        ));
    }
    if !post.is_empty() {
        lines.push(format!("{pad}<>{post}"));
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
                parts.push(super::maps::dynamic_to_ly(&dyn_mark.sign));
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
            Annotation::NoAutoBeam => {} // \autoBeamOff, before the note
            Annotation::Fermata(_) => parts.push("\\fermata".to_string()),
            Annotation::Arpeggio(_) => parts.push("\\arpeggio".to_string()),
            Annotation::Glissando(StartStop::Start) => parts.push("\\glissando".to_string()),
            Annotation::Glissando(_) => {}
            Annotation::Tremolo { marks } => {
                // marks=1 → :8, marks=2 → :16, etc.
                let denom = 1u32 << ((*marks).min(crate::ir::note::MAX_TREMOLO_MARKS) + 2);
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
            Annotation::Lyric(_) => {}    // lyrics handled separately
            Annotation::Velocity(_) => {} // performance data, no notation
            Annotation::OctaveShift(os) => parts.push(format!("\\ottava #{}", os.octaves())),
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
            | Music::Partial(_)
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
            ly.contains("\\slashedGrace"),
            "expected \\slashedGrace in: {ly}"
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
