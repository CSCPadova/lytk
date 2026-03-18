//! IR -> LilyPond emitter.
//!
//! Converts an IR [`Score`] into LilyPond source text.
//!
//! # Reference
//! Ported from the Python prototype `lytk-py/converters/ir_to_ly.py` and
//! helper functions in `lytk-py/converters/ly_emitter.py`.

mod emit;
mod helpers;
mod lyrics;
mod maps;
mod parts;
#[cfg(test)]
mod tests;

use crate::ir::language::{PitchLanguage, PitchMode};
use crate::ir::music::MusicDocument;
use crate::ir::note::VoiceElement;
use crate::ir::score::{PartGroup, Score, ScoreChild};
use crate::ir::voice::Voice;
use crate::ir::Part;

use super::{FromIrAdapter, Result};

use helpers::{part_var_name, roman};
use lyrics::{emit_lyrics_refs, emit_lyrics_variable, part_has_lyrics};
use parts::{emit_figured_bass_variable, emit_harmony_variable, emit_part_variable};

// ---------------------------------------------------------------------------
// Adapter
// ---------------------------------------------------------------------------

/// IR -> LilyPond adapter.
///
/// Converts a [`Score`] into LilyPond source text.
///
/// # Example
/// ```ignore
/// use lytk::adapters::ir_to_ly::IrToLyAdapter;
/// use lytk::adapters::FromIrAdapter;
///
/// let adapter = IrToLyAdapter::new();
/// let ly_text = adapter.convert(&score)?;
/// ```
pub struct IrToLyAdapter {
    language: PitchLanguage,
    mode: PitchMode,
    version: String,
}

impl IrToLyAdapter {
    pub fn new() -> Self {
        Self {
            language: PitchLanguage::Nederlands,
            mode: PitchMode::Absolute,
            version: "2.24.0".to_string(),
        }
    }

    /// Set the pitch language.
    pub fn with_language(mut self, lang: PitchLanguage) -> Self {
        self.language = lang;
        self
    }

    /// Set the pitch mode (absolute / relative).
    pub fn with_mode(mut self, mode: PitchMode) -> Self {
        self.mode = mode;
        self
    }

    /// Set the LilyPond version string.
    pub fn with_version(mut self, version: &str) -> Self {
        self.version = version.to_string();
        self
    }
}

impl Default for IrToLyAdapter {
    fn default() -> Self {
        Self::new()
    }
}

impl FromIrAdapter for IrToLyAdapter {
    fn convert(&self, score: &Score) -> Result<String> {
        // Resolve language: prefer score metadata, fall back to adapter config
        let lang = score
            .metadata
            .pitch_language
            .unwrap_or(self.language);
        let mode = score.metadata.pitch_mode;

        let mut lines: Vec<String> = Vec::new();

        // Preamble
        emit_preamble(score, &self.version, lang, &mut lines);

        // Part variables
        for part in score.parts() {
            emit_part_variable(
                part,
                lang,
                mode,
                score.metadata.partial_duration.as_ref(),
                &mut lines,
            );
            emit_harmony_variable(part, &mut lines);
            emit_figured_bass_variable(part, &mut lines);
            emit_lyrics_variable(part, &mut lines);
        }

        // Score block
        emit_score_block(score, &mut lines);

        lines.push(String::new()); // trailing newline
        Ok(lines.join("\n"))
    }
}

impl super::FromMusicAdapter for IrToLyAdapter {
    fn convert_music(&self, doc: &MusicDocument) -> Result<String> {
        let score = crate::ir::lower::lower_to_score(doc);
        self.convert(&score)
    }
}

// ---------------------------------------------------------------------------
// Emission helpers
// ---------------------------------------------------------------------------

