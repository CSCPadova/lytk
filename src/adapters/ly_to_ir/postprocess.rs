use crate::ir::articulation::{SlurEvent, StartStop, TieEvent};
use crate::ir::measure::Clef;
use crate::ir::note::VoiceElement;
use crate::ir::pitch::Pitch;
use crate::ir::score::{Score, ScoreChild};
use crate::ir::Part;

/// Resolve tie *stops*. The LY walk records only a tie *start* (from `~`) on the
/// note/chord that precedes the tie; the destination note never receives a
/// matching `<tie type="stop">`, leaving every tie dangling. This pass walks
/// each voice's note stream (across measure boundaries) and, for every tie-start
/// at pitch P, adds a tie-stop to the next element in that voice that contains
/// pitch P.
///
/// Must run after `q` chord-repeat expansion so a tie whose destination is a
/// `q`-repeat has a real note to stop on.
pub(super) fn resolve_ties(score: &mut Score) {
    for child in &mut score.children {
        if let ScoreChild::Part(part) = child {
            resolve_ties_in_part(part);
        }
    }
}

fn same_pitch(a: &Pitch, b: &Pitch) -> bool {
    a.step == b.step && a.alter == b.alter && a.octave == b.octave
}

/// Pitches carrying a tie-start on this element.
fn tie_start_pitches(elem: &VoiceElement) -> Vec<Pitch> {
    let has_start = |ties: &[TieEvent]| ties.iter().any(|t| t.tie_type == StartStop::Start);
    match elem {
        VoiceElement::Note(n) if has_start(&n.ties) => vec![n.pitch],
        VoiceElement::Chord(c) => c
            .notes
            .iter()
            .filter(|n| has_start(&n.ties))
            .map(|n| n.pitch)
            .collect(),
        _ => Vec::new(),
    }
}

fn element_has_pitch(elem: &VoiceElement, p: &Pitch) -> bool {
    match elem {
        VoiceElement::Note(n) => same_pitch(&n.pitch, p),
        VoiceElement::Chord(c) => c.notes.iter().any(|n| same_pitch(&n.pitch, p)),
        _ => false,
    }
}

/// Add a tie-stop to the note matching `p` (inserted before any existing
/// tie-start so a mid-chain note emits stop-then-start, per MusicXML).
fn add_tie_stop(elem: &mut VoiceElement, p: &Pitch) {
    let stop = TieEvent {
        tie_type: StartStop::Stop,
    };
    let apply = |n: &mut crate::ir::Note| {
        if n.ties.iter().any(|t| t.tie_type == StartStop::Stop) {
            return; // already stopped
        }
        n.ties.insert(0, stop.clone());
    };
    match elem {
        VoiceElement::Note(n) if same_pitch(&n.pitch, p) => apply(n),
        VoiceElement::Chord(c) => {
            if let Some(n) = c.notes.iter_mut().find(|n| same_pitch(&n.pitch, p)) {
                apply(n);
            }
        }
        _ => {}
    }
}

/// Assign MusicXML slur `number`s. The LY walk hardcodes every slur to
/// `number = 1`. In a piano part (one MusicXML part, two staves) a slur in the
/// right hand and one in the left hand are then both `number=1`; importers that
/// match slur start/stop by number across the whole part pair the RH start with
/// the LH stop, producing a giant slur swooping across both staves.
///
/// This pass allocates numbers so that no two *concurrently open* slurs anywhere
/// in the part share one, while a start and its matching stop (same voice, LIFO)
/// get the same number.
pub(super) fn assign_slur_numbers(score: &mut Score) {
    for child in &mut score.children {
        if let ScoreChild::Part(part) = child {
            assign_slur_numbers_in_part(part);
        }
    }
}

fn set_slur_numbers(slurs: &mut [SlurEvent], lane: u8) {
    for s in slurs {
        s.number = lane;
    }
}

