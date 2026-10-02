//! Retrograde transform — reverse the order of voice elements within each voice.
//!
//! This reverses the temporal ordering of notes, rests, and chords in every
//! voice of every measure, producing a time-reversed version of the music.
//!
//! # Idempotency
//! Retrograde is self-inverse: `R(R(x)) == x`.

use crate::ir::annotation::Annotation;
use crate::ir::articulation::{StartStop, SyllabicType};
use crate::ir::duration::{Duration, Frac};
use crate::ir::music::{Music, MusicDocument};
use crate::ir::note::VoiceElement;
use crate::ir::score::Score;

use super::{MusicTransform, Transform};

/// Reverse the order of voice elements within every voice.
///
/// Voice elements are reversed, producing a time-reversed version.
pub struct Retrograde;

impl Retrograde {
    pub fn new() -> Self {
        Self
    }
}

impl Default for Retrograde {
    fn default() -> Self {
        Self::new()
    }
}

impl Transform for Retrograde {
    fn apply(&self, score: &Score) -> Score {
        let mut result = score.clone();

        for part in result.parts_mut() {
            // The key, meter, transposition and clefs in force in each bar
            // (clefs at its start and at its end): they go with its music.
            let forces = in_force(&part.measures);
            // Musical content travels backwards. Barlines, numbering and
            // divisions stay on their measure shells.
            let mut contents: Vec<_> = part
                .measures
                .iter_mut()
                .map(|m| {
                    (
                        std::mem::take(&mut m.voices),
                        std::mem::take(&mut m.directions),
                        std::mem::take(&mut m.harmonies),
                        std::mem::take(&mut m.figured_bass),
                    )
                })
                .collect();
            contents.reverse();
            for (measure, (voices, directions, harmonies, figured_bass)) in
                part.measures.iter_mut().zip(contents)
            {
                measure.voices = voices;
                measure.directions = directions;
                measure.harmonies = harmonies;
                measure.figured_bass = figured_bass;
            }

            for measure in &mut part.measures {
                for voice in &mut measure.voices {
                    voice.elements.reverse();
                    reanchor_graces(&mut voice.elements);
                    for elem in &mut voice.elements {
                        swap_pairings(elem);
                    }
                }
            }
            reattribute(&mut part.measures, forces);
        }

        result
    }
}

/// What is in force in a bar: key, meter, transposition, and each staff's
/// clef at the bar's start and at its end.
#[derive(Clone, Default, PartialEq)]
struct Force {
    key: Option<crate::ir::measure::KeySignature>,
    time: Option<crate::ir::measure::TimeSignature>,
    transpose: Option<crate::ir::measure::Transpose>,
    clefs_at_start: std::collections::BTreeMap<u8, crate::ir::measure::Clef>,
    clefs_at_end: std::collections::BTreeMap<u8, crate::ir::measure::Clef>,
}

fn in_force(measures: &[crate::ir::measure::Measure]) -> Vec<Force> {
    let mut now = Force::default();
    measures
        .iter()
        .map(|m| {
            if let Some(a) = &m.attributes {
                now.key = a.key.or(now.key);
                now.time = a.time.clone().or(now.time.take());
                now.transpose = a.transpose.or(now.transpose);
                now.clefs_at_end
                    .extend(a.clefs.iter().map(|(s, c)| (*s, *c)));
            }
            now.clefs_at_start = now.clefs_at_end.clone();
            let mut changes: Vec<_> = m
                .directions
                .iter()
                .filter_map(|d| Some((d.offset_frac, d.staff.max(1), d.clef?)))
                .collect();
            changes.sort_by_key(|c| c.0);
            now.clefs_at_end
                .extend(changes.into_iter().map(|(_, s, c)| (s, c)));
            now.clone()
        })
        .collect()
}

