use crate::adapters::ir_to_mxml::IrToMxmlAdapter;
use crate::adapters::FromIrAdapter;
use crate::ir::articulation::{Articulation, Fermata, Placement, SlurEvent, StartStop, TieEvent};
use crate::ir::direction::{Direction, TempoDirection};
use crate::ir::duration::Duration;
use crate::ir::measure::{Clef, KeyMode, KeySignature, MeasureAttributes, TimeSignature};
use crate::ir::note::{Chord, Note, Rest, VoiceElement};
use crate::ir::pitch::{Pitch, PitchStep};
use crate::ir::score::{Score, ScoreChild, ScoreMetadata};
use crate::ir::voice::Voice;
use crate::ir::Part;
use std::collections::HashMap;

fn make_simple_score() -> Score {
    let mut attrs = MeasureAttributes {
        divisions: 4,
        key: Some(KeySignature {
            fifths: 0,
            mode: KeyMode::Major,
        }),
        time: Some(TimeSignature::default()),
        clefs: HashMap::new(),
        transpose: None,
        staves: None,
        staff_lines: None,
    };
    attrs.clefs.insert(1, Clef::default());

    let note = Note::new(Pitch::new(PitchStep::C, 4), Duration::quarter());
    let voice = Voice {
        number: 1,
        elements: vec![VoiceElement::Note(Box::new(note))],
    };
    let measure = crate::ir::measure::Measure {
        number: 1,
        implicit: false,
        width: None,
        attributes: Some(attrs),
        left_barline: None,
        right_barline: None,
        directions: vec![],
        harmonies: vec![],
        figured_bass: vec![],
        print_object: true,
        multi_measure_rest: None,
        voices: vec![voice],
    };

    let part = Part {
        name: "Piano".to_string(),
        abbreviation: "Pno.".to_string(),
        part_id: "P1".to_string(),
        midi_instrument: String::new(),
        midi_channel: 0,
        midi_program: 0,
        staves: 1,
        measures: vec![measure],
    };

    Score {
        metadata: ScoreMetadata {
            title: Some("Test".to_string()),
            composer: Some("Composer".to_string()),
            ..Default::default()
        },
        page_layout: None,
        children: vec![ScoreChild::Part(part)],
    }
}

#[test]
fn simple_score_structure() {
    let score = make_simple_score();
    let adapter = IrToMxmlAdapter::new();
    let xml = adapter.convert(&score).unwrap();

    assert!(xml.contains("<score-partwise"));
    assert!(xml.contains("<movement-title>Test</movement-title>"));
    assert!(xml.contains("type=\"composer\""));
    assert!(xml.contains("Composer"));
    assert!(xml.contains("<software>lytk</software>"));
    assert!(xml.contains("<score-part id=\"P1\""));
    assert!(xml.contains("<part-name>Piano</part-name>"));
    assert!(xml.contains("<part-abbreviation>Pno.</part-abbreviation>"));
    assert!(xml.contains("<part id=\"P1\""));
}

#[test]
fn attributes_key_time_clef() {
    let score = make_simple_score();
    let adapter = IrToMxmlAdapter::new();
    let xml = adapter.convert(&score).unwrap();

    assert!(xml.contains("<divisions>4</divisions>"));
    assert!(xml.contains("<fifths>0</fifths>"));
    assert!(xml.contains("<mode>major</mode>"));
    assert!(xml.contains("<beats>4</beats>"));
    assert!(xml.contains("<beat-type>4</beat-type>"));
    assert!(xml.contains("<sign>G</sign>"));
    assert!(xml.contains("<line>2</line>"));
}

#[test]
fn note_with_pitch() {
    let score = make_simple_score();
    let adapter = IrToMxmlAdapter::new();
    let xml = adapter.convert(&score).unwrap();

    assert!(xml.contains("<step>C</step>"));
    assert!(xml.contains("<octave>4</octave>"));
    assert!(xml.contains("<duration>4</duration>"));
    assert!(xml.contains("<voice>1</voice>"));
    assert!(xml.contains("<type>quarter</type>"));
}

#[test]
fn rest_emission() {
    let rest = Rest::new(Duration::half());
    let voice = Voice {
        number: 1,
        elements: vec![VoiceElement::Rest(rest)],
    };
    let measure = crate::ir::measure::Measure {
        number: 1,
        implicit: false,
        width: None,
        attributes: None,
        left_barline: None,
        right_barline: None,
        directions: vec![],
        harmonies: vec![],
        figured_bass: vec![],
        print_object: true,
        multi_measure_rest: None,
        voices: vec![voice],
    };
    let part = Part {
        name: String::new(),
        part_id: "P1".to_string(),
        ..Part::new("P1")
    };
    let mut score = Score::new();
    let mut part = part;
    part.measures.push(measure);
    score.children.push(ScoreChild::Part(part));

    let adapter = IrToMxmlAdapter::new();
    let xml = adapter.convert(&score).unwrap();

    assert!(xml.contains("<rest/>"));
    assert!(xml.contains("<duration>8</duration>"));
    assert!(xml.contains("<type>half</type>"));
}

