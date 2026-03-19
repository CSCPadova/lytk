//! Direction, dynamics, wedge, pedal, harmony, and figured bass parsing.

use crate::ir::articulation::DynamicMark;
use crate::ir::articulation::Wedge;
use crate::ir::direction::*;
use crate::ir::duration::Duration;
use crate::ir::harmony::{ChordDegree, ChordPitch, Figure, FiguredBass, Harmony};
use crate::ir::Placement;

use super::helpers::XmlNode;

// ---------------------------------------------------------------------------
// Harmony / chord symbol parsing
// ---------------------------------------------------------------------------

pub(super) fn parse_harmony_elem(elem: &XmlNode) -> Option<Harmony> {
    let root_elem = elem.find("root")?;
    let root_step = root_elem.child_text("root-step")?.to_string();
    let root_alter: f64 = root_elem
        .child_text("root-alter")
        .and_then(|s| s.parse().ok())
        .unwrap_or(0.0);

    let kind = elem.child_text("kind").unwrap_or("major").to_string();

    let bass = elem.find("bass").and_then(|b| {
        let step = b.child_text("bass-step")?.to_string();
        let alter: f64 = b
            .child_text("bass-alter")
            .and_then(|s| s.parse().ok())
            .unwrap_or(0.0);
        Some(ChordPitch { step, alter })
    });

    let degrees: Vec<ChordDegree> = elem
        .find_all("degree")
        .iter()
        .filter_map(|d| {
            let value: u8 = d.child_text("degree-value")?.parse().ok()?;
            let alter: f64 = d
                .child_text("degree-alter")
                .and_then(|s| s.parse().ok())
                .unwrap_or(0.0);
            let degree_type = d.child_text("degree-type").unwrap_or("add").to_string();
            Some(ChordDegree {
                value,
                alter,
                degree_type,
            })
        })
        .collect();

    let offset: i32 = elem
        .child_text("offset")
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);

    Some(Harmony {
        root: ChordPitch {
            step: root_step,
            alter: root_alter,
        },
        kind,
        bass,
        degrees,
        offset,
    })
}

// ---------------------------------------------------------------------------
// Figured bass parsing
// ---------------------------------------------------------------------------

pub(super) fn parse_figured_bass_elem(elem: &XmlNode, divisions: i64) -> FiguredBass {
    let figures: Vec<Figure> = elem
        .find_all("figure")
        .iter()
        .map(|f| {
            let number: Option<u8> = f.child_text("figure-number").and_then(|s| s.parse().ok());
            let prefix = f.child_text("prefix").map(|s| s.to_string());
            let suffix = f.child_text("suffix").map(|s| s.to_string());
            Figure {
                number,
                prefix,
                suffix,
            }
        })
        .collect();

    let dur_val = elem.child_i64("duration", 0);
    let duration = if dur_val > 0 {
        Duration::from_divisions(dur_val, divisions, 0)
    } else {
        Duration::default()
    };
    let parentheses = elem.attr("parentheses") == Some("yes");
    let offset: i32 = elem
        .child_text("offset")
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);

    FiguredBass {
        figures,
        duration,
        parentheses,
        offset,
    }
}

// ---------------------------------------------------------------------------
// Direction parsing
// ---------------------------------------------------------------------------

