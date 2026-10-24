//! Note, chord, rest, pitch, notations, and lyric conversion from musicxml types.

use crate::ir::articulation::*;
use crate::ir::duration::Duration;
use crate::ir::note::*;
use crate::ir::pitch::{AccidentalDisplay, Alter, Pitch, PitchStep};

use crate::ir::duration::NoteType;
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
    let note_type = mxml_note
        .content
        .r#type
        .as_ref()
        .map(|t| note_type_value(&t.content));

    let duration = if is_grace {
        Duration::dotted(note_type.unwrap_or(NoteType::Eighth).length(), dots)
    } else {
        let dur_val: i64 = match &mxml_note.content.info {
            mxml::NoteType::Normal(info) => info.duration.content.0 as i64,
            mxml::NoteType::Cue(info) => info.duration.content.0 as i64,
            mxml::NoteType::Grace(_) => 0,
        };
        if let Some(t) = note_type {
            Duration::dotted(t.length(), dots)
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
        note.stem_direction = Some(stem_direction(&stem.content));
    }

    // Notehead
    if let Some(ref nh) = mxml_note.content.notehead {
        note.notehead = Some(notehead(&nh.content));
    }

    // print-object attribute
    if mxml_note.attributes.print_object == Some(mdt::YesNo::No) {
        note.print_object = false;
    }

    // `dynamics="…"`: the note's MIDI velocity as a percentage of 90.
    if let Some(d) = &mxml_note.attributes.dynamics {
        note.velocity = Some((d.0 * 0.9).round().clamp(1.0, 127.0) as u8);
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
        note.beams.push(BeamEvent {
            beam_type: beam_value(&beam_elem.content),
            number,
        });
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

fn extract_rest_info(note: &mxml::Note) -> (Option<PitchStep>, Option<i32>, bool) {
    if let mxml::AudibleType::Rest(rest) = note_audible(note) {
        let display_step = rest
            .content
            .display_step
            .as_ref()
            .map(|s| convert_step(&s.content));
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
                        note.articulations.push(Articulation { name, placement });
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
                        note.technicals.push(Technical { name, value });
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
                        note.dynamics.push(DynamicMark { sign, placement });
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
                note.glissando_line_type = gliss.attributes.line_type.as_ref().map(line_type);
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
    Fermata {
        shape: fermata_shape(&fermata.content),
        inverted: fermata.attributes.r#type == Some(mdt::UprightInverted::Inverted),
    }
}

fn convert_tuplet_display(tuplet: &mxml::Tuplet) -> TupletDisplay {
    let tuplet_type = match tuplet.attributes.r#type {
        mdt::StartStop::Start => StartStop::Start,
        mdt::StartStop::Stop => StartStop::Stop,
    };
    let bracket = tuplet.attributes.bracket == Some(mdt::YesNo::Yes);
    // MusicXML's default: the actual number.
    let show_number = Some(match tuplet.attributes.show_number.as_ref() {
        Some(mdt::ShowTuplet::Both) => ShowNumber::Both,
        Some(mdt::ShowTuplet::None) => ShowNumber::NoNumber,
        _ => ShowNumber::Actual,
    });
    TupletDisplay {
        tuplet_type,
        bracket,
        show_number,
    }
}

/// A `<lyric>`: its texts joined with `‿` when elided (`my‿a` on one note),
/// the syllabic from the first's start and the last's end. A `number` that
/// is no number (`chorus`) is the verse's name, numbered later per part
/// (`number_verses`); `number` 0 until then.
fn parse_lyric(lyric: &mxml::Lyric) -> Option<LyricSyllable> {
    let mxml::LyricContents::Text(text_lyric) = &lyric.content else {
        // An extend-only lyric goes on an extender a syllable already has;
        // humming and laughing have no text.
        return None;
    };
    let syllabic = |s: &Option<mxml::Syllabic>| match s.as_ref().map(|s| &s.content) {
        Some(mdt::Syllabic::Begin) => SyllabicType::Begin,
        Some(mdt::Syllabic::Middle) => SyllabicType::Middle,
        Some(mdt::Syllabic::End) => SyllabicType::End,
        _ => SyllabicType::Single,
    };
    let mut texts = vec![text_lyric.text.content.clone()];
    texts.extend(text_lyric.additional.iter().map(|a| a.text.content.clone()));
    texts.retain(|t| !t.is_empty());
    if texts.is_empty() {
        return None;
    }
    let first = syllabic(&text_lyric.syllabic);
    let last = text_lyric
        .additional
        .last()
        .map_or(first, |a| syllabic(&a.syllabic));
    let starts_word = matches!(first, SyllabicType::Single | SyllabicType::Begin);
    let ends_word = matches!(last, SyllabicType::Single | SyllabicType::End);
    let syllabic = match (starts_word, ends_word) {
        (true, true) => SyllabicType::Single,
        (true, false) => SyllabicType::Begin,
        (false, true) => SyllabicType::End,
        (false, false) => SyllabicType::Middle,
    };
    // `<extend type="stop"/>` ends an extender; it starts none.
    let extend = text_lyric
        .extend
        .as_ref()
        .is_some_and(|e| e.attributes.r#type != Some(mdt::StartStopContinue::Stop));
    let token = lyric.attributes.number.as_ref().map(|n| n.0.trim());
    let (number, named) = match token.map(|t| t.parse::<u8>()) {
        None => (1, None),
        Some(Ok(n)) if n > 0 => (n, None),
        _ => (0, token.map(str::to_string)),
    };
    let name = lyric
        .attributes
        .name
        .as_ref()
        .map(|n| n.0.clone())
        .or(named);
    Some(LyricSyllable {
        elision: texts.len() > 1,
        text: texts.join("\u{203F}"),
        syllabic,
        number,
        extend,
        name,
    })
}

/// One lyric line per (number, name, repetition on its note): MusicXML says
/// nothing of how `number` and `name` combine (suite 61g), and two
/// syllables of one line on one note can't be written. A line keeps its
/// number when it is the first with it (a `number` that is no number reads
/// as 0); the others take the first numbers left.
pub(super) fn number_verses(part: &mut crate::ir::Part) {
    type Line = (u8, Option<String>, usize);
    fn lines_of(lyrics: &[LyricSyllable]) -> Vec<Line> {
        lyrics
            .iter()
            .enumerate()
            .map(|(i, l)| {
                let before = lyrics[..i]
                    .iter()
                    .filter(|o| o.number == l.number && o.name == l.name)
                    .count();
                (l.number, l.name.clone(), before)
            })
            .collect()
    }
    let mut lines: Vec<Line> = Vec::new();
    for n in part
        .measures
        .iter()
        .flat_map(|m| &m.voices)
        .flat_map(|v| &v.elements)
        .flat_map(|e| e.notes())
    {
        for line in lines_of(&n.lyrics) {
            if !lines.contains(&line) {
                lines.push(line);
            }
        }
    }
    let mut used = std::collections::BTreeSet::new();
    let mut number: std::collections::HashMap<&Line, u8> = Default::default();
    for line in &lines {
        if line.0 > 0 && used.insert(line.0) {
            number.insert(line, line.0);
        }
    }
    for line in &lines {
        if !number.contains_key(line) {
            let n = (1..=u8::MAX).find(|n| !used.contains(n)).unwrap_or(u8::MAX);
            used.insert(n);
            number.insert(line, n);
        }
    }
    if number.iter().all(|(line, n)| line.0 == *n) {
        return;
    }
    for e in part
        .measures
        .iter_mut()
        .flat_map(|m| &mut m.voices)
        .flat_map(|v| &mut v.elements)
    {
        for n in e.notes_mut() {
            let renumbered: Vec<u8> = lines_of(&n.lyrics).iter().map(|l| number[l]).collect();
            for (l, v) in n.lyrics.iter_mut().zip(renumbered) {
                l.number = v;
            }
        }
    }
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

fn convert_articulation(art: &mxml::ArticulationsType) -> (Option<ArticulationType>, Placement) {
    use mxml::ArticulationsType::*;
    // Every arm reports its name plus the (uniformly typed) placement attribute.
    match art {
        Accent(a) => (
            Some(ArticulationType::Accent),
            placement_or_unspecified(&a.attributes.placement),
        ),
        StrongAccent(a) => (
            Some(ArticulationType::StrongAccent),
            placement_or_unspecified(&a.attributes.placement),
        ),
        Staccato(a) => (
            Some(ArticulationType::Staccato),
            placement_or_unspecified(&a.attributes.placement),
        ),
        Tenuto(a) => (
            Some(ArticulationType::Tenuto),
            placement_or_unspecified(&a.attributes.placement),
        ),
        DetachedLegato(a) => (
            Some(ArticulationType::DetachedLegato),
            placement_or_unspecified(&a.attributes.placement),
        ),
        Staccatissimo(a) => (
            Some(ArticulationType::Staccatissimo),
            placement_or_unspecified(&a.attributes.placement),
        ),
        Spiccato(a) => (
            Some(ArticulationType::Spiccato),
            placement_or_unspecified(&a.attributes.placement),
        ),
        BreathMark(a) => (
            Some(ArticulationType::BreathMark),
            placement_or_unspecified(&a.attributes.placement),
        ),
        Caesura(a) => (
            Some(ArticulationType::Caesura),
            placement_or_unspecified(&a.attributes.placement),
        ),
        Stress(a) => (
            Some(ArticulationType::Stress),
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
                name: OrnamentType::Tremolo,
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
            let name = match wl.attributes.r#type {
                mdt::StartStopContinue::Start => OrnamentType::WavyLineStart,
                mdt::StartStopContinue::Stop => OrnamentType::WavyLineStop,
                mdt::StartStopContinue::Continue => OrnamentType::WavyLineContinue,
            };
            note.ornaments.push(Ornament { name, placement });
        }
        TrillMark(t) => {
            let placement = t
                .attributes
                .placement
                .as_ref()
                .map(convert_above_below)
                .unwrap_or(Placement::Unspecified);
            note.ornaments.push(Ornament {
                name: OrnamentType::TrillMark,
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
                name: OrnamentType::Mordent,
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
                name: OrnamentType::InvertedMordent,
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
                name: OrnamentType::Turn,
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
                name: OrnamentType::InvertedTurn,
                placement,
            });
        }
        _ => {}
    }
}

fn convert_technical(tech: &mxml::TechnicalContents) -> Option<(TechnicalType, String)> {
    use mxml::TechnicalContents::*;
    match tech {
        UpBow(_) => Some((TechnicalType::UpBow, String::new())),
        DownBow(_) => Some((TechnicalType::DownBow, String::new())),
        Harmonic(_) => Some((TechnicalType::Harmonic, String::new())),
        OpenString(_) => Some((TechnicalType::OpenString, String::new())),
        Stopped(_) => Some((TechnicalType::Stopped, String::new())),
        SnapPizzicato(_) => Some((TechnicalType::SnapPizzicato, String::new())),
        Fingering(f) => Some((TechnicalType::Fingering, f.content.clone())),
        Fret(f) => Some((TechnicalType::Fret, f.content.0.to_string())),
        StringNumber(s) => Some((TechnicalType::String, s.content.0.to_string())),
        _ => None,
    }
}

fn convert_dynamic_type(dyn_content: &mxml::DynamicsType) -> Option<DynamicType> {
    use mxml::DynamicsType::*;
    match dyn_content {
        Ppp(_) => Some(DynamicType::Ppp),
        Pp(_) => Some(DynamicType::Pp),
        P(_) => Some(DynamicType::P),
        Mp(_) => Some(DynamicType::Mp),
        Mf(_) => Some(DynamicType::Mf),
        F(_) => Some(DynamicType::F),
        Ff(_) => Some(DynamicType::Ff),
        Fff(_) => Some(DynamicType::Fff),
        Sf(_) => Some(DynamicType::Sf),
        Sfz(_) => Some(DynamicType::Sfz),
        Fp(_) => Some(DynamicType::Fp),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Type conversion helpers
// ---------------------------------------------------------------------------

pub(super) fn convert_step(step: &mdt::Step) -> PitchStep {
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

pub(super) fn note_type_value(ntv: &mdt::NoteTypeValue) -> NoteType {
    match ntv {
        mdt::NoteTypeValue::Maxima => NoteType::Maxima,
        mdt::NoteTypeValue::Long => NoteType::Long,
        mdt::NoteTypeValue::Breve => NoteType::Breve,
        mdt::NoteTypeValue::Whole => NoteType::Whole,
        mdt::NoteTypeValue::Half => NoteType::Half,
        mdt::NoteTypeValue::Quarter => NoteType::Quarter,
        mdt::NoteTypeValue::Eighth => NoteType::Eighth,
        mdt::NoteTypeValue::Sixteenth => NoteType::Sixteenth,
        mdt::NoteTypeValue::ThirtySecond => NoteType::ThirtySecond,
        mdt::NoteTypeValue::SixtyFourth => NoteType::SixtyFourth,
        mdt::NoteTypeValue::OneHundredTwentyEighth => NoteType::OneHundredTwentyEighth,
        mdt::NoteTypeValue::TwoHundredFiftySixth => NoteType::TwoHundredFiftySixth,
        mdt::NoteTypeValue::FiveHundredTwelfth => NoteType::FiveHundredTwelfth,
        mdt::NoteTypeValue::OneThousandTwentyFourth => NoteType::OneThousandTwentyFourth,
    }
}

fn stem_direction(sv: &mdt::StemValue) -> StemDirection {
    match sv {
        mdt::StemValue::Up => StemDirection::Up,
        mdt::StemValue::Down => StemDirection::Down,
        mdt::StemValue::Double => StemDirection::Double,
        mdt::StemValue::None => StemDirection::NoStem,
    }
}

fn notehead(nh: &mdt::NoteheadValue) -> Notehead {
    use mdt::NoteheadValue::*;
    match nh {
        Slash => crate::ir::note::Notehead::Slash,
        Triangle => crate::ir::note::Notehead::Triangle,
        Diamond => crate::ir::note::Notehead::Diamond,
        Square => crate::ir::note::Notehead::Square,
        Cross => crate::ir::note::Notehead::Cross,
        X => crate::ir::note::Notehead::X,
        CircleX => crate::ir::note::Notehead::CircleX,
        InvertedTriangle => crate::ir::note::Notehead::InvertedTriangle,
        ArrowDown => crate::ir::note::Notehead::ArrowDown,
        ArrowUp => crate::ir::note::Notehead::ArrowUp,
        Circled => crate::ir::note::Notehead::Circled,
        Slashed => crate::ir::note::Notehead::Slashed,
        BackSlashed => crate::ir::note::Notehead::BackSlashed,
        Normal => crate::ir::note::Notehead::Normal,
        Cluster => crate::ir::note::Notehead::Cluster,
        CircleDot => crate::ir::note::Notehead::CircleDot,
        LeftTriangle => crate::ir::note::Notehead::LeftTriangle,
        Rectangle => crate::ir::note::Notehead::Rectangle,
        None => crate::ir::note::Notehead::NoHead,
        Do => crate::ir::note::Notehead::Do,
        Re => crate::ir::note::Notehead::Re,
        Mi => crate::ir::note::Notehead::Mi,
        Fa => crate::ir::note::Notehead::Fa,
        FaUp => crate::ir::note::Notehead::FaUp,
        So => crate::ir::note::Notehead::So,
        La => crate::ir::note::Notehead::La,
        Ti => crate::ir::note::Notehead::Ti,
        Other => crate::ir::note::Notehead::Other,
    }
}

fn beam_value(bv: &mdt::BeamValue) -> BeamValue {
    match bv {
        mdt::BeamValue::Begin => BeamValue::Begin,
        mdt::BeamValue::Continue => BeamValue::Continue,
        mdt::BeamValue::End => BeamValue::End,
        mdt::BeamValue::ForwardHook => BeamValue::ForwardHook,
        mdt::BeamValue::BackwardHook => BeamValue::BackwardHook,
    }
}

fn fermata_shape(shape: &mdt::FermataShape) -> FermataShape {
    match shape {
        mdt::FermataShape::Normal => FermataShape::Normal,
        mdt::FermataShape::Angled => FermataShape::Angled,
        mdt::FermataShape::Square => FermataShape::Square,
        mdt::FermataShape::DoubleAngled => FermataShape::DoubleAngled,
        mdt::FermataShape::DoubleSquare => FermataShape::DoubleSquare,
        mdt::FermataShape::DoubleDot => FermataShape::DoubleDot,
        mdt::FermataShape::HalfCurve => FermataShape::HalfCurve,
        mdt::FermataShape::Curlew => FermataShape::Curlew,
        mdt::FermataShape::Empty => FermataShape::Normal,
    }
}

fn line_type(lt: &mdt::LineType) -> LineType {
    match lt {
        mdt::LineType::Solid => LineType::Solid,
        mdt::LineType::Dashed => LineType::Dashed,
        mdt::LineType::Dotted => LineType::Dotted,
        mdt::LineType::Wavy => LineType::Wavy,
    }
}

pub(super) fn convert_above_below(ab: &mdt::AboveBelow) -> Placement {
    match ab {
        mdt::AboveBelow::Above => Placement::Above,
        mdt::AboveBelow::Below => Placement::Below,
    }
}