#[test]
fn measure_rest() {
    let rest = Rest::measure_rest(Duration::whole());
    let voice = Voice {
        number: 1,
        elements: vec![VoiceElement::Rest(rest)],
    };
    let measure = crate::ir::measure::Measure {
        number: 1,
        implicit: false,
        width: None,
        attributes: None,
        left_barline: None,
        right_barline: None,
        directions: vec![],
        harmonies: vec![],
        figured_bass: vec![],
        print_object: true,
        multi_measure_rest: None,
        voices: vec![voice],
    };
    let mut part = Part::new("P1");
    part.measures.push(measure);
    let mut score = Score::new();
    score.children.push(ScoreChild::Part(part));

    let adapter = IrToMxmlAdapter::new();
    let xml = adapter.convert(&score).unwrap();

    assert!(xml.contains("measure=\"yes\""));
}

#[test]
fn chord_emission() {
    let n1 = Note::new(Pitch::new(PitchStep::C, 4), Duration::quarter());
    let n2 = Note::new(Pitch::new(PitchStep::E, 4), Duration::quarter());
    let n3 = Note::new(Pitch::new(PitchStep::G, 4), Duration::quarter());
    let chord = Chord::new(Duration::quarter(), vec![n1, n2, n3]);
    let voice = Voice {
        number: 1,
        elements: vec![VoiceElement::Chord(chord)],
    };
    let measure = crate::ir::measure::Measure {
        number: 1,
        implicit: false,
        width: None,
        attributes: None,
        left_barline: None,
        right_barline: None,
        directions: vec![],
        harmonies: vec![],
        figured_bass: vec![],
        print_object: true,
        multi_measure_rest: None,
        voices: vec![voice],
    };
    let mut part = Part::new("P1");
    part.measures.push(measure);
    let mut score = Score::new();
    score.children.push(ScoreChild::Part(part));

    let adapter = IrToMxmlAdapter::new();
    let xml = adapter.convert(&score).unwrap();

    // First note has no <chord/>, subsequent notes do
    let chord_count = xml.matches("<chord/>").count();
    assert_eq!(
        chord_count, 2,
        "expected 2 <chord/> tags for a 3-note chord"
    );
    assert!(xml.contains("<step>C</step>"));
    assert!(xml.contains("<step>E</step>"));
    assert!(xml.contains("<step>G</step>"));
}

#[test]
fn note_with_tie() {
    let mut note = Note::new(Pitch::new(PitchStep::D, 4), Duration::half());
    note.ties.push(TieEvent {
        tie_type: StartStop::Start,
    });
    let voice = Voice {
        number: 1,
        elements: vec![VoiceElement::Note(Box::new(note))],
    };
    let measure = crate::ir::measure::Measure {
        number: 1,
        implicit: false,
        width: None,
        attributes: None,
        left_barline: None,
        right_barline: None,
        directions: vec![],
        harmonies: vec![],
        figured_bass: vec![],
        print_object: true,
        multi_measure_rest: None,
        voices: vec![voice],
    };
    let mut part = Part::new("P1");
    part.measures.push(measure);
    let mut score = Score::new();
    score.children.push(ScoreChild::Part(part));

    let adapter = IrToMxmlAdapter::new();
    let xml = adapter.convert(&score).unwrap();

    assert!(xml.contains("<tie type=\"start\""));
    assert!(xml.contains("<tied type=\"start\""));
}

#[test]
fn note_with_articulations() {
    let mut note = Note::new(Pitch::new(PitchStep::E, 5), Duration::eighth());
    note.articulations.push(Articulation {
        name: "staccato".to_string(),
        placement: Placement::Above,
    });
    note.slurs.push(SlurEvent {
        slur_type: StartStop::Start,
        number: 1,
        placement: Placement::Above,
    });
    let voice = Voice {
        number: 1,
        elements: vec![VoiceElement::Note(Box::new(note))],
    };
    let measure = crate::ir::measure::Measure {
        number: 1,
        implicit: false,
        width: None,
        attributes: None,
        left_barline: None,
        right_barline: None,
        directions: vec![],
        harmonies: vec![],
        figured_bass: vec![],
        print_object: true,
        multi_measure_rest: None,
        voices: vec![voice],
    };
    let mut part = Part::new("P1");
    part.measures.push(measure);
    let mut score = Score::new();
    score.children.push(ScoreChild::Part(part));

    let adapter = IrToMxmlAdapter::new();
    let xml = adapter.convert(&score).unwrap();

    assert!(xml.contains("<notations>"));
    assert!(xml.contains("<slur type=\"start\""));
    assert!(xml.contains("<articulations>"));
    assert!(xml.contains("<staccato/>"));
}

#[test]
fn direction_with_dynamics() {
    use crate::ir::articulation::DynamicMark;

    let dir = Direction {
        dynamic: Some(DynamicMark {
            sign: "ff".to_string(),
            placement: Placement::Below,
        }),
        ..Default::default()
    };
    let measure = crate::ir::measure::Measure {
        number: 1,
        implicit: false,
        width: None,
        attributes: None,
        left_barline: None,
        right_barline: None,
        directions: vec![dir],
        harmonies: vec![],
        figured_bass: vec![],
        print_object: true,
        multi_measure_rest: None,
        voices: vec![],
    };
    let mut part = Part::new("P1");
    part.measures.push(measure);
    let mut score = Score::new();
    score.children.push(ScoreChild::Part(part));

    let adapter = IrToMxmlAdapter::new();
    let xml = adapter.convert(&score).unwrap();

    assert!(xml.contains("<direction>"));
    assert!(xml.contains("<dynamics>"));
    assert!(xml.contains("<ff/>"));
}