/// Give each reversed bar (holding the music of bar `n - 1 - i`) what was in
/// force there, written where it changes: a bar starts with the clef its
/// music ended with, and a clef change inside it swaps places with the
/// music around it.
fn reattribute(measures: &mut [crate::ir::measure::Measure], forces: Vec<Force>) {
    let n = measures.len();
    let mut before: Option<Force> = None;
    for (i, m) in measures.iter_mut().enumerate() {
        let f = &forces[n - 1 - i];
        let len = m.content_length();
        // Clef changes inside the bar: at the mirrored place, back to the
        // clef before them.
        let mut changes: Vec<(Frac, u8, crate::ir::measure::Clef)> = m
            .directions
            .iter()
            .filter_map(|d| Some((d.offset_frac, d.staff.max(1), d.clef?)))
            .collect();
        changes.sort_by_key(|c| c.0);
        let mut current = f.clefs_at_start.clone();
        let mut mirrored = Vec::new();
        for (at, staff, clef) in changes {
            // A staff with no clef yet is in the treble clef.
            let old = current.get(&staff).copied().unwrap_or_default();
            mirrored.push((len - at, staff, old));
            current.insert(staff, clef);
        }
        m.directions.retain(|d| d.clef.is_none());
        for (at, staff, clef) in mirrored {
            m.directions.push(crate::ir::direction::Direction {
                offset_frac: at,
                staff,
                clef: Some(clef),
                ..Default::default()
            });
        }
        let prev = before.as_ref();
        let key = f.key.filter(|k| prev.is_none_or(|p| p.key != Some(*k)));
        let time = f
            .time
            .clone()
            .filter(|t| prev.is_none_or(|p| p.time.as_ref() != Some(t)));
        let transpose = f
            .transpose
            .filter(|t| prev.is_none_or(|p| p.transpose != Some(*t)));
        // The bar starts with the clef its music ended with; the bar before
        // ended (reversed) with the clef its music started with.
        let clefs: std::collections::HashMap<u8, crate::ir::measure::Clef> = f
            .clefs_at_end
            .iter()
            .filter(|(s, c)| prev.is_none_or(|p| p.clefs_at_start.get(s) != Some(c)))
            .map(|(s, c)| (*s, *c))
            .collect();
        let changes = key.is_some() || time.is_some() || transpose.is_some() || !clefs.is_empty();
        if changes || m.attributes.is_some() {
            let attrs = m.attributes.get_or_insert_with(Default::default);
            (attrs.key, attrs.time, attrs.transpose, attrs.clefs) = (key, time, transpose, clefs);
        }
        before = Some(f.clone());
    }
}

/// After reversal a grace run trails its principal; put it back in front
/// (in original order). Graces at the very start (a trailing grace forward,
/// e.g. `\afterGrace`) are left alone.
fn reanchor_graces(elements: &mut Vec<VoiceElement>) {
    let is_grace = |e: &VoiceElement| matches!(e, VoiceElement::Note(n) if n.is_grace);
    let old = std::mem::take(elements);
    let mut out: Vec<VoiceElement> = Vec::with_capacity(old.len());
    let mut iter = old.into_iter().peekable();
    while let Some(e) = iter.next() {
        if is_grace(&e) {
            out.push(e); // leading grace: no principal before it
            continue;
        }
        let mut run: Vec<VoiceElement> = Vec::new();
        while iter.peek().is_some_and(&is_grace) {
            run.push(iter.next().unwrap());
        }
        if run.is_empty() {
            out.push(e);
        } else {
            run.reverse();
            out.extend(run);
            out.push(e);
        }
    }
    *elements = out;
}

/// Swap Start↔Stop on paired events (ties, slurs, tuplet brackets, beams)
/// so every pair still opens before it closes in the reversed timeline; a
/// beam's hooks turn round too.
fn swap_pairings(elem: &mut VoiceElement) {
    let flip = |t: &mut StartStop| {
        *t = match *t {
            StartStop::Start => StartStop::Stop,
            StartStop::Stop => StartStop::Start,
            StartStop::Continue => StartStop::Continue,
        }
    };
    if let VoiceElement::Rest(r) = elem {
        if let Some(t) = &mut r.tuplet {
            flip(&mut t.tuplet_type);
        }
    }
    for n in elem.notes_mut() {
        for tie in &mut n.ties {
            flip(&mut tie.tie_type);
        }
        for slur in &mut n.slurs {
            flip(&mut slur.slur_type);
        }
        if let Some(t) = &mut n.tuplet {
            flip(&mut t.tuplet_type);
        }
        // A word's last syllable now begins it.
        for l in &mut n.lyrics {
            l.syllabic = match l.syllabic {
                SyllabicType::Begin => SyllabicType::End,
                SyllabicType::End => SyllabicType::Begin,
                other => other,
            };
        }
        for b in &mut n.beams {
            let turned = match b.beam_type.as_str() {
                "begin" => "end",
                "end" => "begin",
                "forward hook" => "backward hook",
                "backward hook" => "forward hook",
                _ => continue,
            };
            b.beam_type = turned.to_string();
        }
    }
}

