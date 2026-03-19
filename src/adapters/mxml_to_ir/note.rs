//! Note, chord, rest, pitch, notations, and lyric parsing.

use crate::ir::articulation::*;
use crate::ir::duration::Duration;
use crate::ir::note::*;
use crate::ir::pitch::{AccidentalDisplay, Alter, Pitch, PitchStep};

use super::helpers::XmlNode;

// ---------------------------------------------------------------------------
// Note parsing
// ---------------------------------------------------------------------------

pub(super) enum NoteOrRest {
    Note(Box<Note>),
    Rest(Rest),
}

pub(super) fn parse_note(elem: &XmlNode, divisions: i64) -> Option<NoteOrRest> {
    let is_rest = elem.find("rest").is_some();
    let is_grace = elem.find("grace").is_some();
    let voice_num = elem.child_i64("voice", 1) as u8;
    let staff_num = elem.child_i64("staff", 1) as u8;

    // Duration
    let dots = elem.find_all("dot").len() as u8;
    let type_name = elem.child_text("type");

    let duration = if is_grace {
        let tn = type_name.unwrap_or("eighth");
        Duration::from_musicxml_type(tn, dots).unwrap_or_default()
    } else if let Some(dur_elem) = elem.find("duration") {
        let dur_val = dur_elem.text_i64(0);
        if let Some(tn) = type_name {
            Duration::from_musicxml_type(tn, dots)
                .unwrap_or_else(|| Duration::from_divisions(dur_val, divisions, dots))
        } else {
            Duration::from_divisions(dur_val, divisions, dots)
        }
    } else {
        Duration::default()
    };

    // Tuplet scaling
    let duration = if let Some(time_mod) = elem.find("time-modification") {
        let actual = time_mod.child_i64("actual-notes", 1) as u8;
        let normal = time_mod.child_i64("normal-notes", 1) as u8;
        Duration {
            tuplet_normal: normal,
            tuplet_actual: actual,
            ..duration
        }
    } else {
        duration
    };

    if is_rest {
        let rest_elem = elem.find("rest").unwrap();
        let display_step = rest_elem.child_text("display-step").map(|s| s.to_string());
        let display_octave = rest_elem
            .find("display-octave")
            .map(|o| o.text_i64(0) as i32);
        let is_measure_rest = rest_elem.attr("measure") == Some("yes");

        let mut rest = Rest {
            duration,
            voice: voice_num,
            staff: staff_num,
            display_step,
            display_octave,
            is_measure_rest,
            is_spacer: false,
            fermata: None,
            tuplet: None,
        };

        // Check for fermata and tuplet display in notations.
        if let Some(notations) = elem.find("notations") {
            rest.fermata = parse_fermata(notations);
            if let Some(tuplet) = notations.find("tuplet") {
                let tuplet_type = match tuplet.attr("type").unwrap_or("start") {
                    "start" => StartStop::Start,
                    "stop" => StartStop::Stop,
                    _ => StartStop::Start,
                };
                let bracket = tuplet.attr("bracket") == Some("yes");
                let show_number = tuplet.attr("show-number").unwrap_or("actual").to_string();
                rest.tuplet = Some(TupletDisplay {
                    tuplet_type,
                    bracket,
                    show_number,
                });
            }
        }

        return Some(NoteOrRest::Rest(rest));
    }

    // Pitched note
    let pitch = if let Some(pitch_elem) = elem.find("pitch") {
        parse_pitch(pitch_elem)?
    } else if let Some(unpitched) = elem.find("unpitched") {
        // Percussion: use display-step/display-octave.
        let step_str = unpitched.child_text("display-step").unwrap_or("C");
        let step = PitchStep::from_name(step_str)?;
        let octave = unpitched.child_i64("display-octave", 4) as i32;
        Pitch::new(step, octave)
    } else {
        return None;
    };

    // Accidental display
    let accidental = if let Some(acc_elem) = elem.find("accidental") {
        if acc_elem.attr("cautionary") == Some("yes") {
            AccidentalDisplay::Cautionary
        } else if acc_elem.attr("editorial") == Some("yes") {
            AccidentalDisplay::Editorial
        } else {
            AccidentalDisplay::Forced
        }
    } else {
        AccidentalDisplay::None
    };

    let mut note = Note::new(
        Pitch {
            accidental,
            ..pitch
        },
        duration,
    );
    note.voice = voice_num;
    note.staff = staff_num;
    note.is_grace = is_grace;
    note.grace_slash = elem.find("grace").and_then(|g| g.attr("slash")) == Some("yes");
    note.after_grace = elem
        .find("grace")
        .and_then(|g| g.attr("steal-time-previous"))
        .is_some();
    note.is_cue = elem.find("cue").is_some();

    // Stem direction
    if let Some(stem) = elem.child_text("stem") {
        note.stem_direction = stem.to_string();
    }

    // Notehead
    if let Some(nh) = elem.child_text("notehead") {
        note.notehead = nh.to_string();
    }

    // print-object attribute
    if elem.attr("print-object") == Some("no") {
        note.print_object = false;
    }

    // Notations
    if let Some(notations) = elem.find("notations") {
        parse_notations(notations, &mut note);
    }

    // Lyrics
    for lyric_elem in elem.find_all("lyric") {
        if let Some(syllable) = parse_lyric(lyric_elem) {
            note.lyrics.push(syllable);
        }
    }

    // Beams
    for beam_elem in elem.find_all("beam") {
        let number: u8 = beam_elem
            .attr("number")
            .and_then(|s| s.parse().ok())
            .unwrap_or(1);
        let beam_type = beam_elem.text_content().to_string();
        if !matches!(
            beam_type.as_str(),
            "begin" | "continue" | "end" | "forward hook" | "backward hook"
        ) {
            continue;
        }
        note.beams.push(BeamEvent { beam_type, number });
    }

    Some(NoteOrRest::Note(Box::new(note)))
}