#[test]
fn direction_with_tempo() {
    let dir = Direction {
        tempo: Some(TempoDirection {
            text: None,
            beat_unit: Some("quarter".to_string()),
            per_minute: Some(120.0),
            dots: 0,
            placement: Placement::Above,
        }),
        ..Default::default()
    };
    let measure = crate::ir::measure::Measure {
        number: 1,
        implicit: false,
        width: None,
        attributes: None,
        left_barline: None,
        right_barline: None,
        directions: vec![dir],
        harmonies: vec![],
        figured_bass: vec![],
        print_object: true,
        multi_measure_rest: None,
        voices: vec![],
    };
    let mut part = Part::new("P1");
    part.measures.push(measure);
    let mut score = Score::new();
    score.children.push(ScoreChild::Part(part));

    let adapter = IrToMxmlAdapter::new();
    let xml = adapter.convert(&score).unwrap();

    assert!(xml.contains("<metronome>"));
    assert!(xml.contains("<beat-unit>quarter</beat-unit>"));
    assert!(xml.contains("<per-minute>120</per-minute>"));
}

#[test]
fn fermata_on_note() {
    let mut note = Note::new(Pitch::new(PitchStep::G, 4), Duration::whole());
    note.fermata = Some(Fermata {
        shape: "normal".to_string(),
        inverted: false,
    });
    let voice = Voice {
        number: 1,
        elements: vec![VoiceElement::Note(Box::new(note))],
    };
    let measure = crate::ir::measure::Measure {
        number: 1,
        implicit: false,
        width: None,
        attributes: None,
        left_barline: None,
        right_barline: None,
        directions: vec![],
        harmonies: vec![],
        figured_bass: vec![],
        print_object: true,
        multi_measure_rest: None,
        voices: vec![voice],
    };
    let mut part = Part::new("P1");
    part.measures.push(measure);
    let mut score = Score::new();
    score.children.push(ScoreChild::Part(part));

    let adapter = IrToMxmlAdapter::new();
    let xml = adapter.convert(&score).unwrap();

    assert!(xml.contains("<fermata>normal</fermata>"));
}

#[test]
fn multi_voice_backup() {
    let n1 = Note::new(Pitch::new(PitchStep::C, 5), Duration::quarter());
    let n2 = Note::new(Pitch::new(PitchStep::E, 4), Duration::quarter());
    let v1 = Voice {
        number: 1,
        elements: vec![VoiceElement::Note(Box::new(n1))],
    };
    let v2 = Voice {
        number: 2,
        elements: vec![VoiceElement::Note(Box::new(n2))],
    };
    let measure = crate::ir::measure::Measure {
        number: 1,
        implicit: false,
        width: None,
        attributes: None,
        left_barline: None,
        right_barline: None,
        directions: vec![],
        harmonies: vec![],
        figured_bass: vec![],
        print_object: true,
        multi_measure_rest: None,
        voices: vec![v1, v2],
    };
    let mut part = Part::new("P1");
    part.measures.push(measure);
    let mut score = Score::new();
    score.children.push(ScoreChild::Part(part));

    let adapter = IrToMxmlAdapter::new();
    let xml = adapter.convert(&score).unwrap();

    // Should contain a backup between the two voices
    assert!(xml.contains("<backup>"));
    assert!(xml.contains("<voice>1</voice>"));
    assert!(xml.contains("<voice>2</voice>"));
}

// ── accidental display tests ──────────────────────────────────────────

#[test]
fn test_emit_forced_accidental() {
    use crate::ir::pitch::AccidentalDisplay;

    let mut note = Note::new(Pitch::new(PitchStep::F, 4), Duration::quarter());
    note.pitch.alter = crate::ir::pitch::Alter::new(1, 1); // sharp
    note.pitch.accidental = AccidentalDisplay::Forced;

    let xml = emit_single_note(note);
    assert!(xml.contains("<accidental>sharp</accidental>"), "{xml}");
}

#[test]
fn test_emit_cautionary_accidental() {
    use crate::ir::pitch::AccidentalDisplay;

    let mut note = Note::new(Pitch::new(PitchStep::B, 4), Duration::quarter());
    note.pitch.alter = crate::ir::pitch::Alter::new(-1, 1); // flat
    note.pitch.accidental = AccidentalDisplay::Cautionary;

    let xml = emit_single_note(note);
    assert!(
        xml.contains("<accidental cautionary=\"yes\">flat</accidental>"),
        "{xml}"
    );
}

#[test]
fn test_emit_editorial_accidental() {
    use crate::ir::pitch::AccidentalDisplay;

    let mut note = Note::new(Pitch::new(PitchStep::C, 4), Duration::quarter());
    // natural – alter stays 0
    note.pitch.accidental = AccidentalDisplay::Editorial;

    let xml = emit_single_note(note);
    assert!(
        xml.contains("<accidental editorial=\"yes\">natural</accidental>"),
        "{xml}"
    );
}

// ── after-grace steal-time tests ──────────────────────────────────────

#[test]
fn test_emit_after_grace_steal_time() {
    let mut note = Note::new(Pitch::new(PitchStep::D, 5), Duration::eighth());
    note.is_grace = true;
    note.after_grace = true;

    let xml = emit_single_note(note);
    assert!(xml.contains("steal-time-previous=\"100\""), "{xml}");
}