impl MusicTransform for Retrograde {
    fn apply_music(&self, doc: &MusicDocument) -> MusicDocument {
        let mut result = doc.clone();
        retrograde_music_node(&mut result.music);
        // The old last bar opens the reversed music: its pickup is what the
        // last bar lacked of a whole one.
        if let Some(p) = &doc.metadata.partial_duration {
            let bar = first_meter(&doc.music).map_or(Frac::from_integer(1), |t| t.beats_fraction());
            let tail = (doc.music.written_length() - p.actual_duration()) % bar;
            result.metadata.partial_duration =
                (tail > Frac::from_integer(0)).then(|| Duration::new(tail));
        }
        result
    }
}

/// True for events that position the music (key, meter, clef, tempo) rather
/// than being music — a leading run of these stays at the front.
fn is_attribute_event(m: &Music) -> bool {
    matches!(
        m,
        Music::KeySignature(_) | Music::TimeSignature(_) | Music::Clef(_) | Music::Tempo(_)
    )
}

/// Swap paired annotations (ties, slurs, beams, a word's syllables) on a
/// reversed note/chord.
fn swap_annotations(annotations: &mut [Annotation]) {
    for a in annotations {
        *a = match std::mem::replace(a, Annotation::TieStart) {
            Annotation::TieStart => Annotation::TieStop,
            Annotation::TieStop => Annotation::TieStart,
            Annotation::SlurStart { number, .. } => Annotation::SlurStop { number },
            Annotation::SlurStop { number } => Annotation::SlurStart {
                number,
                placement: Default::default(),
            },
            Annotation::BeamStart => Annotation::BeamStop,
            Annotation::BeamStop => Annotation::BeamStart,
            Annotation::Lyric(mut l) => {
                l.syllabic = match l.syllabic {
                    SyllabicType::Begin => SyllabicType::End,
                    SyllabicType::End => SyllabicType::Begin,
                    other => other,
                };
                Annotation::Lyric(l)
            }
            other => other,
        };
    }
}

/// After reversal a `Music::Grace` node trails its principal; move each grace
/// run back in front of the element it ornaments (in original order).
fn reanchor_grace_nodes(children: &mut Vec<Music>) {
    let is_grace = |m: &Music| matches!(m, Music::Grace { .. });
    let old = std::mem::take(children);
    let mut out: Vec<Music> = Vec::with_capacity(old.len());
    let mut iter = old.into_iter().peekable();
    while let Some(e) = iter.next() {
        if is_grace(&e) {
            out.push(e);
            continue;
        }
        let mut run: Vec<Music> = Vec::new();
        while iter.peek().is_some_and(&is_grace) {
            run.push(iter.next().unwrap());
        }
        if run.is_empty() {
            out.push(e);
        } else {
            run.reverse();
            out.extend(run);
            out.push(e);
        }
    }
    *children = out;
}

/// After reversal a `Music::Partial` trails the bar it sizes; move each one
/// back over that bar's music to its start.
fn reanchor_partials(children: &mut [Music]) {
    for i in 0..children.len() {
        if let Music::Partial(d) = &children[i] {
            let want = d.actual_duration();
            let (mut j, mut got) = (i, Frac::from_integer(0));
            while j > 0
                && got < want
                && !matches!(children[j - 1], Music::Barline(_) | Music::Partial(_))
            {
                got += children[j - 1].written_length();
                children.swap(j - 1, j);
                j -= 1;
            }
        }
    }
}

/// The first time signature in the music.
fn first_meter(m: &Music) -> Option<&crate::ir::measure::TimeSignature> {
    match m {
        Music::TimeSignature(t) => Some(t),
        Music::Sequential(items) | Music::Simultaneous(items) => items.iter().find_map(first_meter),
        Music::Context { content, .. }
        | Music::Variable { content, .. }
        | Music::Tuplet { content, .. } => first_meter(content),
        _ => None,
    }
}

