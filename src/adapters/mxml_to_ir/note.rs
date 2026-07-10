//! Note, chord, rest, pitch, notations, and lyric conversion from musicxml types.

use crate::ir::articulation::*;
use crate::ir::duration::Duration;
use crate::ir::note::*;
use crate::ir::pitch::{AccidentalDisplay, Alter, Pitch, PitchStep};

use musicxml::datatypes as mdt;
use musicxml::elements as mxml;

// ---------------------------------------------------------------------------
// Note conversion
// ---------------------------------------------------------------------------

pub(super) enum NoteOrRest {
    Note(Box<Note>),
    Rest(Rest),
}

pub(super) fn convert_note(mxml_note: &mxml::Note, divisions: i64) -> Option<NoteOrRest> {
    let is_rest = note_is_rest(mxml_note);
    let is_grace = matches!(&mxml_note.content.info, mxml::NoteType::Grace(_));
    let voice_num: u8 = mxml_note
        .content
        .voice
        .as_ref()
        .and_then(|v| v.content.parse().ok())
        .unwrap_or(1);
    let staff_num: u8 = mxml_note
        .content
        .staff
        .as_ref()
        .map(|s| s.content.0 as u8)
        .unwrap_or(1);

    // Duration. Cap the dot count so a crafted note with hundreds of <dot/>
    // elements can't truncate-wrap the u8 (dot_multiplier clamps it anyway).
    let dots = mxml_note
        .content
        .dot
        .len()
        .min(crate::ir::duration::MAX_DOTS as usize) as u8;
    let type_name = mxml_note
        .content
        .r#type
        .as_ref()
        .map(|t| note_type_value_to_str(&t.content));

    let duration = if is_grace {
        let tn = type_name.unwrap_or("eighth");
        Duration::from_musicxml_type(tn, dots).unwrap_or_default()
    } else {
        let dur_val: i64 = match &mxml_note.content.info {
            mxml::NoteType::Normal(info) => info.duration.content.0 as i64,
            mxml::NoteType::Cue(info) => info.duration.content.0 as i64,
            mxml::NoteType::Grace(_) => 0,
        };
        if let Some(tn) = type_name {
            Duration::from_musicxml_type(tn, dots)
                .unwrap_or_else(|| Duration::from_divisions(dur_val, divisions, dots))
        } else {
            Duration::from_divisions(dur_val, divisions, dots)
        }
    };

    // Tuplet scaling. Ignore ratios outside 1..=255 rather than truncating
    // through u8: actual-notes=0 (or 256, which wraps to 0) would panic in
    // Frac::new (zero denominator) via Duration::actual_duration.
    let duration = if let Some(ref time_mod) = mxml_note.content.time_modification {
        let actual = time_mod.content.actual_notes.content.0;
        let normal = time_mod.content.normal_notes.content.0;
        if (1..=u8::MAX as u32).contains(&actual) && (1..=u8::MAX as u32).contains(&normal) {
            Duration {
                tuplet_normal: normal as u8,
                tuplet_actual: actual as u8,
                ..duration
            }
        } else {
            duration
        }
    } else {
        duration
    };

    if is_rest {
        let (display_step, display_octave, is_measure_rest) = extract_rest_info(mxml_note);

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
            dynamics: Vec::new(),
            wedges: Vec::new(),
        };

        // Check notations for fermata and tuplet display
        for notations in &mxml_note.content.notations {
            rest.fermata = rest
                .fermata
                .or_else(|| parse_fermata_from_notations(notations));
            if rest.tuplet.is_none() {
                for notation in &notations.content.notations {
                    if let mxml::NotationContentTypes::Tuplet(tuplet) = notation {
                        rest.tuplet = Some(convert_tuplet_display(tuplet));
                    }
                }
            }
        }

        return Some(NoteOrRest::Rest(rest));
    }

    // Pitched note
    let pitch = extract_pitch(mxml_note)?;

    // Accidental display
    let accidental = if let Some(ref acc_elem) = mxml_note.content.accidental {
        if acc_elem.attributes.cautionary == Some(mdt::YesNo::Yes) {
            AccidentalDisplay::Cautionary
        } else if acc_elem.attributes.editorial == Some(mdt::YesNo::Yes) {
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

    // Grace note details
    if let mxml::NoteType::Grace(ref grace_info) = mxml_note.content.info {
        note.grace_slash = grace_info.grace.attributes.slash == Some(mdt::YesNo::Yes);
        note.after_grace = grace_info.grace.attributes.steal_time_previous.is_some();
    }

    // Cue note
    note.is_cue = matches!(&mxml_note.content.info, mxml::NoteType::Cue(_));

    // Stem direction
    if let Some(ref stem) = mxml_note.content.stem {
        note.stem_direction = stem_value_to_str(&stem.content);
    }

    // Notehead
    if let Some(ref nh) = mxml_note.content.notehead {
        note.notehead = notehead_value_to_str(&nh.content);
    }

    // print-object attribute
    if mxml_note.attributes.print_object == Some(mdt::YesNo::No) {
        note.print_object = false;
    }

    // Notations
    for notations in &mxml_note.content.notations {
        parse_notations(notations, &mut note);
    }

    // Lyrics
    for lyric_elem in &mxml_note.content.lyric {
        if let Some(syllable) = parse_lyric(lyric_elem) {
            note.lyrics.push(syllable);
        }
    }

    // Beams
    for beam_elem in &mxml_note.content.beam {
        let number: u8 = beam_elem
            .attributes
            .number
            .as_ref()
            .map(|n| n.0)
            .unwrap_or(1);
        let beam_type = beam_value_to_str(&beam_elem.content);
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

// ---------------------------------------------------------------------------
// Pitch extraction
// ---------------------------------------------------------------------------

/// Reach the `AudibleType` (pitch / unpitched / rest) inside any note variant.
fn note_audible(note: &mxml::Note) -> &mxml::AudibleType {
    match &note.content.info {
        mxml::NoteType::Normal(info) => &info.audible,
        mxml::NoteType::Grace(info) => match &info.info {
            mxml::GraceType::Normal(n) => &n.audible,
            mxml::GraceType::Cue(c) => &c.audible,
        },
        mxml::NoteType::Cue(info) => &info.audible,
    }
}

fn extract_pitch(mxml_note: &mxml::Note) -> Option<Pitch> {
    match note_audible(mxml_note) {
        mxml::AudibleType::Pitch(p) => {
            let step = convert_step(&p.content.step.content);
            let octave = p.content.octave.content.0 as i32;
            let alter = p
                .content
                .alter
                .as_ref()
                .map(|a| {
                    let semitones = a.content.0 as i32;
                    // Encoded fractional alter (microtone) — see
                    // `adapters::encode_fractional_alters`.
                    if (crate::adapters::ALTER_ENC_MIN..=crate::adapters::ALTER_ENC_MAX)
                        .contains(&semitones)
                    {
                        Alter::new(semitones - crate::adapters::ALTER_ENC_BASE, 100)
                    } else {
                        Alter::from_integer(semitones)
                    }
                })
                .unwrap_or_else(|| Alter::from_integer(0));
            Some(Pitch::with_alter(step, alter, octave))
        }
        mxml::AudibleType::Unpitched(u) => {
            let step = convert_step(&u.content.display_step.content);
            let octave = u.content.display_octave.content.0 as i32;
            Some(Pitch::new(step, octave))
        }
        mxml::AudibleType::Rest(_) => None,
    }
}

fn note_is_rest(note: &mxml::Note) -> bool {
    matches!(note_audible(note), mxml::AudibleType::Rest(_))
}

fn extract_rest_info(note: &mxml::Note) -> (Option<String>, Option<i32>, bool) {
    if let mxml::AudibleType::Rest(rest) = note_audible(note) {
        let display_step = rest
            .content
            .display_step
            .as_ref()
            .map(|s| step_to_str(&s.content));
        let display_octave = rest
            .content
            .display_octave
            .as_ref()
            .map(|o| o.content.0 as i32);
        let is_measure_rest = rest.attributes.measure == Some(mdt::YesNo::Yes);
        (display_step, display_octave, is_measure_rest)
    } else {
        (None, None, false)
    }
}

// ---------------------------------------------------------------------------
// Notations
// ---------------------------------------------------------------------------

fn parse_notations(notations: &mxml::Notations, note: &mut Note) {
    for notation in &notations.content.notations {
        match notation {
            mxml::NotationContentTypes::Tied(tied) => {
                let event = match tied.attributes.r#type {
                    mdt::StartStopContinue::Start => TieEvent {
                        tie_type: StartStop::Start,
                    },
                    mdt::StartStopContinue::Stop => TieEvent {
                        tie_type: StartStop::Stop,
                    },
                    _ => continue,
                };
                note.ties.push(event);
            }
            mxml::NotationContentTypes::Slur(slur) => {
                let number: u8 = slur.attributes.number.as_ref().map(|n| n.0).unwrap_or(1);
                let placement = slur
                    .attributes
                    .placement
                    .as_ref()
                    .map(convert_above_below)
                    .unwrap_or(Placement::Unspecified);
                let event = match slur.attributes.r#type {
                    mdt::StartStopContinue::Start => SlurEvent {
                        slur_type: StartStop::Start,
                        number,
                        placement,
                    },
                    mdt::StartStopContinue::Stop => SlurEvent {
                        slur_type: StartStop::Stop,
                        number,
                        placement: Placement::Unspecified,
                    },
                    mdt::StartStopContinue::Continue => continue,
                };
                note.slurs.push(event);
            }
            mxml::NotationContentTypes::Articulations(arts) => {
                for art in &arts.content {
                    let (name, placement) = convert_articulation(art);
                    if let Some(name) = name {
                        note.articulations.push(Articulation {
                            name: name.to_string(),
                            placement,
                        });
                    }
                }
            }
            mxml::NotationContentTypes::Ornaments(orns) => {
                for orn in &orns.content.ornaments {
                    convert_ornament(orn, note);
                }
            }
            mxml::NotationContentTypes::Technical(techs) => {
                for tech in &techs.content {
                    if let Some((name, value)) = convert_technical(tech) {
                        note.technicals.push(Technical {
                            name: name.to_string(),
                            value: value.to_string(),
                        });
                    }
                }
            }
            mxml::NotationContentTypes::Dynamics(dyn_elem) => {
                let placement = dyn_elem
                    .attributes
                    .placement
                    .as_ref()
                    .map(convert_above_below)
                    .unwrap_or(Placement::Unspecified);
                for dyn_content in &dyn_elem.content {
                    if let Some(sign) = convert_dynamic_type(dyn_content) {
                        note.dynamics.push(DynamicMark {
                            sign: sign.to_string(),
                            placement,
                        });
                    }
                }
            }
            mxml::NotationContentTypes::Tuplet(tuplet) => {
                note.tuplet = Some(convert_tuplet_display(tuplet));
            }
            mxml::NotationContentTypes::Fermata(fermata) => {
                note.fermata = Some(convert_fermata(fermata));
            }
            mxml::NotationContentTypes::Glissando(gliss) => {
                note.glissando = match gliss.attributes.r#type {
                    mdt::StartStop::Start => Some(StartStop::Start),
                    mdt::StartStop::Stop => Some(StartStop::Stop),
                };
                note.glissando_line_type =
                    gliss.attributes.line_type.as_ref().map(line_type_to_str);
            }
            mxml::NotationContentTypes::Slide(slide) => {
                note.slide = match slide.attributes.r#type {
                    mdt::StartStop::Start => Some(StartStop::Start),
                    mdt::StartStop::Stop => Some(StartStop::Stop),
                };
            }
            _ => {}
        }
    }
}