fn emit_preamble(
    score: &Score,
    version: &str,
    lang: PitchLanguage,
    lines: &mut Vec<String>,
) {
    lines.push(format!("\\version \"{version}\""));
    lines.push(format!("\\language \"{}\"", lang.as_str()));
    lines.push(String::new());

    let meta = &score.metadata;
    let has_header = meta.title.is_some()
        || meta.composer.is_some()
        || meta.arranger.is_some()
        || meta.lyricist.is_some();

    if has_header {
        lines.push("\\header {".to_string());
        if let Some(t) = &meta.title {
            lines.push(format!("  title = \"{t}\""));
        }
        if let Some(s) = &meta.subtitle {
            lines.push(format!("  subtitle = \"{s}\""));
        }
        if let Some(c) = &meta.composer {
            lines.push(format!("  composer = \"{c}\""));
        }
        if let Some(a) = &meta.arranger {
            lines.push(format!("  arranger = \"{a}\""));
        }
        if let Some(l) = &meta.lyricist {
            lines.push(format!("  poet = \"{l}\""));
        }
        for (key, val) in &meta.extra {
            lines.push(format!("  {key} = \"{val}\""));
        }
        lines.push("}".to_string());
        lines.push(String::new());
    }

    // Paper block / page layout
    emit_paper(score, lines);
}

fn emit_paper(score: &Score, lines: &mut Vec<String>) {
    let layout = match &score.page_layout {
        Some(l) => l,
        None => return,
    };

    if let Some(size) = layout.staff_size {
        lines.push(format!("#(set-global-staff-size {size:.1})"));
        lines.push(String::new());
    }

    let mut paper_lines: Vec<String> = Vec::new();
    if let Some(h) = layout.page_height {
        paper_lines.push(format!("  page-height = {h:.2}\\cm"));
    }
    if let Some(w) = layout.page_width {
        paper_lines.push(format!("  page-width = {w:.2}\\cm"));
    }
    if let Some(v) = layout.left_margin {
        paper_lines.push(format!("  left-margin = {v:.2}\\cm"));
    }
    if let Some(v) = layout.right_margin {
        paper_lines.push(format!("  right-margin = {v:.2}\\cm"));
    }
    if let Some(v) = layout.top_margin {
        paper_lines.push(format!("  top-margin = {v:.2}\\cm"));
    }
    if let Some(v) = layout.bottom_margin {
        paper_lines.push(format!("  bottom-margin = {v:.2}\\cm"));
    }
    if let Some(v) = layout.system_distance {
        paper_lines.push(format!("  system-system-spacing.basic-distance = #{v:.1}"));
    }
    if let Some(v) = layout.top_system_distance {
        paper_lines.push(format!("  top-system-spacing.basic-distance = #{v:.1}"));
    }

    if !paper_lines.is_empty() {
        lines.push("\\paper {".to_string());
        lines.extend(paper_lines);
        lines.push("}".to_string());
        lines.push(String::new());
    }
}

fn emit_score_block(score: &Score, lines: &mut Vec<String>) {
    lines.push("\\score {".to_string());
    lines.push("  <<".to_string());

    // Group parts by whether they appear in a PartGroup
    let mut emitted_ids: std::collections::HashSet<String> = std::collections::HashSet::new();

    for child in &score.children {
        match child {
            ScoreChild::PartGroup(group) => {
                emit_part_group_ref(group, 4, lines, &mut emitted_ids);
            }
            ScoreChild::Part(part) => {
                if emitted_ids.insert(part.part_id.clone()) {
                    emit_part_ref(part, 4, lines);
                }
            }
        }
    }

    lines.push("  >>".to_string());
    lines.push("  \\layout { }".to_string());
    lines.push("  \\midi { }".to_string());
    lines.push("}".to_string());
}