/// Recursively reverse Sequential children in a Music tree.
fn retrograde_music_node(music: &mut Music) {
    match music {
        Music::Sequential(children) => {
            // A leading attribute run (\key \time \clef \tempo) stays put —
            // reversing it would migrate the declarations to the end.
            let split = children
                .iter()
                .position(|c| !is_attribute_event(c))
                .unwrap_or(children.len());
            let mut tail: Vec<Music> = children.split_off(split);
            // So does the closing bar line (`|]`).
            let closing = tail.len()
                - tail
                    .iter()
                    .rev()
                    .take_while(|c| matches!(c, Music::Barline(_)))
                    .count();
            let end = tail.split_off(closing);
            tail.reverse();
            reanchor_grace_nodes(&mut tail);
            reanchor_partials(&mut tail);
            children.extend(tail);
            children.extend(end);
            for child in children {
                retrograde_music_node(child);
            }
        }
        Music::Note { annotations, .. } => {
            swap_annotations(annotations);
        }
        Music::Chord {
            annotations,
            pitches,
            ..
        } => {
            swap_annotations(annotations);
            for (_, per_note) in pitches.iter_mut() {
                swap_annotations(per_note);
            }
        }
        Music::Simultaneous(children) => {
            // Don't reverse simultaneous — each voice gets retrograded independently
            for child in children {
                retrograde_music_node(child);
            }
        }
        Music::Context { content, .. }
        | Music::Grace { content, .. }
        | Music::Tuplet { content, .. }
        | Music::Variable { content, .. } => {
            retrograde_music_node(content);
        }
        Music::Repeat {
            body, alternatives, ..
        } => {
            retrograde_music_node(body);
            for alt in alternatives {
                retrograde_music_node(alt);
            }
        }
        _ => {}
    }
}

/// Functional API: reverse all voice elements and measure order.
pub fn retrograde(score: &Score) -> Score {
    Retrograde::new().apply(score)
}

/// Functional API: reverse Sequential children in a Music tree.
pub fn retrograde_music(doc: &MusicDocument) -> MusicDocument {
    Retrograde::new().apply_music(doc)
}

#[cfg(test)]
mod tests {
    #[test]
    fn keys_and_clefs_go_with_their_music() {
        use crate::adapters::ly_to_ir::LyToIrAdapter;
        use crate::adapters::ToIrAdapter;
        use crate::ir::measure::ClefSign;
        let score = LyToIrAdapter::new()
            .convert_str(r"{ \key g \major g'1 | \key f \major f'2 \clef bass f2 }")
            .unwrap();
        let r = Retrograde::new().apply(&score);
        let m = &r.parts()[0].measures;
        let attrs = |i: usize| m[i].attributes.clone().unwrap_or_default();
        // Bar 1 holds the F-major bar, which ended in the bass clef; its
        // clef change comes back to the treble clef halfway.
        assert_eq!(attrs(0).key.map(|k| k.fifths), Some(-1));
        assert_eq!(attrs(0).clefs.get(&1).map(|c| c.sign), Some(ClefSign::F));
        let change: Vec<_> = m[0]
            .directions
            .iter()
            .filter_map(|d| Some((d.offset_frac, d.clef?.sign)))
            .collect();
        assert_eq!(change, [(Frac::new(1, 2), ClefSign::G)]);
        // Bar 2 holds the G-major bar, in the treble clef already in force.
        assert_eq!(attrs(1).key.map(|k| k.fifths), Some(1));
        assert!(attrs(1).clefs.is_empty());
    }

    use super::*;
    use crate::ir::duration::Duration;
    use crate::ir::measure::Measure;
    use crate::ir::note::{Note, VoiceElement};
    use crate::ir::pitch::{Pitch, PitchStep};
    use crate::ir::score::{Score, ScoreChild};
    use crate::ir::voice::Voice;
    use crate::ir::Part;