fn parse_fermata_from_notations(notations: &mxml::Notations) -> Option<Fermata> {
    for notation in &notations.content.notations {
        if let mxml::NotationContentTypes::Fermata(f) = notation {
            return Some(convert_fermata(f));
        }
    }
    None
}

fn convert_fermata(fermata: &mxml::Fermata) -> Fermata {
    let shape = fermata_shape_to_str(&fermata.content);
    let inverted = fermata.attributes.r#type == Some(mdt::UprightInverted::Inverted);
    Fermata {
        shape: shape.to_string(),
        inverted,
    }
}

fn convert_tuplet_display(tuplet: &mxml::Tuplet) -> TupletDisplay {
    let tuplet_type = match tuplet.attributes.r#type {
        mdt::StartStop::Start => StartStop::Start,
        mdt::StartStop::Stop => StartStop::Stop,
    };
    let bracket = tuplet.attributes.bracket == Some(mdt::YesNo::Yes);
    let show_number = tuplet
        .attributes
        .show_number
        .as_ref()
        .map(|sn| {
            use mdt::ShowTuplet;
            match sn {
                ShowTuplet::Actual => "actual",
                ShowTuplet::Both => "both",
                ShowTuplet::None => "none",
            }
        })
        .unwrap_or("actual")
        .to_string();
    TupletDisplay {
        tuplet_type,
        bracket,
        show_number,
    }
}