fn emit_part_group_ref(
    group: &PartGroup,
    indent: usize,
    lines: &mut Vec<String>,
    emitted: &mut std::collections::HashSet<String>,
) {
    let pad = " ".repeat(indent);
    let context = if group.group_type.is_empty() {
        "StaffGroup"
    } else {
        &group.group_type
    };
    lines.push(format!("{pad}\\new {context} <<"));

    for child in &group.children {
        match child {
            ScoreChild::Part(part) => {
                if emitted.insert(part.part_id.clone()) {
                    emit_part_ref(part, indent + 2, lines);
                }
            }
            ScoreChild::PartGroup(sub) => {
                emit_part_group_ref(sub, indent + 2, lines, emitted);
            }
        }
    }

    lines.push(format!("{pad}>>"));
}

fn emit_part_ref(part: &Part, indent: usize, lines: &mut Vec<String>) {
    let pad = " ".repeat(indent);
    let var = part_var_name(part);

    // ChordNames context (if harmonies exist)
    let has_harmonies = part.measures.iter().any(|m| !m.harmonies.is_empty());
    if has_harmonies {
        let chords_var = format!("{var}Chords");
        lines.push(format!("{pad}\\new ChordNames \\{chords_var}"));
    }

    // FiguredBass context (if figured bass exists)
    let has_figures = part.measures.iter().any(|m| !m.figured_bass.is_empty());
    if has_figures {
        let figures_var = format!("{var}Figures");
        lines.push(format!("{pad}\\new FiguredBass \\{figures_var}"));
    }

    let has_lyrics = part_has_lyrics(part);
    let voice_name = var.clone();

    if part.staves > 1 {
        lines.push(format!("{pad}\\new PianoStaff <<"));
        for staff_num in 1..=part.staves {
            let staff_var = format!("{}Staff{}", var, roman(staff_num));
            lines.push(format!(
                "{pad}  \\new Staff = \"{} {}\" \\{staff_var}",
                part.name, staff_num
            ));
        }
        lines.push(format!("{pad}>>"));
    } else if has_lyrics {
        // When lyrics exist, wrap in << ... >> with a named Voice so \lyricsto can target it
        if !part.name.is_empty() {
            lines.push(format!(
                "{pad}\\new Staff \\with {{ instrumentName = \"{}\" }} <<",
                part.name
            ));
        } else {
            lines.push(format!("{pad}\\new Staff <<"));
        }
        lines.push(format!(
            "{pad}  \\new Voice = \"{voice_name}\" \\{var}"
        ));
        emit_lyrics_refs(part, &voice_name, indent + 2, lines);
        lines.push(format!("{pad}>>"));
    } else if !part.name.is_empty() {
        lines.push(format!(
            "{pad}\\new Staff \\with {{ instrumentName = \"{}\" }} \\{var}",
            part.name
        ));
    } else {
        lines.push(format!("{pad}\\new Staff \\{var}"));
    }
}

/// Returns true if the voice contains any real music content (notes, rests, chords),
/// not just timing elements (forward/backup).
fn voice_has_content(voice: &Voice) -> bool {
    voice.elements.iter().any(|e| matches!(e,
        VoiceElement::Note(_) | VoiceElement::Rest(_) | VoiceElement::Chord(_)
    ))
}

fn voice_matches_staff(voice: &Voice, staff_num: u8) -> bool {
    let mut has_staff_info = false;
    for elem in &voice.elements {
        match elem {
            VoiceElement::Note(n) => {
                if n.staff == staff_num {
                    return true;
                }
                if n.staff != 0 {
                    has_staff_info = true;
                }
            }
            VoiceElement::Rest(r) => {
                if r.staff == staff_num {
                    return true;
                }
                if r.staff != 0 {
                    has_staff_info = true;
                }
            }
            VoiceElement::Chord(c) => {
                if c.staff == staff_num {
                    return true;
                }
                if c.staff != 0 {
                    has_staff_info = true;
                }
            }
            VoiceElement::Forward(f) => {
                if f.staff == staff_num {
                    return true;
                }
                if f.staff != 0 {
                    has_staff_info = true;
                }
            }
            VoiceElement::Backup(_) => {}
        }
    }
    // If we found staff info but nothing matched, this voice belongs to a different staff
    !has_staff_info
}