#[test]
fn test_emit_regular_grace_has_no_steal_time() {
    let mut note = Note::new(Pitch::new(PitchStep::D, 5), Duration::eighth());
    note.is_grace = true;
    note.after_grace = false;

    let xml = emit_single_note(note);
    assert!(!xml.contains("steal-time-previous"), "{xml}");
    assert!(xml.contains("<grace/>"), "{xml}");
}

// ── glissando / slide tests ───────────────────────────────────────────

#[test]
fn test_emit_glissando_start() {
    use crate::ir::articulation::StartStop;

    let mut note = Note::new(Pitch::new(PitchStep::C, 4), Duration::quarter());
    note.glissando = Some(StartStop::Start);
    note.glissando_line_type = Some("wavy".to_string());

    let xml = emit_single_note(note);
    assert!(xml.contains("<glissando"), "{xml}");
    assert!(xml.contains("type=\"start\""), "{xml}");
    assert!(xml.contains("line-type=\"wavy\""), "{xml}");
}

#[test]
fn test_emit_slide_stop() {
    use crate::ir::articulation::StartStop;

    let mut note = Note::new(Pitch::new(PitchStep::G, 4), Duration::quarter());
    note.slide = Some(StartStop::Stop);

    let xml = emit_single_note(note);
    assert!(xml.contains("<slide"), "{xml}");
    assert!(xml.contains("type=\"stop\""), "{xml}");
}

// ── arpeggiate / non-arpeggiate tests ────────────────────────────────

#[test]
fn test_emit_arpeggiate_up() {
    use crate::ir::note::ArpeggioType;

    let note1 = Note::new(Pitch::new(PitchStep::C, 4), Duration::quarter());
    let note2 = Note::new(Pitch::new(PitchStep::E, 4), Duration::quarter());
    let mut chord = Chord::new(Duration::quarter(), vec![note1, note2]);
    chord.arpeggio = Some(ArpeggioType::Up);

    let xml = emit_chord(chord);
    assert!(xml.contains("<arpeggiate"), "{xml}");
    assert!(xml.contains("direction=\"up\""), "{xml}");
}

#[test]
fn test_emit_non_arpeggiate() {
    use crate::ir::note::ArpeggioType;

    let note1 = Note::new(Pitch::new(PitchStep::C, 4), Duration::quarter());
    let note2 = Note::new(Pitch::new(PitchStep::G, 4), Duration::quarter());
    let mut chord = Chord::new(Duration::quarter(), vec![note1, note2]);
    chord.arpeggio = Some(ArpeggioType::NonArpeggio);

    let xml = emit_chord(chord);
    assert!(xml.contains("<non-arpeggiate/>"), "{xml}");
}

// ── harmony tests ─────────────────────────────────────────────────────

#[test]
fn test_emit_harmony_basic() {
    use crate::ir::harmony::{ChordPitch, Harmony};

    let harmony = Harmony {
        root: ChordPitch {
            step: "C".to_string(),
            alter: 0.0,
        },
        kind: "major".to_string(),
        bass: None,
        degrees: vec![],
        offset: 0,
    };

    let xml = emit_measure_with_harmony(harmony);
    assert!(xml.contains("<harmony>"), "{xml}");
    assert!(xml.contains("<root-step>C</root-step>"), "{xml}");
    assert!(xml.contains("<kind>major</kind>"), "{xml}");
}

#[test]
fn test_emit_harmony_with_bass() {
    use crate::ir::harmony::{ChordPitch, Harmony};

    let harmony = Harmony {
        root: ChordPitch {
            step: "G".to_string(),
            alter: 0.0,
        },
        kind: "major".to_string(),
        bass: Some(ChordPitch {
            step: "B".to_string(),
            alter: 0.0,
        }),
        degrees: vec![],
        offset: 0,
    };

    let xml = emit_measure_with_harmony(harmony);
    assert!(xml.contains("<bass-step>B</bass-step>"), "{xml}");
}

#[test]
fn test_emit_harmony_with_offset() {
    use crate::ir::harmony::{ChordPitch, Harmony};

    let harmony = Harmony {
        root: ChordPitch {
            step: "F".to_string(),
            alter: 0.0,
        },
        kind: "minor".to_string(),
        bass: None,
        degrees: vec![],
        offset: 4,
    };

    let xml = emit_measure_with_harmony(harmony);
    assert!(xml.contains("<offset>4</offset>"), "{xml}");
}

// ── figured bass tests ────────────────────────────────────────────────

#[test]
fn test_emit_figured_bass() {
    use crate::ir::harmony::{Figure, FiguredBass};

    let fb = FiguredBass {
        figures: vec![
            Figure {
                number: Some(6),
                prefix: None,
                suffix: None,
            },
            Figure {
                number: Some(4),
                prefix: None,
                suffix: None,
            },
        ],
        duration: Duration::quarter(),
        parentheses: false,
        offset: 0,
    };

    let xml = emit_measure_with_figured_bass(fb);
    assert!(xml.contains("<figured-bass>"), "{xml}");
    assert!(xml.contains("<figure-number>6</figure-number>"), "{xml}");
    assert!(xml.contains("<figure-number>4</figure-number>"), "{xml}");
}

// ── page layout / defaults tests ──────────────────────────────────────