fn parse_lyric(lyric: &mxml::Lyric) -> Option<LyricSyllable> {
    let mut text = String::new();
    let mut syllabic_type = SyllabicType::Single;
    let mut extend = false;
    let elision;

    match &lyric.content {
        mxml::LyricContents::Text(text_lyric) => {
            text = text_lyric.text.content.clone();
            if let Some(ref syl) = text_lyric.syllabic {
                syllabic_type = match syl.content {
                    mdt::Syllabic::Single => SyllabicType::Single,
                    mdt::Syllabic::Begin => SyllabicType::Begin,
                    mdt::Syllabic::Middle => SyllabicType::Middle,
                    mdt::Syllabic::End => SyllabicType::End,
                };
            }
            extend = text_lyric.extend.is_some();
            elision = !text_lyric.additional.is_empty();
        }
        mxml::LyricContents::Extend(_) => {
            extend = true;
            elision = false;
        }
        _ => {
            elision = false;
        }
    }

    if text.is_empty() {
        return None;
    }

    let number: u8 = lyric
        .attributes
        .number
        .as_ref()
        .and_then(|n| n.0.parse().ok())
        .unwrap_or(1);

    Some(LyricSyllable {
        text,
        syllabic: syllabic_type,
        number,
        extend,
        elision,
    })
}