pub(super) fn parse_direction(elem: &XmlNode) -> Option<Direction> {
    let mut dir = Direction::default();
    let placement_str = elem.attr("placement");
    if let Some(p) = placement_str {
        dir.placement = match p {
            "above" => Placement::Above,
            "below" => Placement::Below,
            _ => Placement::Unspecified,
        };
    }

    for dir_type in elem.find_all("direction-type") {
        for child in &dir_type.children {
            match child.tag.as_str() {
                "dynamics" => {
                    // Take the first dynamic child element.
                    for dyn_child in &child.children {
                        let sign = match dyn_child.tag.as_str() {
                            "ppp" | "pp" | "p" | "mp" | "mf" | "f" | "ff" | "fff" | "sf"
                            | "sfz" | "fp" => dyn_child.tag.as_str(),
                            _ => continue,
                        };
                        dir.dynamic = Some(DynamicMark {
                            sign: sign.to_string(),
                            placement: dir.placement,
                        });
                        break;
                    }
                }
                "wedge" => {
                    let wedge_type = child.attr("type").unwrap_or("").to_string();
                    if !wedge_type.is_empty() {
                        dir.wedge = Some(Wedge {
                            wedge_type,
                            placement: dir.placement,
                        });
                    }
                }
                "words" => {
                    let text = child.text_content().to_string();
                    if !text.is_empty() {
                        dir.text = Some(TextDirection {
                            text,
                            placement: dir.placement,
                            font_style: child.attr("font-style").map(|s| s.to_string()),
                            font_weight: child.attr("font-weight").map(|s| s.to_string()),
                        });
                    }
                }
                "rehearsal" => {
                    let text = child.text_content().to_string();
                    dir.rehearsal = Some(RehearsalMark { text });
                }
                "metronome" => {
                    let beat_unit = child.child_text("beat-unit").map(|s| s.to_string());
                    let per_minute = child
                        .find("per-minute")
                        .and_then(|pm| pm.text_content().parse::<f64>().ok());
                    let dots = child.find_all("beat-unit-dot").len() as u8;
                    dir.tempo = Some(TempoDirection {
                        text: None,
                        beat_unit,
                        per_minute,
                        dots,
                        placement: dir.placement,
                    });
                }
                "octave-shift" => {
                    let shift_type = child.attr("type").unwrap_or("up").to_string();
                    let size = child
                        .attr("size")
                        .and_then(|s| s.parse::<i8>().ok())
                        .unwrap_or(8);
                    dir.octave_shift = Some(OctaveShift { shift_type, size });
                }
                "pedal" => {
                    let pedal_type = child.attr("type").unwrap_or("").to_string();
                    let line = child.attr("line") == Some("yes");
                    if !pedal_type.is_empty() {
                        dir.pedal = Some(PedalEvent { pedal_type, line });
                    }
                }
                "coda" => {
                    dir.coda = true;
                }
                "segno" => {
                    dir.segno = true;
                }
                _ => {}
            }
        }
    }

    // If we have both a tempo (from <metronome>) and a text direction (from
    // <words>), merge the text into the tempo so that LilyPond emits e.g.
    // \tempo "Allegro" 4 = 120  instead of a separate \markup.
    if dir.tempo.is_some() && dir.text.is_some() {
        if let (Some(tempo), Some(text_dir)) = (dir.tempo.as_mut(), dir.text.take()) {
            tempo.text = Some(text_dir.text);
        }
    }

    // Check <sound> element for dacapo/dalsegno and tempo attributes
    if let Some(sound) = elem.find("sound") {
        if let Some(dc) = sound.attr("dacapo") {
            if dc == "yes" {
                dir.da_capo = Some("D.C.".to_string());
            }
        }
        if let Some(ds) = sound.attr("dalsegno") {
            if !ds.is_empty() {
                dir.dal_segno = Some("D.S.".to_string());
            }
        }
        // <sound tempo="120"> as fallback BPM when no <metronome> was given,
        // or to fill in a missing per_minute value.
        if let Some(tempo_val) = sound.attr("tempo").and_then(|s| s.parse::<f64>().ok()) {
            if let Some(ref mut tempo) = dir.tempo {
                // Fill in BPM if the metronome element didn't have one
                if tempo.per_minute.is_none() {
                    tempo.per_minute = Some(tempo_val);
                }
            } else {
                // No <metronome> at all — create a tempo from <sound tempo>
                // and absorb any <words> text as the tempo label.
                let text = dir.text.take().map(|t| t.text);
                dir.tempo = Some(TempoDirection {
                    text,
                    beat_unit: Some("quarter".to_string()),
                    per_minute: Some(tempo_val),
                    dots: 0,
                    placement: dir.placement,
                });
            }
        }
    }

    // Check if any content was parsed.
    if dir.dynamic.is_none()
        && dir.wedge.is_none()
        && dir.text.is_none()
        && dir.rehearsal.is_none()
        && dir.tempo.is_none()
        && dir.octave_shift.is_none()
        && dir.pedal.is_none()
        && !dir.coda
        && !dir.segno
        && dir.da_capo.is_none()
        && dir.dal_segno.is_none()
    {
        return None;
    }

    Some(dir)
}
