use crate::ir::articulation::{
    Articulation, BeamEvent, DynamicMark, Fermata, Placement, SlurEvent, StartStop, Technical,
    TieEvent, Wedge,
};
use crate::ir::direction::TextDirection;
use crate::ir::note::{Chord, Note, Rest, VoiceElement};

use super::consume::is_dynamic_name;
use super::merge::beam_level_for_duration;
use super::state::WalkState;

pub(super) fn apply_note_attachments(
    _state: &mut WalkState,
    note: &mut Note,
    attachments: &[String],
) {
    for att in attachments {
        match att.as_str() {
            "[" => {
                // Start of manual beam group
                _state.in_beam_group = true;
                // Beam level depends on note duration: 8th=1, 16th=2, 32nd=3, 64th=4
                let level = beam_level_for_duration(&note.duration);
                if level > 0 {
                    note.beams.push(BeamEvent {
                        beam_type: "begin".to_string(),
                        number: 1,
                    });
                }
                continue;
            }
            "]" => {
                // End of manual beam group
                _state.in_beam_group = false;
                let level = beam_level_for_duration(&note.duration);
                if level > 0 {
                    note.beams.push(BeamEvent {
                        beam_type: "end".to_string(),
                        number: 1,
                    });
                }
                continue;
            }
            "~" => {
                note.ties.push(TieEvent {
                    tie_type: StartStop::Start,
                });
            }
            "(" => {
                note.slurs.push(SlurEvent {
                    slur_type: StartStop::Start,
                    number: 1,
                    placement: Placement::Unspecified,
                });
            }
            ")" => {
                note.slurs.push(SlurEvent {
                    slur_type: StartStop::Stop,
                    number: 1,
                    placement: Placement::Unspecified,
                });
            }
            "\\fermata" => {
                note.fermata = Some(Fermata {
                    shape: "normal".to_string(),
                    inverted: false,
                });
            }
            "\\glissando" => {
                if _state.pending_slide {
                    note.slide = Some(StartStop::Start);
                    _state.pending_slide = false;
                } else {
                    note.glissando = Some(StartStop::Start);
                    if let Some(style) = _state.pending_glissando_style.take() {
                        note.glissando_line_type = Some(style);
                    }
                }
            }
            "\\arpeggio" => {
                // Arpeggio on a single note — unusual but valid in LilyPond
            }
            s if is_dynamic_name(s) => {
                let sign = s.trim_start_matches('\\').to_string();
                note.dynamics.push(DynamicMark {
                    sign,
                    placement: Placement::Unspecified,
                });
            }
            "\\<" | "\\crescendo" => {
                note.wedges.push(Wedge {
                    wedge_type: "crescendo".to_string(),
                    placement: Placement::Unspecified,
                });
            }
            "\\>" | "\\diminuendo" | "\\decrescendo" => {
                note.wedges.push(Wedge {
                    wedge_type: "diminuendo".to_string(),
                    placement: Placement::Unspecified,
                });
            }
            "\\!" => {
                note.wedges.push(Wedge {
                    wedge_type: "stop".to_string(),
                    placement: Placement::Unspecified,
                });
            }
            "\\trill" => {
                note.ornaments.push(crate::ir::articulation::Ornament {
                    name: "trill-mark".to_string(),
                    placement: Placement::Unspecified,
                });
            }
            "\\mordent" => {
                note.ornaments.push(crate::ir::articulation::Ornament {
                    name: "mordent".to_string(),
                    placement: Placement::Unspecified,
                });
            }
            "\\prall" => {
                note.ornaments.push(crate::ir::articulation::Ornament {
                    name: "inverted-mordent".to_string(),
                    placement: Placement::Unspecified,
                });
            }
            "\\turn" => {
                note.ornaments.push(crate::ir::articulation::Ornament {
                    name: "turn".to_string(),
                    placement: Placement::Unspecified,
                });
            }
            "\\reverseturn" => {
                note.ornaments.push(crate::ir::articulation::Ornament {
                    name: "inverted-turn".to_string(),
                    placement: Placement::Unspecified,
                });
            }
            "\\staccato" => {
                note.articulations.push(Articulation {
                    name: "staccato".to_string(),
                    placement: Placement::Unspecified,
                });
            }
            "\\tenuto" => {
                note.articulations.push(Articulation {
                    name: "tenuto".to_string(),
                    placement: Placement::Unspecified,
                });
            }
            "\\accent" => {
                note.articulations.push(Articulation {
                    name: "accent".to_string(),
                    placement: Placement::Unspecified,
                });
            }
            "\\marcato" => {
                note.articulations.push(Articulation {
                    name: "strong-accent".to_string(),
                    placement: Placement::Unspecified,
                });
            }
            "\\staccatissimo" => {
                note.articulations.push(Articulation {
                    name: "staccatissimo".to_string(),
                    placement: Placement::Unspecified,
                });
            }
            "\\portato" => {
                note.articulations.push(Articulation {
                    name: "detached-legato".to_string(),
                    placement: Placement::Unspecified,
                });
            }
            "\\stopped" => {
                note.technicals.push(Technical {
                    name: "stopped".to_string(),
                    value: String::new(),
                });
            }
            "\\breathe" => {
                note.articulations.push(Articulation {
                    name: "breath-mark".to_string(),
                    placement: Placement::Unspecified,
                });
            }
            s if s.starts_with("text:") => {
                // "text:above:pizz." or "text:below:arco"
                let rest = &s[5..];
                if let Some((placement_str, text)) = rest.split_once(':') {
                    let placement = match placement_str {
                        "above" => Placement::Above,
                        "below" => Placement::Below,
                        _ => Placement::Unspecified,
                    };
                    note.text_directions.push(TextDirection {
                        text: text.to_string(),
                        placement,
                        font_style: None,
                        font_weight: None,
                    });
                }
            }
            _ => {}
        }
    }
}