// ---------------------------------------------------------------------------
// Articulation, ornament, and technical conversion
// ---------------------------------------------------------------------------

/// Resolve an optional MusicXML above/below placement, defaulting to unspecified.
fn placement_or_unspecified(p: &Option<mdt::AboveBelow>) -> Placement {
    p.as_ref()
        .map(convert_above_below)
        .unwrap_or(Placement::Unspecified)
}

fn convert_articulation(art: &mxml::ArticulationsType) -> (Option<&str>, Placement) {
    use mxml::ArticulationsType::*;
    // Every arm reports its name plus the (uniformly typed) placement attribute.
    match art {
        Accent(a) => (
            Some("accent"),
            placement_or_unspecified(&a.attributes.placement),
        ),
        StrongAccent(a) => (
            Some("strong-accent"),
            placement_or_unspecified(&a.attributes.placement),
        ),
        Staccato(a) => (
            Some("staccato"),
            placement_or_unspecified(&a.attributes.placement),
        ),
        Tenuto(a) => (
            Some("tenuto"),
            placement_or_unspecified(&a.attributes.placement),
        ),
        DetachedLegato(a) => (
            Some("detached-legato"),
            placement_or_unspecified(&a.attributes.placement),
        ),
        Staccatissimo(a) => (
            Some("staccatissimo"),
            placement_or_unspecified(&a.attributes.placement),
        ),
        Spiccato(a) => (
            Some("spiccato"),
            placement_or_unspecified(&a.attributes.placement),
        ),
        BreathMark(a) => (
            Some("breath-mark"),
            placement_or_unspecified(&a.attributes.placement),
        ),
        Caesura(a) => (
            Some("caesura"),
            placement_or_unspecified(&a.attributes.placement),
        ),
        Stress(a) => (
            Some("stress"),
            placement_or_unspecified(&a.attributes.placement),
        ),
        _ => (None, Placement::Unspecified),
    }
}