#[test]
fn test_emit_page_layout_defaults() {
    use crate::ir::score::PageLayout;

    let layout = PageLayout {
        page_height: Some(29.7),
        page_width: Some(21.0),
        left_margin: Some(1.5),
        right_margin: Some(1.5),
        top_margin: Some(1.5),
        bottom_margin: Some(1.5),
        system_distance: None,
        top_system_distance: None,
        staff_size: Some(20.0),
    };

    let mut score = make_simple_score();
    score.page_layout = Some(layout);

    let adapter = IrToMxmlAdapter::new();
    let xml = adapter.convert(&score).unwrap();

    assert!(xml.contains("<defaults>"), "{xml}");
    assert!(xml.contains("<scaling>"), "{xml}");
    assert!(xml.contains("<page-layout>"), "{xml}");
    assert!(xml.contains("<page-height>"), "{xml}");
    assert!(xml.contains("<page-margins"), "{xml}");
}

// ── coda / segno / da_capo / dal_segno tests ─────────────────────────

#[test]
fn test_emit_coda_direction() {
    use crate::ir::direction::Direction;

    let mut dir = Direction::default();
    dir.coda = true;

    let xml = emit_direction(dir);
    assert!(xml.contains("<coda/>"), "{xml}");
}

#[test]
fn test_emit_segno_direction() {
    use crate::ir::direction::Direction;

    let mut dir = Direction::default();
    dir.segno = true;

    let xml = emit_direction(dir);
    assert!(xml.contains("<segno/>"), "{xml}");
}

#[test]
fn test_emit_da_capo_direction() {
    use crate::ir::direction::Direction;

    let mut dir = Direction::default();
    dir.da_capo = Some("D.C.".to_string());

    let xml = emit_direction(dir);
    assert!(xml.contains("<words>D.C.</words>"), "{xml}");
    assert!(xml.contains("<sound dacapo=\"yes\"/>"), "{xml}");
}

#[test]
fn test_emit_dal_segno_direction() {
    use crate::ir::direction::Direction;

    let mut dir = Direction::default();
    dir.dal_segno = Some("D.S.".to_string());

    let xml = emit_direction(dir);
    assert!(xml.contains("<words>D.S.</words>"), "{xml}");
    assert!(xml.contains("<sound dalsegno=\"yes\"/>"), "{xml}");
}

#[test]
fn note_level_dynamics_emitted_as_direction() {
    use crate::ir::articulation::DynamicMark;

    let mut note = Note::new(Pitch::new(PitchStep::C, 4), Duration::quarter());
    note.dynamics.push(DynamicMark {
        sign: "ff".to_string(),
        placement: Placement::Below,
    });
    let xml = emit_single_note(note);

    assert!(
        xml.contains("<dynamics>"),
        "note-level dynamics should emit <direction> with <dynamics>: {xml}"
    );
    assert!(xml.contains("<ff/>"), "expected <ff/> in output: {xml}");
}

#[test]
fn note_level_wedge_emitted_as_direction() {
    use crate::ir::articulation::Wedge;

    let mut note = Note::new(Pitch::new(PitchStep::D, 4), Duration::quarter());
    note.wedges.push(Wedge {
        wedge_type: "crescendo".to_string(),
        placement: Placement::Below,
    });
    let xml = emit_single_note(note);

    assert!(
        xml.contains("<wedge type=\"crescendo\""),
        "note-level wedge should emit <direction> with <wedge>: {xml}"
    );
}

#[test]
fn triplet_divisions_auto_computed() {
    let mut note = Note::new(Pitch::new(PitchStep::C, 4), Duration::eighth());
    note.duration.tuplet_actual = 3;
    note.duration.tuplet_normal = 2;

    let voice = Voice {
        number: 1,
        elements: vec![VoiceElement::Note(Box::new(note))],
    };
    let mut measure = make_empty_measure();
    measure.attributes = Some(MeasureAttributes::default());
    measure.voices = vec![voice];
    let mut part = Part::new("P1");
    part.measures.push(measure);
    let mut score = Score::new();
    score.children.push(ScoreChild::Part(part));

    let adapter = IrToMxmlAdapter::new();
    let xml = adapter.convert(&score).unwrap();

    // With divisions=12 (lcm(4,3)), triplet eighth = duration 4
    assert!(
        xml.contains("<divisions>12</divisions>"),
        "divisions should be 12 for triplets: {xml}"
    );
    assert!(
        xml.contains("<duration>4</duration>"),
        "triplet eighth with divisions=12 should be duration 4: {xml}"
    );
}

// ── test helpers ──────────────────────────────────────────────────────

fn emit_single_note(note: Note) -> String {
    let mut measure = make_empty_measure();
    let voice = Voice {
        number: 1,
        elements: vec![VoiceElement::Note(Box::new(note))],
    };
    measure.voices = vec![voice];
    emit_measure(measure)
}

fn emit_chord(chord: Chord) -> String {
    let mut measure = make_empty_measure();
    let voice = Voice {
        number: 1,
        elements: vec![VoiceElement::Chord(chord)],
    };
    measure.voices = vec![voice];
    emit_measure(measure)
}

fn emit_measure_with_harmony(harmony: crate::ir::harmony::Harmony) -> String {
    let mut measure = make_empty_measure();
    measure.harmonies = vec![harmony];
    emit_measure(measure)
}