fn parse_pitch(elem: &XmlNode) -> Option<Pitch> {
    let step_str = elem.child_text("step")?;
    let step = PitchStep::from_name(step_str)?;
    let octave = elem.child_i64("octave", 4) as i32;
    let alter = elem
        .find("alter")
        .map(|a| {
            let text = a.text_content();
            // Parse as float first to handle "0.5", "-0.5", etc., then convert to Ratio.
            if let Ok(f) = text.parse::<f64>() {
                // Convert to Ratio: multiply by 2 to get integer half-semitones.
                let half_semitones = (f * 2.0).round() as i32;
                Alter::new(half_semitones, 2)
            } else {
                Alter::from_integer(0)
            }
        })
        .unwrap_or_else(|| Alter::from_integer(0));

    Some(Pitch::with_alter(step, alter, octave))
}

fn parse_notations(notations: &XmlNode, note: &mut Note) {
    // Ties
    for tied in notations.find_all("tied") {
        let tie_type = tied.attr("type").unwrap_or("");
        let event = match tie_type {
            "start" => TieEvent {
                tie_type: StartStop::Start,
            },
            "stop" => TieEvent {
                tie_type: StartStop::Stop,
            },
            _ => continue,
        };
        note.ties.push(event);
    }

    // Slurs
    for slur in notations.find_all("slur") {
        let slur_type = slur.attr("type").unwrap_or("");
        let number: u8 = slur
            .attr("number")
            .and_then(|s| s.parse().ok())
            .unwrap_or(1);
        let placement = slur
            .attr("placement")
            .map(|p| match p {
                "above" => Placement::Above,
                "below" => Placement::Below,
                _ => Placement::Unspecified,
            })
            .unwrap_or(Placement::Unspecified);
        let event = match slur_type {
            "start" => SlurEvent {
                slur_type: StartStop::Start,
                number,
                placement,
            },
            "stop" => SlurEvent {
                slur_type: StartStop::Stop,
                number,
                placement: Placement::Unspecified,
            },
            _ => continue,
        };
        note.slurs.push(event);
    }

    // Articulations
    if let Some(arts) = notations.find("articulations") {
        for child in &arts.children {
            let name = match child.tag.as_str() {
                "staccato" | "staccatissimo" | "accent" | "strong-accent" | "marcato"
                | "tenuto" | "detached-legato" | "stress" | "spiccato" | "breath-mark"
                | "caesura" | "portato" => child.tag.as_str(),
                _ => continue,
            };
            let placement = child
                .attr("placement")
                .map(|p| match p {
                    "above" => Placement::Above,
                    "below" => Placement::Below,
                    _ => Placement::Unspecified,
                })
                .unwrap_or(Placement::Unspecified);
            note.articulations.push(Articulation {
                name: name.to_string(),
                placement,
            });
        }
    }

    // Ornaments
    if let Some(orns) = notations.find("ornaments") {
        for child in &orns.children {
            match child.tag.as_str() {
                "tremolo" => {
                    let marks: u8 = child.text_content().parse().unwrap_or(0);
                    note.tremolo_marks = marks;
                    let ttype = child.attr("type").unwrap_or("single");
                    note.two_note_tremolo = ttype == "start" || ttype == "stop";
                    note.tremolo_start = ttype == "start";
                    // Also keep as ornament for round-trip
                    let placement = child
                        .attr("placement")
                        .map(|p| match p {
                            "above" => Placement::Above,
                            "below" => Placement::Below,
                            _ => Placement::Unspecified,
                        })
                        .unwrap_or(Placement::Unspecified);
                    note.ornaments.push(Ornament {
                        name: "tremolo".to_string(),
                        placement,
                    });
                }
                "wavy-line" => {
                    let placement = child
                        .attr("placement")
                        .map(|p| match p {
                            "above" => Placement::Above,
                            "below" => Placement::Below,
                            _ => Placement::Unspecified,
                        })
                        .unwrap_or(Placement::Unspecified);
                    let wl_type = child.attr("type").unwrap_or("start");
                    note.ornaments.push(Ornament {
                        name: format!("wavy-line-{}", wl_type),
                        placement,
                    });
                }
                "trill-mark" | "mordent" | "inverted-mordent" | "turn" | "inverted-turn" => {
                    let placement = child
                        .attr("placement")
                        .map(|p| match p {
                            "above" => Placement::Above,
                            "below" => Placement::Below,
                            _ => Placement::Unspecified,
                        })
                        .unwrap_or(Placement::Unspecified);
                    note.ornaments.push(Ornament {
                        name: child.tag.to_string(),
                        placement,
                    });
                }
                _ => continue,
            }
        }
    }

    // Technicals
    if let Some(techs) = notations.find("technical") {
        for child in &techs.children {
            let (name, value) = match child.tag.as_str() {
                "up-bow" | "down-bow" | "harmonic" | "open-string" | "stopped"
                | "snap-pizzicato" => (child.tag.as_str(), ""),
                "fingering" | "fret" | "string" => (child.tag.as_str(), child.text_content()),
                _ => continue,
            };
            note.technicals.push(Technical {
                name: name.to_string(),
                value: value.to_string(),
            });
        }
    }

    // Dynamics (inside notations)
    if let Some(dyn_elem) = notations.find("dynamics") {
        let placement = dyn_elem
            .attr("placement")
            .map(|p| match p {
                "above" => Placement::Above,
                "below" => Placement::Below,
                _ => Placement::Unspecified,
            })
            .unwrap_or(Placement::Unspecified);
        for child in &dyn_elem.children {
            let sign = match child.tag.as_str() {
                "ppp" | "pp" | "p" | "mp" | "mf" | "f" | "ff" | "fff" | "sf" | "sfz" | "fp" => {
                    child.tag.as_str()
                }
                _ => continue,
            };
            note.dynamics.push(DynamicMark {
                sign: sign.to_string(),
                placement,
            });
        }
    }

    // Tuplet display
    if let Some(tuplet) = notations.find("tuplet") {
        let tuplet_type = match tuplet.attr("type").unwrap_or("start") {
            "start" => StartStop::Start,
            "stop" => StartStop::Stop,
            _ => StartStop::Start,
        };
        let bracket = tuplet.attr("bracket") == Some("yes");
        let show_number = tuplet.attr("show-number").unwrap_or("actual").to_string();
        note.tuplet = Some(TupletDisplay {
            tuplet_type,
            bracket,
            show_number,
        });
    }

    // Fermata
    note.fermata = parse_fermata(notations);

    // Glissando
    if let Some(gliss) = notations.find("glissando") {
        let gliss_type = match gliss.attr("type").unwrap_or("") {
            "start" => Some(StartStop::Start),
            "stop" => Some(StartStop::Stop),
            _ => None,
        };
        note.glissando = gliss_type;
        note.glissando_line_type = gliss.attr("line-type").map(|s| s.to_string());
    }

    // Slide (portamento)
    if let Some(slide) = notations.find("slide") {
        let slide_type = match slide.attr("type").unwrap_or("") {
            "start" => Some(StartStop::Start),
            "stop" => Some(StartStop::Stop),
            _ => None,
        };
        note.slide = slide_type;
    }
}

pub(super) fn parse_fermata(notations: &XmlNode) -> Option<Fermata> {
    let fermata_elem = notations.find("fermata")?;
    let shape = match fermata_elem.text_content() {
        "normal" | "" => "normal",
        "angled" => "angled",
        "square" => "square",
        other => other,
    };
    let inverted = fermata_elem.attr("type") == Some("inverted");
    Some(Fermata {
        shape: shape.to_string(),
        inverted,
    })
}

fn parse_lyric(elem: &XmlNode) -> Option<LyricSyllable> {
    let text = elem.child_text("text")?.to_string();
    let syllabic = elem.child_text("syllabic").unwrap_or("single");
    let syllabic_type = match syllabic {
        "single" => SyllabicType::Single,
        "begin" => SyllabicType::Begin,
        "middle" => SyllabicType::Middle,
        "end" => SyllabicType::End,
        _ => SyllabicType::Single,
    };
    let number: u8 = elem
        .attr("number")
        .and_then(|s| s.parse().ok())
        .unwrap_or(1);
    let extend = elem.find("extend").is_some();
    let elision = elem.find("elision").is_some();

    Some(LyricSyllable {
        text,
        syllabic: syllabic_type,
        number,
        extend,
        elision,
    })
}