fn convert_ornament(orn: &mxml::OrnamentType, note: &mut Note) {
    use mxml::OrnamentType::*;
    match orn {
        Tremolo(t) => {
            let marks: u8 = t.content.0;
            note.tremolo_marks = marks;
            let ttype = &t.attributes.r#type;
            let is_two_note = matches!(
                ttype,
                Some(mdt::TremoloType::Start) | Some(mdt::TremoloType::Stop)
            );
            note.two_note_tremolo = is_two_note;
            note.tremolo_start = matches!(ttype, Some(mdt::TremoloType::Start));
            let placement = t
                .attributes
                .placement
                .as_ref()
                .map(convert_above_below)
                .unwrap_or(Placement::Unspecified);
            note.ornaments.push(Ornament {
                name: "tremolo".to_string(),
                placement,
            });
        }
        WavyLine(wl) => {
            let placement = wl
                .attributes
                .placement
                .as_ref()
                .map(convert_above_below)
                .unwrap_or(Placement::Unspecified);
            let wl_type = match wl.attributes.r#type {
                mdt::StartStopContinue::Start => "start",
                mdt::StartStopContinue::Stop => "stop",
                mdt::StartStopContinue::Continue => "continue",
            };
            note.ornaments.push(Ornament {
                name: format!("wavy-line-{}", wl_type),
                placement,
            });
        }
        TrillMark(t) => {
            let placement = t
                .attributes
                .placement
                .as_ref()
                .map(convert_above_below)
                .unwrap_or(Placement::Unspecified);
            note.ornaments.push(Ornament {
                name: "trill-mark".to_string(),
                placement,
            });
        }
        Mordent(m) => {
            let placement = m
                .attributes
                .placement
                .as_ref()
                .map(convert_above_below)
                .unwrap_or(Placement::Unspecified);
            note.ornaments.push(Ornament {
                name: "mordent".to_string(),
                placement,
            });
        }
        InvertedMordent(m) => {
            let placement = m
                .attributes
                .placement
                .as_ref()
                .map(convert_above_below)
                .unwrap_or(Placement::Unspecified);
            note.ornaments.push(Ornament {
                name: "inverted-mordent".to_string(),
                placement,
            });
        }
        Turn(t) => {
            let placement = t
                .attributes
                .placement
                .as_ref()
                .map(convert_above_below)
                .unwrap_or(Placement::Unspecified);
            note.ornaments.push(Ornament {
                name: "turn".to_string(),
                placement,
            });
        }
        InvertedTurn(t) => {
            let placement = t
                .attributes
                .placement
                .as_ref()
                .map(convert_above_below)
                .unwrap_or(Placement::Unspecified);
            note.ornaments.push(Ornament {
                name: "inverted-turn".to_string(),
                placement,
            });
        }
        _ => {}
    }
}

fn convert_technical(tech: &mxml::TechnicalContents) -> Option<(&str, String)> {
    use mxml::TechnicalContents::*;
    match tech {
        UpBow(_) => Some(("up-bow", String::new())),
        DownBow(_) => Some(("down-bow", String::new())),
        Harmonic(_) => Some(("harmonic", String::new())),
        OpenString(_) => Some(("open-string", String::new())),
        Stopped(_) => Some(("stopped", String::new())),
        SnapPizzicato(_) => Some(("snap-pizzicato", String::new())),
        Fingering(f) => Some(("fingering", f.content.clone())),
        Fret(f) => Some(("fret", f.content.0.to_string())),
        StringNumber(s) => Some(("string", s.content.0.to_string())),
        _ => None,
    }
}