fn emit_measure_with_figured_bass(fb: crate::ir::harmony::FiguredBass) -> String {
    let mut measure = make_empty_measure();
    measure.figured_bass = vec![fb];
    emit_measure(measure)
}

fn emit_direction(dir: crate::ir::direction::Direction) -> String {
    let mut measure = make_empty_measure();
    measure.directions = vec![dir];
    emit_measure(measure)
}

fn emit_measure(measure: crate::ir::measure::Measure) -> String {
    let mut part = Part::new("P1");
    part.measures.push(measure);
    let mut score = Score::new();
    score.children.push(ScoreChild::Part(part));
    let adapter = IrToMxmlAdapter::new();
    adapter.convert(&score).unwrap()
}

fn make_empty_measure() -> crate::ir::measure::Measure {
    crate::ir::measure::Measure {
        number: 1,
        implicit: false,
        width: None,
        attributes: None,
        left_barline: None,
        right_barline: None,
        directions: vec![],
        harmonies: vec![],
        figured_bass: vec![],
        print_object: true,
        multi_measure_rest: None,
        voices: vec![],
    }
}

#[test]
fn tremolo_single_note_emission() {
    let mut note = Note::new(Pitch::new(PitchStep::C, 4), Duration::quarter());
    note.tremolo_marks = 3;
    note.ornaments.push(crate::ir::articulation::Ornament {
        name: "tremolo".to_string(),
        placement: Placement::Unspecified,
    });
    let voice = Voice {
        number: 1,
        elements: vec![VoiceElement::Note(Box::new(note))],
    };
    let mut measure = make_empty_measure();
    measure.voices.push(voice);
    let xml = emit_measure(measure);
    assert!(
        xml.contains("<tremolo type=\"single\">3</tremolo>"),
        "should emit tremolo with type and marks: {xml}"
    );
}

#[test]
fn tremolo_two_note_emission() {
    let mut n1 = Note::new(Pitch::new(PitchStep::C, 4), Duration::quarter());
    n1.tremolo_marks = 2;
    n1.two_note_tremolo = true;
    n1.tremolo_start = true;
    n1.ornaments.push(crate::ir::articulation::Ornament {
        name: "tremolo".to_string(),
        placement: Placement::Unspecified,
    });
    let mut n2 = Note::new(Pitch::new(PitchStep::E, 4), Duration::quarter());
    n2.tremolo_marks = 2;
    n2.two_note_tremolo = true;
    n2.tremolo_start = false;
    n2.ornaments.push(crate::ir::articulation::Ornament {
        name: "tremolo".to_string(),
        placement: Placement::Unspecified,
    });
    let voice = Voice {
        number: 1,
        elements: vec![
            VoiceElement::Note(Box::new(n1)),
            VoiceElement::Note(Box::new(n2)),
        ],
    };
    let mut measure = make_empty_measure();
    measure.voices.push(voice);
    let xml = emit_measure(measure);
    assert!(
        xml.contains("<tremolo type=\"start\">2</tremolo>"),
        "first note should have tremolo start: {xml}"
    );
    assert!(
        xml.contains("<tremolo type=\"stop\">2</tremolo>"),
        "second note should have tremolo stop: {xml}"
    );
}

#[test]
fn layout_break_emission() {
    let mut measure = make_empty_measure();
    measure.directions.push(Direction {
        layout_break: Some(crate::ir::direction::LayoutBreakType::Page),
        ..Default::default()
    });
    let xml = emit_measure(measure);
    assert!(
        xml.contains("<print new-page=\"yes\""),
        "should emit page break as <print>: {xml}"
    );
    assert!(
        !xml.contains("<direction>"),
        "layout-break-only directions should not emit <direction>: {xml}"
    );
}

#[test]
fn system_break_emission() {
    let mut measure = make_empty_measure();
    measure.directions.push(Direction {
        layout_break: Some(crate::ir::direction::LayoutBreakType::System),
        ..Default::default()
    });
    let xml = emit_measure(measure);
    assert!(
        xml.contains("<print new-system=\"yes\""),
        "should emit system break: {xml}"
    );
}

#[test]
fn staff_lines_emission() {
    let mut measure = make_empty_measure();
    measure.attributes = Some(crate::ir::measure::MeasureAttributes {
        staff_lines: Some(1),
        ..Default::default()
    });
    let xml = emit_measure(measure);
    assert!(
        xml.contains("<staff-details>"),
        "should emit staff-details: {xml}"
    );
    assert!(
        xml.contains("<staff-lines>1</staff-lines>"),
        "should emit staff-lines: {xml}"
    );
}

#[test]
fn staff_lines_5_not_emitted() {
    let mut measure = make_empty_measure();
    measure.attributes = Some(crate::ir::measure::MeasureAttributes {
        staff_lines: Some(5),
        ..Default::default()
    });
    let xml = emit_measure(measure);
    assert!(
        !xml.contains("<staff-details>"),
        "standard 5-line staff should not emit staff-details: {xml}"
    );
}

#[test]
fn multi_measure_rest_emission() {
    let mut measure = make_empty_measure();
    measure.attributes = Some(crate::ir::measure::MeasureAttributes::default());
    measure.multi_measure_rest = Some(4);
    let xml = emit_measure(measure);
    assert!(
        xml.contains("<measure-style>"),
        "should emit measure-style: {xml}"
    );
    assert!(
        xml.contains("<multiple-rest>4</multiple-rest>"),
        "should emit multiple-rest count: {xml}"
    );
}