pub(super) fn apply_rest_attachments(rest: &mut Rest, attachments: &[String]) {
    for att in attachments {
        if att == "\\fermata" {
            rest.fermata = Some(Fermata {
                shape: "normal".to_string(),
                inverted: false,
            });
        }
    }
}

pub(super) fn apply_chord_attachments(
    _state: &mut WalkState,
    chord: &mut Chord,
    attachments: &[String],
) {
    for att in attachments {
        match att.as_str() {
            "\\arpeggio" => {
                // Apply pending arpeggio type, defaulting to Up
                chord.arpeggio = Some(
                    _state
                        .pending_arpeggio_type
                        .take()
                        .unwrap_or(crate::ir::note::ArpeggioType::Up),
                );
                continue;
            }
            "\\glissando" => {
                // Glissando on a chord — apply to first note
                if !chord.notes.is_empty() {
                    if _state.pending_slide {
                        chord.notes[0].slide = Some(StartStop::Start);
                        _state.pending_slide = false;
                    } else {
                        chord.notes[0].glissando = Some(StartStop::Start);
                        if let Some(style) = _state.pending_glissando_style.take() {
                            chord.notes[0].glissando_line_type = Some(style);
                        }
                    }
                }
                continue;
            }
            _ => {}
        }
    }
    if chord.notes.is_empty() {
        return;
    }
    // Apply remaining attachments to first note only (as is convention)
    let first = &mut chord.notes[0];
    for att in attachments {
        match att.as_str() {
            "[" => {
                _state.in_beam_group = true;
                let level = beam_level_for_duration(&chord.duration);
                if level > 0 {
                    first.beams.push(BeamEvent {
                        beam_type: "begin".to_string(),
                        number: 1,
                    });
                }
                continue;
            }
            "]" => {
                _state.in_beam_group = false;
                let level = beam_level_for_duration(&chord.duration);
                if level > 0 {
                    first.beams.push(BeamEvent {
                        beam_type: "end".to_string(),
                        number: 1,
                    });
                }
                continue;
            }
            "~" => {
                first.ties.push(TieEvent {
                    tie_type: StartStop::Start,
                });
            }
            "(" => {
                first.slurs.push(SlurEvent {
                    slur_type: StartStop::Start,
                    number: 1,
                    placement: Placement::Unspecified,
                });
            }
            ")" => {
                first.slurs.push(SlurEvent {
                    slur_type: StartStop::Stop,
                    number: 1,
                    placement: Placement::Unspecified,
                });
            }
            "\\fermata" => {
                first.fermata = Some(Fermata {
                    shape: "normal".to_string(),
                    inverted: false,
                });
            }
            s if is_dynamic_name(s) => {
                let sign = s.trim_start_matches('\\').to_string();
                first.dynamics.push(DynamicMark {
                    sign,
                    placement: Placement::Unspecified,
                });
            }
            "\\<" | "\\crescendo" => {
                first.wedges.push(Wedge {
                    wedge_type: "crescendo".to_string(),
                    placement: Placement::Unspecified,
                });
            }
            "\\>" | "\\diminuendo" | "\\decrescendo" => {
                first.wedges.push(Wedge {
                    wedge_type: "diminuendo".to_string(),
                    placement: Placement::Unspecified,
                });
            }
            "\\!" => {
                first.wedges.push(Wedge {
                    wedge_type: "stop".to_string(),
                    placement: Placement::Unspecified,
                });
            }
            "\\trill" => {
                first.ornaments.push(crate::ir::articulation::Ornament {
                    name: "trill-mark".to_string(),
                    placement: Placement::Unspecified,
                });
            }
            "\\mordent" => {
                first.ornaments.push(crate::ir::articulation::Ornament {
                    name: "mordent".to_string(),
                    placement: Placement::Unspecified,
                });
            }
            "\\prall" => {
                first.ornaments.push(crate::ir::articulation::Ornament {
                    name: "inverted-mordent".to_string(),
                    placement: Placement::Unspecified,
                });
            }
            "\\turn" => {
                first.ornaments.push(crate::ir::articulation::Ornament {
                    name: "turn".to_string(),
                    placement: Placement::Unspecified,
                });
            }
            "\\reverseturn" => {
                first.ornaments.push(crate::ir::articulation::Ornament {
                    name: "inverted-turn".to_string(),
                    placement: Placement::Unspecified,
                });
            }
            "\\staccato" => {
                first.articulations.push(Articulation {
                    name: "staccato".to_string(),
                    placement: Placement::Unspecified,
                });
            }
            "\\tenuto" => {
                first.articulations.push(Articulation {
                    name: "tenuto".to_string(),
                    placement: Placement::Unspecified,
                });
            }
            "\\accent" => {
                first.articulations.push(Articulation {
                    name: "accent".to_string(),
                    placement: Placement::Unspecified,
                });
            }
            "\\marcato" => {
                first.articulations.push(Articulation {
                    name: "strong-accent".to_string(),
                    placement: Placement::Unspecified,
                });
            }
            "\\staccatissimo" => {
                first.articulations.push(Articulation {
                    name: "staccatissimo".to_string(),
                    placement: Placement::Unspecified,
                });
            }
            "\\portato" => {
                first.articulations.push(Articulation {
                    name: "detached-legato".to_string(),
                    placement: Placement::Unspecified,
                });
            }
            "\\stopped" => {
                first.technicals.push(Technical {
                    name: "stopped".to_string(),
                    value: String::new(),
                });
            }
            "\\breathe" => {
                first.articulations.push(Articulation {
                    name: "breath-mark".to_string(),
                    placement: Placement::Unspecified,
                });
            }
            s if s.starts_with("text:") => {
                // "text:above:pizz." or "text:below:arco"
                let rest = &s[5..];
                if let Some((placement_str, text)) = rest.split_once(':') {
                    let placement = match placement_str {
                        "above" => Placement::Above,
                        "below" => Placement::Below,
                        _ => Placement::Unspecified,
                    };
                    first.text_directions.push(TextDirection {
                        text: text.to_string(),
                        placement,
                        font_style: None,
                        font_weight: None,
                    });
                }
            }
            _ => {}
        }
    }
}