fn convert_dynamic_type(dyn_content: &mxml::DynamicsType) -> Option<&str> {
    use mxml::DynamicsType::*;
    match dyn_content {
        Ppp(_) => Some("ppp"),
        Pp(_) => Some("pp"),
        P(_) => Some("p"),
        Mp(_) => Some("mp"),
        Mf(_) => Some("mf"),
        F(_) => Some("f"),
        Ff(_) => Some("ff"),
        Fff(_) => Some("fff"),
        Sf(_) => Some("sf"),
        Sfz(_) => Some("sfz"),
        Fp(_) => Some("fp"),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Type conversion helpers
// ---------------------------------------------------------------------------

fn convert_step(step: &mdt::Step) -> PitchStep {
    match step {
        mdt::Step::C => PitchStep::C,
        mdt::Step::D => PitchStep::D,
        mdt::Step::E => PitchStep::E,
        mdt::Step::F => PitchStep::F,
        mdt::Step::G => PitchStep::G,
        mdt::Step::A => PitchStep::A,
        mdt::Step::B => PitchStep::B,
    }
}

fn step_to_str(step: &mdt::Step) -> String {
    match step {
        mdt::Step::C => "C",
        mdt::Step::D => "D",
        mdt::Step::E => "E",
        mdt::Step::F => "F",
        mdt::Step::G => "G",
        mdt::Step::A => "A",
        mdt::Step::B => "B",
    }
    .to_string()
}

pub(super) fn note_type_value_to_str(ntv: &mdt::NoteTypeValue) -> &'static str {
    match ntv {
        mdt::NoteTypeValue::Maxima => "maxima",
        mdt::NoteTypeValue::Long => "long",
        mdt::NoteTypeValue::Breve => "breve",
        mdt::NoteTypeValue::Whole => "whole",
        mdt::NoteTypeValue::Half => "half",
        mdt::NoteTypeValue::Quarter => "quarter",
        mdt::NoteTypeValue::Eighth => "eighth",
        mdt::NoteTypeValue::Sixteenth => "16th",
        mdt::NoteTypeValue::ThirtySecond => "32nd",
        mdt::NoteTypeValue::SixtyFourth => "64th",
        mdt::NoteTypeValue::OneHundredTwentyEighth => "128th",
        mdt::NoteTypeValue::TwoHundredFiftySixth => "256th",
        mdt::NoteTypeValue::FiveHundredTwelfth => "512th",
        mdt::NoteTypeValue::OneThousandTwentyFourth => "1024th",
    }
}

fn stem_value_to_str(sv: &mdt::StemValue) -> String {
    match sv {
        mdt::StemValue::Up => "up",
        mdt::StemValue::Down => "down",
        mdt::StemValue::Double => "double",
        mdt::StemValue::None => "none",
    }
    .to_string()
}

fn notehead_value_to_str(nh: &mdt::NoteheadValue) -> String {
    use mdt::NoteheadValue::*;
    match nh {
        Slash => "slash",
        Triangle => "triangle",
        Diamond => "diamond",
        Square => "square",
        Cross => "cross",
        X => "x",
        CircleX => "circle-x",
        InvertedTriangle => "inverted triangle",
        ArrowDown => "arrow down",
        ArrowUp => "arrow up",
        Circled => "circled",
        Slashed => "slashed",
        BackSlashed => "back slashed",
        Normal => "normal",
        Cluster => "cluster",
        CircleDot => "circle dot",
        LeftTriangle => "left triangle",
        Rectangle => "rectangle",
        None => "none",
        Do => "do",
        Re => "re",
        Mi => "mi",
        Fa => "fa",
        FaUp => "fa up",
        So => "so",
        La => "la",
        Ti => "ti",
        Other => "other",
    }
    .to_string()
}

fn beam_value_to_str(bv: &mdt::BeamValue) -> String {
    match bv {
        mdt::BeamValue::Begin => "begin",
        mdt::BeamValue::Continue => "continue",
        mdt::BeamValue::End => "end",
        mdt::BeamValue::ForwardHook => "forward hook",
        mdt::BeamValue::BackwardHook => "backward hook",
    }
    .to_string()
}

fn fermata_shape_to_str(shape: &mdt::FermataShape) -> &str {
    match shape {
        mdt::FermataShape::Normal => "normal",
        mdt::FermataShape::Angled => "angled",
        mdt::FermataShape::Square => "square",
        mdt::FermataShape::DoubleAngled => "double-angled",
        mdt::FermataShape::DoubleSquare => "double-square",
        mdt::FermataShape::DoubleDot => "double-dot",
        mdt::FermataShape::HalfCurve => "half-curve",
        mdt::FermataShape::Curlew => "curlew",
        mdt::FermataShape::Empty => "normal",
    }
}

fn line_type_to_str(lt: &mdt::LineType) -> String {
    match lt {
        mdt::LineType::Solid => "solid",
        mdt::LineType::Dashed => "dashed",
        mdt::LineType::Dotted => "dotted",
        mdt::LineType::Wavy => "wavy",
    }
    .to_string()
}

pub(super) fn convert_above_below(ab: &mdt::AboveBelow) -> Placement {
    match ab {
        mdt::AboveBelow::Above => Placement::Above,
        mdt::AboveBelow::Below => Placement::Below,
    }
}