#[test]
fn sound_tempo_with_metronome() {
    let mut measure = make_empty_measure();
    measure.directions.push(Direction {
        tempo: Some(crate::ir::direction::TempoDirection {
            text: None,
            beat_unit: Some("quarter".to_string()),
            per_minute: Some(120.0),
            dots: 0,
            placement: Placement::Above,
        }),
        ..Default::default()
    });
    let xml = emit_measure(measure);
    assert!(xml.contains("<metronome>"), "should emit metronome: {xml}");
    assert!(
        xml.contains("<sound tempo=\"120\""),
        "should also emit sound tempo: {xml}"
    );
}

#[test]
fn wavy_line_emission() {
    let mut note = Note::new(Pitch::new(PitchStep::D, 5), Duration::half());
    note.ornaments.push(crate::ir::articulation::Ornament {
        name: "trill-mark".to_string(),
        placement: Placement::Above,
    });
    note.ornaments.push(crate::ir::articulation::Ornament {
        name: "wavy-line-start".to_string(),
        placement: Placement::Above,
    });
    let voice = Voice {
        number: 1,
        elements: vec![VoiceElement::Note(Box::new(note))],
    };
    let mut measure = make_empty_measure();
    measure.voices.push(voice);
    let xml = emit_measure(measure);
    assert!(
        xml.contains("<trill-mark/>"),
        "should emit trill-mark: {xml}"
    );
    assert!(
        xml.contains("<wavy-line type=\"start\""),
        "should emit wavy-line with type: {xml}"
    );
}

// ── Phase 5: tests for newly-added emission features ─────────────────

#[test]
fn grace_slash_attribute() {
    let mut note = Note::new(
        Pitch::new(PitchStep::C, 4),
        Duration::new(num::rational::Ratio::new(1, 16)),
    );
    note.is_grace = true;
    note.grace_slash = true;
    let voice = Voice {
        number: 1,
        elements: vec![VoiceElement::Note(Box::new(note))],
    };
    let mut measure = make_empty_measure();
    measure.voices.push(voice);
    let xml = emit_measure(measure);
    assert!(
        xml.contains("slash=\"yes\""),
        "should emit slash=yes on grace: {xml}"
    );
}

#[test]
fn print_object_no() {
    let mut note = Note::new(Pitch::new(PitchStep::C, 4), Duration::quarter());
    note.print_object = false;
    let voice = Voice {
        number: 1,
        elements: vec![VoiceElement::Note(Box::new(note))],
    };
    let mut measure = make_empty_measure();
    measure.voices.push(voice);
    let xml = emit_measure(measure);
    assert!(
        xml.contains("print-object=\"no\""),
        "should emit print-object=no: {xml}"
    );
}

#[test]
fn notehead_emission() {
    let mut note = Note::new(Pitch::new(PitchStep::C, 4), Duration::quarter());
    note.notehead = "x".to_string();
    let voice = Voice {
        number: 1,
        elements: vec![VoiceElement::Note(Box::new(note))],
    };
    let mut measure = make_empty_measure();
    measure.voices.push(voice);
    let xml = emit_measure(measure);
    assert!(
        xml.contains("<notehead>x</notehead>"),
        "should emit notehead: {xml}"
    );
}

#[test]
fn measure_width_attribute() {
    let mut measure = make_empty_measure();
    measure.width = Some(120.5);
    let xml = emit_measure(measure);
    assert!(xml.contains("width=\"120.5\""), "should emit width: {xml}");
}

#[test]
fn text_direction_font_attrs() {
    let dir = Direction {
        text: Some(crate::ir::direction::TextDirection {
            text: "pizz.".to_string(),
            placement: Placement::Above,
            font_style: Some("italic".to_string()),
            font_weight: Some("bold".to_string()),
        }),
        ..Direction::default()
    };
    let xml = emit_direction(dir);
    assert!(
        xml.contains("font-style=\"italic\""),
        "should emit font-style: {xml}"
    );
    assert!(
        xml.contains("font-weight=\"bold\""),
        "should emit font-weight: {xml}"
    );
}

#[test]
fn pedal_line_attribute() {
    let dir = Direction {
        pedal: Some(crate::ir::direction::PedalEvent {
            pedal_type: "start".to_string(),
            line: true,
        }),
        ..Direction::default()
    };
    let xml = emit_direction(dir);
    assert!(xml.contains("line=\"yes\""), "should emit line=yes: {xml}");
}

#[test]
fn lyric_elision() {
    let mut note = Note::new(Pitch::new(PitchStep::C, 4), Duration::quarter());
    note.lyrics.push(crate::ir::articulation::LyricSyllable {
        text: "la".to_string(),
        syllabic: crate::ir::articulation::SyllabicType::Single,
        number: 1,
        extend: false,
        elision: true,
    });
    let voice = Voice {
        number: 1,
        elements: vec![VoiceElement::Note(Box::new(note))],
    };
    let mut measure = make_empty_measure();
    measure.voices.push(voice);
    let xml = emit_measure(measure);
    assert!(xml.contains("<elision/>"), "should emit elision: {xml}");
}