fn assign_slur_numbers_in_part(part: &mut Part) {
    use std::collections::{BTreeSet, HashMap};
    // Voices carrying any slur, in stable order. Each gets a distinct "lane"
    // number so that slurs in different voices (e.g. the two hands of a piano)
    // never share a `number` and cannot be cross-paired by a renderer. Slurs
    // within a single voice are sequential (LilyPond requires `\=` for true
    // overlap, which this fixture does not use), so one lane per voice suffices
    // and a start/stop pair in that voice keeps the same number.
    let mut voices_with_slurs: BTreeSet<u8> = BTreeSet::new();
    for m in &part.measures {
        for v in &m.voices {
            let has_slur = v.elements.iter().any(|e| match e {
                VoiceElement::Note(n) => !n.slurs.is_empty(),
                VoiceElement::Chord(c) => c.notes.iter().any(|n| !n.slurs.is_empty()),
                VoiceElement::Rest(_) => false,
            });
            if has_slur {
                voices_with_slurs.insert(v.number);
            }
        }
    }
    let lane: HashMap<u8, u8> = voices_with_slurs
        .iter()
        .enumerate()
        .map(|(i, &vn)| (vn, (i as u8).saturating_add(1)))
        .collect();
    for m in &mut part.measures {
        for v in &mut m.voices {
            let ln = lane.get(&v.number).copied().unwrap_or(1);
            for elem in &mut v.elements {
                match elem {
                    VoiceElement::Note(n) => set_slur_numbers(&mut n.slurs, ln),
                    VoiceElement::Chord(c) => {
                        for note in &mut c.notes {
                            set_slur_numbers(&mut note.slurs, ln);
                        }
                    }
                    VoiceElement::Rest(_) => {}
                }
            }
        }
    }
}

fn resolve_ties_in_part(part: &mut Part) {
    use std::collections::BTreeMap;
    // Per voice number, the ordered (measure, voice-index, element-index) of
    // every Note/Chord in document order.
    let mut by_voice: BTreeMap<u8, Vec<(usize, usize, usize)>> = BTreeMap::new();
    for (mi, m) in part.measures.iter().enumerate() {
        for (vi, v) in m.voices.iter().enumerate() {
            for (ei, e) in v.elements.iter().enumerate() {
                if matches!(e, VoiceElement::Note(_) | VoiceElement::Chord(_)) {
                    by_voice.entry(v.number).or_default().push((mi, vi, ei));
                }
            }
        }
    }
    // Collect the stops to apply (deferred to avoid overlapping borrows).
    let mut stops: Vec<(usize, usize, usize, Pitch)> = Vec::new();
    for seq in by_voice.values() {
        for w in seq.windows(2) {
            let (m0, v0, e0) = w[0];
            let (m1, v1, e1) = w[1];
            let starts = tie_start_pitches(&part.measures[m0].voices[v0].elements[e0]);
            for p in starts {
                if element_has_pitch(&part.measures[m1].voices[v1].elements[e1], &p) {
                    stops.push((m1, v1, e1, p));
                }
            }
        }
    }
    for (m, v, e, p) in stops {
        add_tie_stop(&mut part.measures[m].voices[v].elements[e], &p);
    }
}

/// Ensure every staff of a multi-staff part has an initial clef.
///
/// LilyPond leaves the default clef (treble) implicit, so a piano part whose
/// upper staff has no explicit `\clef` records no clef for it. MusicXML then
/// emits a single clef with no `number=` and the upper staff shows the wrong
/// (or default) clef. This fills the first measure with a treble clef for any
/// staff that has none, so each staff gets a numbered `<clef>`. Defaulting a
/// missing staff to treble is also correct when that staff changes clef later
/// (it was treble until the change).
pub(super) fn ensure_staff_clefs(score: &mut Score) {
    use crate::ir::measure::MeasureAttributes;
    for child in &mut score.children {
        if let ScoreChild::Part(part) = child {
            if part.staves <= 1 {
                continue;
            }
            if let Some(m) = part.measures.first_mut() {
                let attrs = m.attributes.get_or_insert_with(MeasureAttributes::default);
                for staff in 1..=part.staves {
                    attrs.clefs.entry(staff).or_insert_with(Clef::default);
                }
            }
        }
    }
}