    fn make_three_note_score() -> Score {
        let n1 = Note::new(Pitch::new(PitchStep::C, 4), Duration::quarter());
        let n2 = Note::new(Pitch::new(PitchStep::D, 4), Duration::quarter());
        let n3 = Note::new(Pitch::new(PitchStep::E, 4), Duration::quarter());
        let voice = Voice {
            number: 1,
            elements: vec![
                VoiceElement::Note(Box::new(n1)),
                VoiceElement::Note(Box::new(n2)),
                VoiceElement::Note(Box::new(n3)),
            ],
        };
        let measure = Measure {
            number: 1,
            number_label: None,
            implicit: false,
            senza_misura: false,
            width: None,
            attributes: None,
            left_barline: None,
            right_barline: None,
            directions: vec![],
            harmonies: vec![],
            figured_bass: vec![],
            print_object: true,
            multi_measure_rest: None,
            measure_repeat: None,
            voices: vec![voice],
        };
        let mut part = Part::new("P1");
        part.measures.push(measure);
        let mut score = Score::new();
        score.children.push(ScoreChild::Part(part));
        score
    }

    fn extract_pitches(score: &Score) -> Vec<PitchStep> {
        let parts = score.parts();
        parts[0].measures[0].voices[0]
            .elements
            .iter()
            .filter_map(|e| match e {
                VoiceElement::Note(n) => Some(n.pitch.step),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn retrograde_basic() {
        let score = make_three_note_score();
        let result = retrograde(&score);
        let pitches = extract_pitches(&result);
        assert_eq!(pitches, vec![PitchStep::E, PitchStep::D, PitchStep::C]);
    }

    #[test]
    fn retrograde_self_inverse() {
        let score = make_three_note_score();
        let doubled = retrograde(&retrograde(&score));
        let orig_pitches = extract_pitches(&score);
        let round_pitches = extract_pitches(&doubled);
        assert_eq!(orig_pitches, round_pitches, "retrograde is self-inverse");
    }

    #[test]
    fn retrograde_no_mutation() {
        let score = make_three_note_score();
        let original = score.clone();
        let _ = retrograde(&score);
        assert_eq!(score, original, "input must not be mutated");
    }

    #[test]
    fn retrograde_empty_score() {
        let score = Score::new();
        let result = retrograde(&score);
        assert_eq!(result, score);
    }

    #[test]
    fn retrograde_music_basic() {
        use crate::ir::music::{Music, MusicDocument};

        let doc = MusicDocument::new(Music::Sequential(vec![
            Music::Note {
                pitch: Pitch::new(PitchStep::C, 4),
                duration: Duration::quarter(),
                annotations: vec![],
            },
            Music::Note {
                pitch: Pitch::new(PitchStep::D, 4),
                duration: Duration::quarter(),
                annotations: vec![],
            },
            Music::Note {
                pitch: Pitch::new(PitchStep::E, 4),
                duration: Duration::quarter(),
                annotations: vec![],
            },
        ]));
        let result = super::retrograde_music(&doc);
        match &result.music {
            Music::Sequential(children) => {
                let steps: Vec<_> = children
                    .iter()
                    .filter_map(|c| match c {
                        Music::Note { pitch, .. } => Some(pitch.step),
                        _ => None,
                    })
                    .collect();
                assert_eq!(steps, vec![PitchStep::E, PitchStep::D, PitchStep::C]);
            }
            _ => panic!("expected Sequential"),
        }
    }

    #[test]
    fn retrograde_music_self_inverse() {
        use crate::ir::music::{Music, MusicDocument};

        let doc = MusicDocument::new(Music::Sequential(vec![
            Music::Note {
                pitch: Pitch::new(PitchStep::C, 4),
                duration: Duration::quarter(),
                annotations: vec![],
            },
            Music::Note {
                pitch: Pitch::new(PitchStep::E, 4),
                duration: Duration::quarter(),
                annotations: vec![],
            },
        ]));
        let doubled = super::retrograde_music(&super::retrograde_music(&doc));
        assert_eq!(doc, doubled);
    }
}

/// Regression tests (review R4): retrograde must keep the score playable —
/// attributes stay at the (new) start, tie/slur/tuplet pairing still opens
/// before it closes, grace notes still precede their principal.
#[cfg(test)]
mod structure_tests {
    use super::*;
    use crate::ir::articulation::{StartStop, TieEvent, TupletDisplay};
    use crate::ir::duration::Duration;
    use crate::ir::measure::{KeyMode, KeySignature, Measure, MeasureAttributes, TimeSignature};
    use crate::ir::note::{Note, VoiceElement};
    use crate::ir::pitch::{Pitch, PitchStep};
    use crate::ir::score::{Score, ScoreChild};
    use crate::ir::voice::Voice;
    use crate::ir::Part;

    fn note(step: PitchStep) -> Note {
        Note::new(Pitch::new(step, 4), Duration::quarter())
    }

    fn score_of(measures: Vec<Measure>) -> Score {
        let mut part = Part::new("P1");
        part.measures = measures;
        let mut score = Score::new();
        score.children.push(ScoreChild::Part(part));
        score
    }

    fn measure_of(number: u32, notes: Vec<Note>) -> Measure {
        let mut m = Measure::new(number);
        m.voices.push(Voice {
            number: 1,
            elements: notes
                .into_iter()
                .map(|n| VoiceElement::Note(Box::new(n)))
                .collect(),
        });
        m
    }

    #[test]
    fn attributes_stay_on_first_measure() {
        let mut m1 = measure_of(1, vec![note(PitchStep::C), note(PitchStep::D)]);
        m1.attributes = Some(MeasureAttributes {
            key: Some(KeySignature {
                fifths: 2,
                mode: KeyMode::Major,
            }),
            time: Some(TimeSignature::default()),
            ..MeasureAttributes::default()
        });
        let m2 = measure_of(2, vec![note(PitchStep::E), note(PitchStep::F)]);
        let result = retrograde(&score_of(vec![m1, m2]));
        let measures = &result.parts()[0].measures;
        assert!(
            measures[0].attributes.is_some(),
            "reversed score must still declare key/time up front"
        );
        assert!(measures[1].attributes.is_none());
        // Content is reversed: F E | D C.
        let steps: Vec<_> = measures
            .iter()
            .flat_map(|m| &m.voices)
            .flat_map(|v| &v.elements)
            .filter_map(|e| match e {
                VoiceElement::Note(n) => Some(n.pitch.step),
                _ => None,
            })
            .collect();
        assert_eq!(
            steps,
            vec![PitchStep::F, PitchStep::E, PitchStep::D, PitchStep::C]
        );
    }

    #[test]
    fn tie_pairing_is_swapped() {
        let mut n1 = note(PitchStep::C);
        n1.ties.push(TieEvent {
            tie_type: StartStop::Start,
        });
        let mut n2 = note(PitchStep::C);
        n2.ties.push(TieEvent {
            tie_type: StartStop::Stop,
        });
        let result = retrograde(&score_of(vec![measure_of(1, vec![n1, n2])]));
        let elems = &result.parts()[0].measures[0].voices[0].elements;
        let tie_of = |e: &VoiceElement| match e {
            VoiceElement::Note(n) => n.ties[0].tie_type,
            _ => panic!("expected note"),
        };
        assert_eq!(tie_of(&elems[0]), StartStop::Start, "chain must open first");
        assert_eq!(tie_of(&elems[1]), StartStop::Stop);
    }

    #[test]
    fn tuplet_markers_are_swapped() {
        let mut n1 = note(PitchStep::C);
        n1.tuplet = Some(TupletDisplay {
            tuplet_type: StartStop::Start,
            bracket: true,
            show_number: "actual".to_string(),
        });
        let mut n3 = note(PitchStep::E);
        n3.tuplet = Some(TupletDisplay {
            tuplet_type: StartStop::Stop,
            bracket: true,
            show_number: "actual".to_string(),
        });
        let result = retrograde(&score_of(vec![measure_of(
            1,
            vec![n1, note(PitchStep::D), n3],
        )]));
        let elems = &result.parts()[0].measures[0].voices[0].elements;
        let tuplet_of = |e: &VoiceElement| match e {
            VoiceElement::Note(n) => n.tuplet.as_ref().map(|t| t.tuplet_type),
            _ => None,
        };
        assert_eq!(tuplet_of(&elems[0]), Some(StartStop::Start));
        assert_eq!(tuplet_of(&elems[2]), Some(StartStop::Stop));
    }

    #[test]
    fn grace_notes_stay_before_their_principal() {
        let mut g = note(PitchStep::B);
        g.is_grace = true;
        let result = retrograde(&score_of(vec![measure_of(
            1,
            vec![note(PitchStep::C), g, note(PitchStep::D)],
        )]));
        let elems = &result.parts()[0].measures[0].voices[0].elements;
        let flags: Vec<(PitchStep, bool)> = elems
            .iter()
            .filter_map(|e| match e {
                VoiceElement::Note(n) => Some((n.pitch.step, n.is_grace)),
                _ => None,
            })
            .collect();
        assert_eq!(
            flags,
            vec![
                (PitchStep::B, true),  // grace still precedes its principal
                (PitchStep::D, false), // reversed principal order: D then C
                (PitchStep::C, false),
            ]
        );
    }

    #[test]
    fn structural_retrograde_is_self_inverse() {
        let mut m1 = measure_of(1, vec![note(PitchStep::C), note(PitchStep::D)]);
        m1.attributes = Some(MeasureAttributes::default());
        let mut g = note(PitchStep::B);
        g.is_grace = true;
        let mut tied = note(PitchStep::E);
        tied.ties.push(TieEvent {
            tie_type: StartStop::Start,
        });
        let m2 = measure_of(2, vec![g, tied, note(PitchStep::F)]);
        let score = score_of(vec![m1, m2]);
        assert_eq!(retrograde(&retrograde(&score)), score);
    }

    #[test]
    fn music_layer_keeps_leading_attributes() {
        use crate::ir::music::{Music, MusicDocument};
        let doc = MusicDocument::new(Music::Sequential(vec![
            Music::KeySignature(KeySignature {
                fifths: 1,
                mode: KeyMode::Major,
            }),
            Music::TimeSignature(TimeSignature::default()),
            Music::Note {
                pitch: Pitch::new(PitchStep::C, 4),
                duration: Duration::quarter(),
                annotations: vec![],
            },
            Music::Note {
                pitch: Pitch::new(PitchStep::D, 4),
                duration: Duration::quarter(),
                annotations: vec![],
            },
        ]));
        let result = retrograde_music(&doc);
        match &result.music {
            Music::Sequential(children) => {
                assert!(
                    matches!(children[0], Music::KeySignature(_)),
                    "key must not migrate to the end"
                );
                assert!(matches!(children[1], Music::TimeSignature(_)));
                match (&children[2], &children[3]) {
                    (Music::Note { pitch: p1, .. }, Music::Note { pitch: p2, .. }) => {
                        assert_eq!((p1.step, p2.step), (PitchStep::D, PitchStep::C));
                    }
                    other => panic!("expected two notes, got {other:?}"),
                }
            }
            other => panic!("expected Sequential, got {other:?}"),
        }
        assert_eq!(retrograde_music(&retrograde_music(&doc)), doc);
    }

    #[test]
    fn music_layer_grace_and_tie_annotations() {
        use crate::ir::annotation::Annotation;
        use crate::ir::music::{Music, MusicDocument};
        let doc = MusicDocument::new(Music::Sequential(vec![
            Music::Grace {
                content: Box::new(Music::Note {
                    pitch: Pitch::new(PitchStep::B, 4),
                    duration: Duration::quarter(),
                    annotations: vec![],
                }),
                slash: true,
            },
            Music::Note {
                pitch: Pitch::new(PitchStep::C, 4),
                duration: Duration::quarter(),
                annotations: vec![Annotation::TieStart],
            },
            Music::Note {
                pitch: Pitch::new(PitchStep::C, 4),
                duration: Duration::quarter(),
                annotations: vec![Annotation::TieStop],
            },
        ]));
        let result = retrograde_music(&doc);
        match &result.music {
            Music::Sequential(children) => {
                assert!(
                    matches!(children[1], Music::Grace { .. }),
                    "grace must precede its principal, got {children:?}"
                );
                match (&children[0], &children[2]) {
                    (
                        Music::Note {
                            annotations: a1, ..
                        },
                        Music::Note {
                            annotations: a2, ..
                        },
                    ) => {
                        assert!(matches!(a1[0], Annotation::TieStart), "chain opens first");
                        assert!(matches!(a2[0], Annotation::TieStop));
                    }
                    other => panic!("expected notes, got {other:?}"),
                }
            }
            other => panic!("expected Sequential, got {other:?}"),
        }
        assert_eq!(retrograde_music(&retrograde_music(&doc)), doc);
    }
}