#[test]
fn midi_instrument_in_score_part() {
    let part = Part {
        name: "Cello".to_string(),
        abbreviation: "Vc.".to_string(),
        part_id: "P1".to_string(),
        midi_instrument: "Cello".to_string(),
        midi_channel: 1,
        midi_program: 43,
        staves: 1,
        measures: vec![make_empty_measure()],
    };
    let mut score = Score::new();
    score.children.push(ScoreChild::Part(part));
    let adapter = IrToMxmlAdapter::new();
    let xml = adapter.convert(&score).unwrap();
    assert!(
        xml.contains("<midi-instrument"),
        "should emit midi-instrument: {xml}"
    );
    assert!(
        xml.contains("<midi-channel>1</midi-channel>"),
        "should emit midi-channel: {xml}"
    );
    assert!(
        xml.contains("<midi-program>43</midi-program>"),
        "should emit midi-program: {xml}"
    );
    assert!(
        xml.contains("<midi-name>Cello</midi-name>"),
        "should emit midi-name: {xml}"
    );
    assert!(
        xml.contains("<score-instrument"),
        "should emit score-instrument: {xml}"
    );
}

#[test]
fn subtitle_credit() {
    let mut score = Score::new();
    score.metadata.subtitle = Some("Op. 1".to_string());
    score.children.push(ScoreChild::Part(Part::new("P1")));
    let adapter = IrToMxmlAdapter::new();
    let xml = adapter.convert(&score).unwrap();
    assert!(xml.contains("<credit>"), "should emit credit: {xml}");
    assert!(
        xml.contains("<credit-type>subtitle</credit-type>"),
        "should emit credit-type: {xml}"
    );
    assert!(xml.contains("Op. 1"), "should contain subtitle text: {xml}");
}

#[test]
fn extra_creators() {
    let mut score = Score::new();
    score
        .metadata
        .extra
        .insert("editor".to_string(), "John".to_string());
    score.children.push(ScoreChild::Part(Part::new("P1")));
    let adapter = IrToMxmlAdapter::new();
    let xml = adapter.convert(&score).unwrap();
    assert!(
        xml.contains("type=\"editor\""),
        "should emit extra creator type: {xml}"
    );
    assert!(
        xml.contains("John"),
        "should emit extra creator value: {xml}"
    );
}

#[test]
fn part_group_number_preserved() {
    let mut group = crate::ir::score::PartGroup::new("StaffGroup");
    group.number = 3;
    group.children.push(ScoreChild::Part(Part::new("P1")));
    let mut score = Score::new();
    score.children.push(ScoreChild::PartGroup(group));
    let adapter = IrToMxmlAdapter::new();
    let xml = adapter.convert(&score).unwrap();
    assert!(
        xml.contains("number=\"3\""),
        "should preserve group number: {xml}"
    );
}

#[test]
fn tempo_text_as_words() {
    let dir = Direction {
        tempo: Some(TempoDirection {
            text: Some("Allegro".to_string()),
            beat_unit: Some("quarter".to_string()),
            per_minute: Some(120.0),
            dots: 0,
            placement: Placement::Above,
        }),
        ..Direction::default()
    };
    let xml = emit_direction(dir);
    assert!(
        xml.contains("<words>Allegro</words>"),
        "should emit tempo text as words: {xml}"
    );
    assert!(xml.contains("<metronome>"), "should emit metronome: {xml}");
    // Words should come before metronome
    let words_pos = xml.find("<words>Allegro</words>").unwrap();
    let metro_pos = xml.find("<metronome>").unwrap();
    assert!(
        words_pos < metro_pos,
        "words should precede metronome: {xml}"
    );
}

#[test]
fn sound_element_merged() {
    let dir = Direction {
        tempo: Some(TempoDirection {
            text: None,
            beat_unit: Some("quarter".to_string()),
            per_minute: Some(120.0),
            dots: 0,
            placement: Placement::Above,
        }),
        da_capo: Some("D.C.".to_string()),
        ..Direction::default()
    };
    let xml = emit_direction(dir);
    // Should have a single <sound with both tempo and dacapo
    assert!(
        xml.contains("tempo=\"120\""),
        "should emit tempo in sound: {xml}"
    );
    assert!(
        xml.contains("dacapo=\"yes\""),
        "should emit dacapo in sound: {xml}"
    );
    // Count <sound occurrences — should be exactly 1
    let sound_count = xml.matches("<sound ").count();
    assert_eq!(
        sound_count, 1,
        "should merge into single sound element: {xml}"
    );
}

#[test]
fn test_music_to_mxml_round_trip() {
    use crate::adapters::FromMusicAdapter;
    use crate::ir::annotation::Annotation;
    use crate::ir::music::{ContextType, Music, MusicDocument};
    use crate::ir::pitch::{Pitch, PitchStep};

    let music = Music::Sequential(vec![
        Music::TimeSignature(TimeSignature {
            beats: "4".to_string(),
            beat_type: 4,
            symbol: None,
        }),
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
        Music::Note {
            pitch: Pitch::new(PitchStep::F, 4),
            duration: Duration::quarter(),
            annotations: vec![],
        },
    ])
    .in_context(ContextType::Staff, None);

    let doc = MusicDocument::new(music);

    let adapter = IrToMxmlAdapter::new();
    let xml = adapter.convert_music(&doc).unwrap();

    assert!(xml.contains("<note"), "should contain notes");
    assert!(xml.contains("<time>"), "should contain time signature");
    assert!(xml.contains("<step>C</step>"), "should contain C note");
}
