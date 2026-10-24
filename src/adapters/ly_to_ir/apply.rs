use crate::ir::articulation::{
    Articulation, ArticulationType, BeamEvent, BeamValue, DynamicMark, Fermata, FermataShape,
    OrnamentType, Placement, SlurEvent, StartStop, Technical, TechnicalType, TieEvent, Wedge,
    WedgeType,
};
use crate::ir::direction::{Direction, TextDirection};
use crate::ir::note::{Chord, Note, Rest, VoiceElement};

use super::consume::is_dynamic_name;
use super::merge::beam_level_for_duration;
use super::state::WalkState;

pub(super) fn apply_note_attachments(
    _state: &mut WalkState,
    note: &mut Note,
    attachments: &[String],
) {
    // `^`/`_` before a mark places it (above, below); one mark each.
    let mut placed = Placement::Unspecified;
    for att in attachments {
        let here = std::mem::replace(&mut placed, Placement::Unspecified);
        match att.as_str() {
            "^" => placed = Placement::Above,
            "_" => placed = Placement::Below,
            "[" => {
                // Start of manual beam group
                _state.in_beam_group = true;
                // Beam level depends on note duration: 8th=1, 16th=2, 32nd=3, 64th=4
                let level = beam_level_for_duration(&note.duration);
                if level > 0 {
                    note.beams.push(BeamEvent {
                        beam_type: BeamValue::Begin,
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
                        beam_type: BeamValue::End,
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
                // `^(`, else `\\slurUp`'s direction
                let placement = match here {
                    Placement::Unspecified => _state.slur_placement,
                    p => p,
                };
                note.slurs.push(SlurEvent {
                    slur_type: StartStop::Start,
                    number: 1,
                    placement,
                });
            }
            ")" => {
                note.slurs.push(SlurEvent {
                    slur_type: StartStop::Stop,
                    number: 1,
                    placement: here,
                });
            }
            "\\fermata" => {
                note.fermata = Some(Fermata {
                    shape: FermataShape::Normal,
                    inverted: here == Placement::Below,
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
            // Not in an automatic beam: the source decided.
            "\\noBeam" => note.no_auto_beam = true,
            s if is_dynamic_name(s) || s.starts_with("dynamic:") => {
                let sign = s.trim_start_matches("dynamic:").trim_start_matches('\\');
                note.dynamics.push(DynamicMark {
                    sign: sign.into(),
                    placement: here,
                });
            }
            "\\<" | "\\crescendo" => {
                note.wedges.push(Wedge {
                    wedge_type: WedgeType::Crescendo,
                    placement: here,
                });
            }
            "\\>" | "\\diminuendo" | "\\decrescendo" => {
                note.wedges.push(Wedge {
                    wedge_type: WedgeType::Diminuendo,
                    placement: here,
                });
            }
            "\\!" => {
                note.wedges.push(Wedge {
                    wedge_type: WedgeType::Stop,
                    placement: here,
                });
            }
            "\\trill" => {
                note.ornaments.push(crate::ir::articulation::Ornament {
                    name: OrnamentType::TrillMark,
                    placement: here,
                });
            }
            "\\mordent" => {
                note.ornaments.push(crate::ir::articulation::Ornament {
                    name: OrnamentType::Mordent,
                    placement: here,
                });
            }
            "\\prall" => {
                note.ornaments.push(crate::ir::articulation::Ornament {
                    name: OrnamentType::InvertedMordent,
                    placement: here,
                });
            }
            "\\turn" => {
                note.ornaments.push(crate::ir::articulation::Ornament {
                    name: OrnamentType::Turn,
                    placement: here,
                });
            }
            "\\reverseturn" => {
                note.ornaments.push(crate::ir::articulation::Ornament {
                    name: OrnamentType::InvertedTurn,
                    placement: here,
                });
            }
            "\\staccato" => {
                note.articulations.push(Articulation {
                    name: ArticulationType::Staccato,
                    placement: here,
                });
            }
            "\\tenuto" => {
                note.articulations.push(Articulation {
                    name: ArticulationType::Tenuto,
                    placement: here,
                });
            }
            "\\accent" => {
                note.articulations.push(Articulation {
                    name: ArticulationType::Accent,
                    placement: here,
                });
            }
            "\\marcato" => {
                note.articulations.push(Articulation {
                    name: ArticulationType::StrongAccent,
                    placement: here,
                });
            }
            "\\staccatissimo" => {
                note.articulations.push(Articulation {
                    name: ArticulationType::Staccatissimo,
                    placement: here,
                });
            }
            "\\portato" => {
                note.articulations.push(Articulation {
                    name: ArticulationType::DetachedLegato,
                    placement: here,
                });
            }
            "\\stopped" => {
                note.technicals.push(Technical {
                    name: TechnicalType::Stopped,
                    value: String::new(),
                });
            }
            "\\upbow" => {
                note.technicals.push(Technical {
                    name: TechnicalType::UpBow,
                    value: String::new(),
                });
            }
            "\\downbow" => {
                note.technicals.push(Technical {
                    name: TechnicalType::DownBow,
                    value: String::new(),
                });
            }
            "\\flageolet" | "\\open" => {
                note.technicals.push(Technical {
                    name: TechnicalType::OpenString,
                    value: String::new(),
                });
            }
            "\\snappizzicato" => {
                note.technicals.push(Technical {
                    name: TechnicalType::SnapPizzicato,
                    value: String::new(),
                });
            }
            "\\breathe" => {
                note.articulations.push(Articulation {
                    name: ArticulationType::BreathMark,
                    placement: here,
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
            s if s.starts_with("finger:") => {
                let value = s["finger:".len()..].to_string();
                note.technicals.push(Technical {
                    name: TechnicalType::Fingering,
                    value,
                });
            }
            _ => {}
        }
    }
}

pub(super) fn apply_rest_attachments(rest: &mut Rest, attachments: &[String]) {
    for (k, att) in attachments.iter().enumerate() {
        if att == "\\fermata" {
            rest.fermata = Some(Fermata {
                shape: FermataShape::Normal,
                inverted: k > 0 && attachments[k - 1] == "_",
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
    // \arpeggio and \glissando were already handled above for the chord; apply
    // every other attachment to the first note exactly as for a plain note
    // (a chord and its notes share one duration, so beam levels match).
    let rest: Vec<String> = attachments
        .iter()
        .filter(|a| a.as_str() != "\\arpeggio" && a.as_str() != "\\glissando")
        .cloned()
        .collect();
    apply_note_attachments(_state, &mut chord.notes[0], &rest);
    // `<d fis>8~` ties every note of the chord, not just the first.
    if attachments.iter().any(|a| a == "~") {
        for n in &mut chord.notes[1..] {
            if !n.ties.iter().any(|t| t.tie_type == StartStop::Start) {
                n.ties.push(TieEvent {
                    tie_type: StartStop::Start,
                });
            }
        }
    }
}

/// Attach a dynamic mark to the most recent note or chord in the current voice.
pub(super) fn attach_dynamic(state: &mut WalkState, dyn_text: &str) {
    let sign = dyn_text.trim_start_matches('\\').to_string();
    match state.current_voice.last_mut() {
        Some(VoiceElement::Note(note)) => {
            attach_dynamic_sign_to_note(note.as_mut(), sign);
        }
        Some(VoiceElement::Chord(chord)) => {
            if let Some(note) = chord.notes.first_mut() {
                attach_dynamic_sign_to_note(note, sign);
            }
        }
        Some(VoiceElement::Rest(rest)) => {
            // Attach directly to the rest so the dynamic survives voice-element
            // re-splitting (e.g. when the spacer variable is later re-binned into
            // measures with a different time signature).
            if sign == "<" {
                rest.wedges.push(Wedge {
                    wedge_type: WedgeType::Crescendo,
                    placement: Placement::Unspecified,
                });
            } else if sign == ">" {
                rest.wedges.push(Wedge {
                    wedge_type: WedgeType::Diminuendo,
                    placement: Placement::Unspecified,
                });
            } else if sign == "!" {
                rest.wedges.push(Wedge {
                    wedge_type: WedgeType::Stop,
                    placement: Placement::Unspecified,
                });
            } else {
                rest.dynamics.push(DynamicMark {
                    sign: sign.into(),
                    placement: Placement::Unspecified,
                });
            }
        }
        _ => {
            // No voice element to attach to — emit as measure-level Direction.
            let dir = if sign == "<" {
                Direction {
                    placement: Placement::Below,
                    wedge: Some(Wedge {
                        wedge_type: WedgeType::Crescendo,
                        placement: Placement::Below,
                    }),
                    ..Default::default()
                }
            } else if sign == ">" {
                Direction {
                    placement: Placement::Below,
                    wedge: Some(Wedge {
                        wedge_type: WedgeType::Diminuendo,
                        placement: Placement::Below,
                    }),
                    ..Default::default()
                }
            } else if sign == "!" {
                Direction {
                    placement: Placement::Below,
                    wedge: Some(Wedge {
                        wedge_type: WedgeType::Stop,
                        placement: Placement::Below,
                    }),
                    ..Default::default()
                }
            } else {
                Direction {
                    placement: Placement::Below,
                    dynamic: Some(DynamicMark {
                        sign: sign.into(),
                        placement: Placement::Below,
                    }),
                    ..Default::default()
                }
            };
            state.add_event(crate::ir::timeline::Event::direction(dir));
        }
    }
}

fn attach_dynamic_sign_to_note(note: &mut crate::ir::note::Note, sign: String) {
    if sign == "<" {
        note.wedges.push(Wedge {
            wedge_type: WedgeType::Crescendo,
            placement: Placement::Unspecified,
        });
    } else if sign == ">" {
        note.wedges.push(Wedge {
            wedge_type: WedgeType::Diminuendo,
            placement: Placement::Unspecified,
        });
    } else if sign == "!" {
        note.wedges.push(Wedge {
            wedge_type: WedgeType::Stop,
            placement: Placement::Unspecified,
        });
    } else {
        note.dynamics.push(DynamicMark {
            sign: sign.into(),
            placement: Placement::Unspecified,
        });
    }
}

/// Attach a fermata to the most recent note or rest.
pub(super) fn attach_fermata(state: &mut WalkState) {
    match state.current_voice.last_mut() {
        Some(VoiceElement::Note(note)) => {
            note.fermata = Some(Fermata {
                shape: FermataShape::Normal,
                inverted: false,
            });
        }
        Some(VoiceElement::Rest(rest)) => {
            rest.fermata = Some(Fermata {
                shape: FermataShape::Normal,
                inverted: false,
            });
        }
        _ => {}
    }
}

/// Attach an articulation to the most recent note.
pub(super) fn attach_articulation(state: &mut WalkState, name: ArticulationType) {
    if let Some(VoiceElement::Note(note)) = state.current_voice.last_mut() {
        note.articulations.push(Articulation {
            name,
            placement: Placement::Unspecified,
        });
    }
}

/// Attach an ornament to the most recent note.
pub(super) fn attach_ornament(state: &mut WalkState, name: OrnamentType) {
    if let Some(VoiceElement::Note(note)) = state.current_voice.last_mut() {
        note.ornaments.push(crate::ir::articulation::Ornament {
            name,
            placement: Placement::Unspecified,
        });
    }
}