/// Attach a dynamic mark to the most recent note or chord in the current voice.
pub(super) fn attach_dynamic(state: &mut WalkState, dyn_text: &str) {
    let sign = dyn_text.trim_start_matches('\\').to_string();
    let target = match state.current_voice.last_mut() {
        Some(VoiceElement::Note(note)) => Some(note.as_mut()),
        Some(VoiceElement::Chord(chord)) => chord.notes.first_mut(),
        _ => None,
    };
    if let Some(note) = target {
        if sign == "<" {
            note.wedges.push(Wedge {
                wedge_type: "crescendo".to_string(),
                placement: Placement::Unspecified,
            });
        } else if sign == ">" {
            note.wedges.push(Wedge {
                wedge_type: "diminuendo".to_string(),
                placement: Placement::Unspecified,
            });
        } else if sign == "!" {
            note.wedges.push(Wedge {
                wedge_type: "stop".to_string(),
                placement: Placement::Unspecified,
            });
        } else {
            note.dynamics.push(DynamicMark {
                sign,
                placement: Placement::Unspecified,
            });
        }
    }
}

/// Attach a fermata to the most recent note or rest.
pub(super) fn attach_fermata(state: &mut WalkState) {
    match state.current_voice.last_mut() {
        Some(VoiceElement::Note(note)) => {
            note.fermata = Some(Fermata {
                shape: "normal".to_string(),
                inverted: false,
            });
        }
        Some(VoiceElement::Rest(rest)) => {
            rest.fermata = Some(Fermata {
                shape: "normal".to_string(),
                inverted: false,
            });
        }
        _ => {}
    }
}

/// Attach an articulation by name to the most recent note.
pub(super) fn attach_articulation(state: &mut WalkState, name: &str) {
    if let Some(VoiceElement::Note(note)) = state.current_voice.last_mut() {
        note.articulations.push(Articulation {
            name: name.to_string(),
            placement: Placement::Unspecified,
        });
    }
}
