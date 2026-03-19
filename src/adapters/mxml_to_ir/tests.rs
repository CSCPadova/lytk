use super::*;

use crate::ir::articulation::StartStop;
use crate::ir::note::VoiceElement;
use crate::ir::pitch::{AccidentalDisplay, Alter, PitchStep};
use num::rational::Ratio;

#[test]
fn test_parse_minimal_score() {
    let xml = r#"<?xml version="1.0"?>
<score-partwise>
  <part-list>
<score-part id="P1">
  <part-name>Piano</part-name>
</score-part>
  </part-list>
  <part id="P1">
<measure number="1">
  <attributes>
    <divisions>1</divisions>
    <key><fifths>0</fifths><mode>major</mode></key>
    <time><beats>4</beats><beat-type>4</beat-type></time>
    <clef><sign>G</sign><line>2</line></clef>
  </attributes>
  <note>
    <pitch><step>C</step><octave>4</octave></pitch>
    <duration>1</duration>
    <voice>1</voice>
    <type>quarter</type>
  </note>
</measure>
  </part>
</score-partwise>"#;

    let adapter = MxmlToIrAdapter::new();
    let score = adapter.convert_str(xml).unwrap();

    assert_eq!(score.parts().len(), 1);
    let part = &score.parts()[0];
    assert_eq!(part.name, "Piano");
    assert_eq!(part.measures.len(), 1);

    let measure = &part.measures[0];
    assert_eq!(measure.number, 1);
    assert!(measure.attributes.is_some());

    let attrs = measure.attributes.as_ref().unwrap();
    assert_eq!(attrs.divisions, 1);
    assert!(attrs.key.is_some());
    assert!(attrs.time.is_some());
    assert_eq!(attrs.time.as_ref().unwrap().beats, "4");
    assert_eq!(attrs.time.as_ref().unwrap().beat_type, 4);

    // One voice with one note
    assert_eq!(measure.voices.len(), 1);
    assert_eq!(measure.voices[0].elements.len(), 1);
    match &measure.voices[0].elements[0] {
        VoiceElement::Note(n) => {
            assert_eq!(n.pitch.step, PitchStep::C);
            assert_eq!(n.pitch.octave, 4);
            assert_eq!(n.voice, 1);
        }
        other => panic!("Expected Note, got {:?}", other),
    }
}

#[test]
fn test_parse_chord() {
    let xml = r#"<?xml version="1.0"?>
<score-partwise>
  <part-list>
<score-part id="P1"><part-name>P</part-name></score-part>
  </part-list>
  <part id="P1">
<measure number="1">
  <attributes><divisions>1</divisions></attributes>
  <note>
    <pitch><step>C</step><octave>4</octave></pitch>
    <duration>1</duration><voice>1</voice><type>quarter</type>
  </note>
  <note>
    <chord/>
    <pitch><step>E</step><octave>4</octave></pitch>
    <duration>1</duration><voice>1</voice><type>quarter</type>
  </note>
  <note>
    <chord/>
    <pitch><step>G</step><octave>4</octave></pitch>
    <duration>1</duration><voice>1</voice><type>quarter</type>
  </note>
</measure>
  </part>
</score-partwise>"#;

    let score = MxmlToIrAdapter::new().convert_str(xml).unwrap();
    let measure = &score.parts()[0].measures[0];
    assert_eq!(measure.voices.len(), 1);
    assert_eq!(measure.voices[0].elements.len(), 1);

    match &measure.voices[0].elements[0] {
        VoiceElement::Chord(chord) => {
            assert_eq!(chord.notes.len(), 3);
            assert_eq!(chord.notes[0].pitch.step, PitchStep::C);
            assert_eq!(chord.notes[1].pitch.step, PitchStep::E);
            assert_eq!(chord.notes[2].pitch.step, PitchStep::G);
        }
        other => panic!("Expected Chord, got {:?}", other),
    }
}

#[test]
fn test_parse_rest() {
    let xml = r#"<?xml version="1.0"?>
<score-partwise>
  <part-list>
<score-part id="P1"><part-name>P</part-name></score-part>
  </part-list>
  <part id="P1">
<measure number="1">
  <attributes><divisions>1</divisions></attributes>
  <note>
    <rest measure="yes"/>
    <duration>4</duration><voice>1</voice><type>whole</type>
  </note>
</measure>
  </part>
</score-partwise>"#;

    let score = MxmlToIrAdapter::new().convert_str(xml).unwrap();
    let measure = &score.parts()[0].measures[0];
    match &measure.voices[0].elements[0] {
        VoiceElement::Rest(r) => {
            assert!(r.is_measure_rest);
        }
        other => panic!("Expected Rest, got {:?}", other),
    }
}

#[test]
fn test_parse_accidentals() {
    let xml = r#"<?xml version="1.0"?>
<score-partwise>
  <part-list>
<score-part id="P1"><part-name>P</part-name></score-part>
  </part-list>
  <part id="P1">
<measure number="1">
  <attributes><divisions>1</divisions></attributes>
  <note>
    <pitch><step>F</step><alter>1</alter><octave>4</octave></pitch>
    <duration>1</duration><voice>1</voice><type>quarter</type>
    <accidental>sharp</accidental>
  </note>
  <note>
    <pitch><step>B</step><alter>-1</alter><octave>3</octave></pitch>
    <duration>1</duration><voice>1</voice><type>quarter</type>
    <accidental cautionary="yes">flat</accidental>
  </note>
</measure>
  </part>
</score-partwise>"#;

    let score = MxmlToIrAdapter::new().convert_str(xml).unwrap();
    let elems = &score.parts()[0].measures[0].voices[0].elements;

    match &elems[0] {
        VoiceElement::Note(n) => {
            assert_eq!(n.pitch.step, PitchStep::F);
            assert_eq!(n.pitch.alter, Alter::from_integer(1));
            assert_eq!(n.pitch.accidental, AccidentalDisplay::Forced);
        }
        other => panic!("Expected Note, got {:?}", other),
    }
    match &elems[1] {
        VoiceElement::Note(n) => {
            assert_eq!(n.pitch.step, PitchStep::B);
            assert_eq!(n.pitch.alter, Alter::from_integer(-1));
            assert_eq!(n.pitch.accidental, AccidentalDisplay::Cautionary);
        }
        other => panic!("Expected Note, got {:?}", other),
    }
}

#[test]
fn test_parse_multivoice() {
    let xml = r#"<?xml version="1.0"?>
<score-partwise>
  <part-list>
<score-part id="P1"><part-name>P</part-name></score-part>
  </part-list>
  <part id="P1">
<measure number="1">
  <attributes><divisions>1</divisions></attributes>
  <note>
    <pitch><step>C</step><octave>5</octave></pitch>
    <duration>4</duration><voice>1</voice><type>whole</type>
  </note>
  <backup><duration>4</duration></backup>
  <note>
    <pitch><step>E</step><octave>3</octave></pitch>
    <duration>4</duration><voice>2</voice><type>whole</type>
  </note>
</measure>
  </part>
</score-partwise>"#;

    let score = MxmlToIrAdapter::new().convert_str(xml).unwrap();
    let measure = &score.parts()[0].measures[0];

    // Voice 0 gets the backup, voices 1 and 2 get notes.
    let voice_numbers: Vec<u8> = measure.voices.iter().map(|v| v.number).collect();
    assert!(voice_numbers.contains(&1));
    assert!(voice_numbers.contains(&2));

    // Each voice has exactly one note.
    for voice in &measure.voices {
        if voice.number == 1 || voice.number == 2 {
            assert_eq!(voice.elements.len(), 1);
        }
    }
}

#[test]
fn test_parse_directions() {
    let xml = r#"<?xml version="1.0"?>
<score-partwise>
  <part-list>
<score-part id="P1"><part-name>P</part-name></score-part>
  </part-list>
  <part id="P1">
<measure number="1">
  <attributes><divisions>1</divisions></attributes>
  <direction>
    <direction-type>
      <dynamics><f/></dynamics>
    </direction-type>
  </direction>
  <direction>
    <direction-type>
      <metronome>
        <beat-unit>quarter</beat-unit>
        <per-minute>120</per-minute>
      </metronome>
    </direction-type>
  </direction>
  <note>
    <pitch><step>C</step><octave>4</octave></pitch>
    <duration>4</duration><voice>1</voice><type>whole</type>
  </note>
</measure>
  </part>
</score-partwise>"#;

    let score = MxmlToIrAdapter::new().convert_str(xml).unwrap();
    let measure = &score.parts()[0].measures[0];

    assert_eq!(measure.directions.len(), 2);

    // First direction: dynamics f
    let dyn_mark = measure.directions[0].dynamic.as_ref().unwrap();
    assert_eq!(dyn_mark.sign, "f");

    // Second direction: tempo
    assert!(measure.directions[1].tempo.is_some());
    let tempo = measure.directions[1].tempo.as_ref().unwrap();
    assert_eq!(tempo.beat_unit.as_deref(), Some("quarter"));
    assert_eq!(tempo.per_minute, Some(120.0));
}

#[test]
fn test_parse_all_fixtures() {
    // Parse all 143 MusicXML fixture files. If any fail to parse, the
    // file name is collected and reported.
    let fixture_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("xml");
    let adapter = MxmlToIrAdapter::new();

    let mut failures: Vec<(String, String)> = Vec::new();
    let mut success_count = 0;

    for entry in std::fs::read_dir(&fixture_dir).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();
        if path.extension().is_some_and(|e| e == "xml") {
            let xml = std::fs::read_to_string(&path).unwrap();
            match adapter.convert_str(&xml) {
                Ok(score) => {
                    // Sanity: the score should have at least one part.
                    assert!(!score.parts().is_empty(), "no parts in {}", path.display());
                    success_count += 1;
                }
                Err(e) => {
                    failures.push((
                        path.file_name().unwrap().to_string_lossy().to_string(),
                        e.to_string(),
                    ));
                }
            }
        }
    }

    if !failures.is_empty() {
        let report: Vec<String> = failures
            .iter()
            .map(|(f, e)| format!("  {f}: {e}"))
            .collect();
        panic!(
            "{} of {} fixture files failed to parse:\n{}",
            failures.len(),
            success_count + failures.len(),
            report.join("\n")
        );
    }

    assert!(
        success_count >= 100,
        "expected at least 100 fixtures, got {success_count}"
    );
}

#[test]
fn test_parse_credit_metadata() {
    let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE score-partwise PUBLIC "-//Recordare//DTD MusicXML 4.0 Partwise//EN" "http://www.musicxml.org/dtds/partwise.dtd">
<score-partwise version="4.0">
  <credit page="1">
<credit-type>title</credit-type>
<credit-words>My Title</credit-words>
  </credit>
  <credit page="1">
<credit-type>composer</credit-type>
<credit-words>A. Composer</credit-words>
  </credit>
  <part-list>
<score-part id="P1"><part-name>Piano</part-name></score-part>
  </part-list>
  <part id="P1">
<measure number="1">
  <attributes>
    <divisions>1</divisions>
    <time><beats>4</beats><beat-type>4</beat-type></time>
  </attributes>
  <note>
    <pitch><step>C</step><octave>4</octave></pitch>
    <duration>4</duration>
    <type>whole</type>
  </note>
</measure>
  </part>
</score-partwise>"#;

    let adapter = MxmlToIrAdapter::new();
    let score = adapter.convert_str(xml).unwrap();
    assert_eq!(score.metadata.title.as_deref(), Some("My Title"));
    assert_eq!(score.metadata.composer.as_deref(), Some("A. Composer"));
}

#[test]
fn test_parse_grace_slash() {
    let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE score-partwise PUBLIC "-//Recordare//DTD MusicXML 4.0 Partwise//EN" "http://www.musicxml.org/dtds/partwise.dtd">
<score-partwise version="4.0">
  <part-list>
<score-part id="P1"><part-name>Piano</part-name></score-part>
  </part-list>
  <part id="P1">
<measure number="1">
  <attributes>
    <divisions>1</divisions>
    <time><beats>4</beats><beat-type>4</beat-type></time>
  </attributes>
  <note>
    <grace slash="yes"/>
    <pitch><step>E</step><octave>5</octave></pitch>
    <type>16th</type>
  </note>
  <note>
    <pitch><step>C</step><octave>4</octave></pitch>
    <duration>4</duration>
    <type>whole</type>
  </note>
</measure>
  </part>
</score-partwise>"#;

    let adapter = MxmlToIrAdapter::new();
    let score = adapter.convert_str(xml).unwrap();
    let elems: Vec<_> = score.parts()[0]
        .measures
        .iter()
        .flat_map(|m| &m.voices)
        .flat_map(|v| &v.elements)
        .collect();
    match &elems[0] {
        VoiceElement::Note(n) => {
            assert!(n.is_grace, "should be grace note");
            assert!(n.grace_slash, "slash=yes should set grace_slash");
        }
        _ => panic!("expected Note"),
    }
}

#[test]
fn test_parse_harmony() {
    let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE score-partwise PUBLIC "-//Recordare//DTD MusicXML 4.0 Partwise//EN" "http://www.musicxml.org/dtds/partwise.dtd">
<score-partwise version="4.0">
  <part-list>
<score-part id="P1"><part-name>Piano</part-name></score-part>
  </part-list>
  <part id="P1">
<measure number="1">
  <attributes>
    <divisions>1</divisions>
    <time><beats>4</beats><beat-type>4</beat-type></time>
  </attributes>
  <harmony>
    <root><root-step>C</root-step></root>
    <kind>major</kind>
    <bass><bass-step>E</bass-step></bass>
  </harmony>
  <note>
    <pitch><step>C</step><octave>4</octave></pitch>
    <duration>4</duration>
    <type>whole</type>
  </note>
</measure>
  </part>
</score-partwise>"#;

    let adapter = MxmlToIrAdapter::new();
    let score = adapter.convert_str(xml).unwrap();
    let m = &score.parts()[0].measures[0];
    assert_eq!(m.harmonies.len(), 1);
    assert_eq!(m.harmonies[0].root.step, "C");
    assert_eq!(m.harmonies[0].kind, "major");
    assert_eq!(m.harmonies[0].bass.as_ref().unwrap().step, "E");
}

#[test]
fn test_parse_figured_bass() {
    let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE score-partwise PUBLIC "-//Recordare//DTD MusicXML 4.0 Partwise//EN" "http://www.musicxml.org/dtds/partwise.dtd">
<score-partwise version="4.0">
  <part-list>
<score-part id="P1"><part-name>Bass</part-name></score-part>
  </part-list>
  <part id="P1">
<measure number="1">
  <attributes>
    <divisions>1</divisions>
    <time><beats>4</beats><beat-type>4</beat-type></time>
  </attributes>
  <figured-bass>
    <figure><figure-number>6</figure-number></figure>
    <figure><figure-number>4</figure-number></figure>
    <duration>4</duration>
  </figured-bass>
  <note>
    <pitch><step>C</step><octave>3</octave></pitch>
    <duration>4</duration>
    <type>whole</type>
  </note>
</measure>
  </part>
</score-partwise>"#;

    let adapter = MxmlToIrAdapter::new();
    let score = adapter.convert_str(xml).unwrap();
    let m = &score.parts()[0].measures[0];
    assert_eq!(m.figured_bass.len(), 1);
    assert_eq!(m.figured_bass[0].figures.len(), 2);
    assert_eq!(m.figured_bass[0].figures[0].number, Some(6));
    assert_eq!(m.figured_bass[0].figures[1].number, Some(4));
}

#[test]
fn test_parse_glissando() {
    let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE score-partwise PUBLIC "-//Recordare//DTD MusicXML 4.0 Partwise//EN" "http://www.musicxml.org/dtds/partwise.dtd">
<score-partwise version="4.0">
  <part-list>
<score-part id="P1"><part-name>Piano</part-name></score-part>
  </part-list>
  <part id="P1">
<measure number="1">
  <attributes>
    <divisions>1</divisions>
    <time><beats>4</beats><beat-type>4</beat-type></time>
  </attributes>
  <note>
    <pitch><step>C</step><octave>4</octave></pitch>
    <duration>2</duration>
    <type>half</type>
    <notations>
      <glissando type="start" line-type="dashed">gliss.</glissando>
    </notations>
  </note>
  <note>
    <pitch><step>E</step><octave>4</octave></pitch>
    <duration>2</duration>
    <type>half</type>
    <notations>
      <glissando type="stop"/>
    </notations>
  </note>
</measure>
  </part>
</score-partwise>"#;

    let adapter = MxmlToIrAdapter::new();
    let score = adapter.convert_str(xml).unwrap();
    let elems: Vec<_> = score.parts()[0]
        .measures
        .iter()
        .flat_map(|m| &m.voices)
        .flat_map(|v| &v.elements)
        .collect();
    match &elems[0] {
        VoiceElement::Note(n) => {
            assert_eq!(n.glissando, Some(StartStop::Start));
            assert_eq!(n.glissando_line_type.as_deref(), Some("dashed"));
        }
        _ => panic!("expected Note"),
    }
    match &elems[1] {
        VoiceElement::Note(n) => {
            assert_eq!(n.glissando, Some(StartStop::Stop));
        }
        _ => panic!("expected Note"),
    }
}

#[test]
fn test_parse_coda_segno() {
    let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE score-partwise PUBLIC "-//Recordare//DTD MusicXML 4.0 Partwise//EN" "http://www.musicxml.org/dtds/partwise.dtd">
<score-partwise version="4.0">
  <part-list>
<score-part id="P1"><part-name>Piano</part-name></score-part>
  </part-list>
  <part id="P1">
<measure number="1">
  <attributes>
    <divisions>1</divisions>
    <time><beats>4</beats><beat-type>4</beat-type></time>
  </attributes>
  <direction>
    <direction-type><coda/></direction-type>
  </direction>
  <note>
    <pitch><step>C</step><octave>4</octave></pitch>
    <duration>4</duration>
    <type>whole</type>
  </note>
</measure>
<measure number="2">
  <direction>
    <direction-type><segno/></direction-type>
    <sound dalsegno="D.S. al Coda"/>
  </direction>
  <note>
    <pitch><step>D</step><octave>4</octave></pitch>
    <duration>4</duration>
    <type>whole</type>
  </note>
</measure>
  </part>
</score-partwise>"#;

    let adapter = MxmlToIrAdapter::new();
    let score = adapter.convert_str(xml).unwrap();
    let m1 = &score.parts()[0].measures[0];
    assert!(
        m1.directions.iter().any(|d| d.coda),
        "measure 1 should have coda"
    );
    let m2 = &score.parts()[0].measures[1];
    assert!(
        m2.directions.iter().any(|d| d.segno),
        "measure 2 should have segno"
    );
    assert!(
        m2.directions.iter().any(|d| d.dal_segno.is_some()),
        "measure 2 should have dal segno"
    );
}

#[test]
fn test_parse_defaults_page_layout() {
    let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE score-partwise PUBLIC "-//Recordare//DTD MusicXML 4.0 Partwise//EN" "http://www.musicxml.org/dtds/partwise.dtd">
<score-partwise version="4.0">
  <defaults>
<scaling>
  <millimeters>7.05556</millimeters>
  <tenths>40</tenths>
</scaling>
<page-layout>
  <page-height>1683.36</page-height>
  <page-width>1190.88</page-width>
  <page-margins type="both">
    <left-margin>56.6929</left-margin>
    <right-margin>56.6929</right-margin>
    <top-margin>56.6929</top-margin>
    <bottom-margin>113.386</bottom-margin>
  </page-margins>
</page-layout>
  </defaults>
  <part-list>
<score-part id="P1"><part-name>Piano</part-name></score-part>
  </part-list>
  <part id="P1">
<measure number="1">
  <attributes>
    <divisions>1</divisions>
    <time><beats>4</beats><beat-type>4</beat-type></time>
  </attributes>
  <note>
    <pitch><step>C</step><octave>4</octave></pitch>
    <duration>4</duration>
    <type>whole</type>
  </note>
</measure>
  </part>
</score-partwise>"#;

    let adapter = MxmlToIrAdapter::new();
    let score = adapter.convert_str(xml).unwrap();
    let layout = score.page_layout.as_ref().expect("should have page layout");
    assert!(layout.staff_size.is_some(), "should have staff size");
    assert!(layout.page_height.is_some(), "should have page height");
    assert!(layout.page_width.is_some(), "should have page width");
    // 7.05556mm / 40 tenths = 0.1763889 mm/tenth
    // page_height = 1683.36 * 0.1763889 / 10 ≈ 29.7 cm
    let h = layout.page_height.unwrap();
    assert!(
        (h - 29.7).abs() < 0.1,
        "page height should be ~29.7cm, got {h}"
    );
}

#[test]
fn test_parse_anacrusis() {
    let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE score-partwise PUBLIC "-//Recordare//DTD MusicXML 4.0 Partwise//EN" "http://www.musicxml.org/dtds/partwise.dtd">
<score-partwise version="4.0">
  <part-list>
<score-part id="P1"><part-name>Piano</part-name></score-part>
  </part-list>
  <part id="P1">
<measure number="0" implicit="yes">
  <attributes>
    <divisions>1</divisions>
    <time><beats>3</beats><beat-type>4</beat-type></time>
  </attributes>
  <note>
    <pitch><step>G</step><octave>4</octave></pitch>
    <duration>1</duration>
    <type>quarter</type>
  </note>
</measure>
<measure number="1">
  <note>
    <pitch><step>C</step><octave>4</octave></pitch>
    <duration>3</duration>
    <type>half</type>
    <dot/>
  </note>
</measure>
  </part>
</score-partwise>"#;

    let adapter = MxmlToIrAdapter::new();
    let score = adapter.convert_str(xml).unwrap();
    let partial = score
        .metadata
        .partial_duration
        .as_ref()
        .expect("should detect anacrusis");
    // 1 quarter note in a 3/4 measure → partial duration = 1/4
    assert_eq!(
        partial.base,
        Ratio::new(1, 4),
        "partial should be a quarter note"
    );
}

#[test]
fn parse_single_note_tremolo() {
    let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE score-partwise PUBLIC "-//Recordare//DTD MusicXML 4.0 Partwise//EN" "http://www.musicxml.org/dtds/partwise.dtd">
<score-partwise version="4.0">
  <part-list><score-part id="P1"><part-name/></score-part></part-list>
  <part id="P1">
<measure number="1">
  <attributes><divisions>4</divisions></attributes>
  <note>
    <pitch><step>C</step><octave>4</octave></pitch>
    <duration>4</duration>
    <type>quarter</type>
    <notations><ornaments><tremolo type="single">3</tremolo></ornaments></notations>
  </note>
</measure>
  </part>
</score-partwise>"#;

    let score = MxmlToIrAdapter::new().convert_str(xml).unwrap();
    let note = &score.parts()[0].measures[0].voices[0].elements[0];
    if let crate::ir::note::VoiceElement::Note(n) = note {
        assert_eq!(n.tremolo_marks, 3);
        assert!(!n.two_note_tremolo);
    } else {
        panic!("expected Note");
    }
}

#[test]
fn parse_two_note_tremolo() {
    let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE score-partwise PUBLIC "-//Recordare//DTD MusicXML 4.0 Partwise//EN" "http://www.musicxml.org/dtds/partwise.dtd">
<score-partwise version="4.0">
  <part-list><score-part id="P1"><part-name/></score-part></part-list>
  <part id="P1">
<measure number="1">
  <attributes><divisions>4</divisions></attributes>
  <note>
    <pitch><step>C</step><octave>4</octave></pitch>
    <duration>4</duration>
    <type>quarter</type>
    <notations><ornaments><tremolo type="start">2</tremolo></ornaments></notations>
  </note>
  <note>
    <pitch><step>E</step><octave>4</octave></pitch>
    <duration>4</duration>
    <type>quarter</type>
    <notations><ornaments><tremolo type="stop">2</tremolo></ornaments></notations>
  </note>
</measure>
  </part>
</score-partwise>"#;

    let score = MxmlToIrAdapter::new().convert_str(xml).unwrap();
    let elems = &score.parts()[0].measures[0].voices[0].elements;
    if let crate::ir::note::VoiceElement::Note(n1) = &elems[0] {
        assert_eq!(n1.tremolo_marks, 2);
        assert!(n1.two_note_tremolo);
        assert!(n1.tremolo_start);
    } else {
        panic!("expected Note");
    }
    if let crate::ir::note::VoiceElement::Note(n2) = &elems[1] {
        assert_eq!(n2.tremolo_marks, 2);
        assert!(n2.two_note_tremolo);
        assert!(!n2.tremolo_start);
    } else {
        panic!("expected Note");
    }
}

#[test]
fn parse_layout_break() {
    let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE score-partwise PUBLIC "-//Recordare//DTD MusicXML 4.0 Partwise//EN" "http://www.musicxml.org/dtds/partwise.dtd">
<score-partwise version="4.0">
  <part-list><score-part id="P1"><part-name/></score-part></part-list>
  <part id="P1">
<measure number="1">
  <attributes><divisions>4</divisions></attributes>
  <print new-system="yes"/>
  <note>
    <pitch><step>C</step><octave>4</octave></pitch>
    <duration>4</duration>
    <type>quarter</type>
  </note>
</measure>
  </part>
</score-partwise>"#;

    let score = MxmlToIrAdapter::new().convert_str(xml).unwrap();
    let dirs = &score.parts()[0].measures[0].directions;
    assert!(
        dirs.iter()
            .any(|d| d.layout_break == Some(crate::ir::direction::LayoutBreakType::System)),
        "should parse system break from <print>"
    );
}

#[test]
fn parse_staff_lines() {
    let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE score-partwise PUBLIC "-//Recordare//DTD MusicXML 4.0 Partwise//EN" "http://www.musicxml.org/dtds/partwise.dtd">
<score-partwise version="4.0">
  <part-list><score-part id="P1"><part-name/></score-part></part-list>
  <part id="P1">
<measure number="1">
  <attributes>
    <divisions>4</divisions>
    <staff-details><staff-lines>1</staff-lines></staff-details>
  </attributes>
  <note>
    <pitch><step>C</step><octave>4</octave></pitch>
    <duration>4</duration>
    <type>quarter</type>
  </note>
</measure>
  </part>
</score-partwise>"#;

    let score = MxmlToIrAdapter::new().convert_str(xml).unwrap();
    let attrs = score.parts()[0].measures[0].attributes.as_ref().unwrap();
    assert_eq!(attrs.staff_lines, Some(1));
}

#[test]
fn parse_multi_measure_rest() {
    let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE score-partwise PUBLIC "-//Recordare//DTD MusicXML 4.0 Partwise//EN" "http://www.musicxml.org/dtds/partwise.dtd">
<score-partwise version="4.0">
  <part-list><score-part id="P1"><part-name/></score-part></part-list>
  <part id="P1">
<measure number="1">
  <attributes>
    <divisions>4</divisions>
    <measure-style><multiple-rest>4</multiple-rest></measure-style>
  </attributes>
  <note>
    <rest measure="yes"/>
    <duration>16</duration>
    <type>whole</type>
  </note>
</measure>
  </part>
</score-partwise>"#;

    let score = MxmlToIrAdapter::new().convert_str(xml).unwrap();
    assert_eq!(score.parts()[0].measures[0].multi_measure_rest, Some(4));
}

#[test]
fn parse_wavy_line() {
    let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE score-partwise PUBLIC "-//Recordare//DTD MusicXML 4.0 Partwise//EN" "http://www.musicxml.org/dtds/partwise.dtd">
<score-partwise version="4.0">
  <part-list><score-part id="P1"><part-name/></score-part></part-list>
  <part id="P1">
<measure number="1">
  <attributes><divisions>4</divisions></attributes>
  <note>
    <pitch><step>D</step><octave>5</octave></pitch>
    <duration>4</duration>
    <type>quarter</type>
    <notations><ornaments>
      <trill-mark/>
      <wavy-line type="start"/>
    </ornaments></notations>
  </note>
</measure>
  </part>
</score-partwise>"#;

    let score = MxmlToIrAdapter::new().convert_str(xml).unwrap();
    let note = &score.parts()[0].measures[0].voices[0].elements[0];
    if let crate::ir::note::VoiceElement::Note(n) = note {
        assert!(
            n.ornaments.iter().any(|o| o.name == "trill-mark"),
            "should have trill-mark"
        );
        assert!(
            n.ornaments.iter().any(|o| o.name == "wavy-line-start"),
            "should have wavy-line-start"
        );
    } else {
        panic!("expected Note");
    }
}

#[test]
fn tremolo_round_trip() {
    use crate::adapters::FromIrAdapter;
    let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE score-partwise PUBLIC "-//Recordare//DTD MusicXML 4.0 Partwise//EN" "http://www.musicxml.org/dtds/partwise.dtd">
<score-partwise version="4.0">
  <part-list><score-part id="P1"><part-name/></score-part></part-list>
  <part id="P1">
<measure number="1">
  <attributes><divisions>4</divisions></attributes>
  <note>
    <pitch><step>C</step><octave>4</octave></pitch>
    <duration>4</duration>
    <type>quarter</type>
    <notations><ornaments><tremolo type="single">3</tremolo></ornaments></notations>
  </note>
</measure>
  </part>
</score-partwise>"#;

    let import = MxmlToIrAdapter::new();
    let score = import.convert_str(xml).unwrap();
    let export = crate::adapters::ir_to_mxml::IrToMxmlAdapter::new();
    let out_xml = export.convert(&score).unwrap();
    assert!(
        out_xml.contains("<tremolo type=\"single\">3</tremolo>"),
        "tremolo should round-trip: {out_xml}"
    );
}

// ---------------------------------------------------------------------------
// E3T1: Extended unit tests for mxml_to_ir parsing
// ---------------------------------------------------------------------------

// --- XmlNode helper tests ---

#[test]
fn test_xml_node_attr() {
    use super::helpers::{parse_xml, XmlNode};
    let xml = r#"<root foo="bar" baz="42"><child/></root>"#;
    let node = parse_xml(xml).unwrap();
    assert_eq!(node.attr("foo"), Some("bar"));
    assert_eq!(node.attr("baz"), Some("42"));
    assert_eq!(node.attr("missing"), None);
}

#[test]
fn test_xml_node_find_and_find_all() {
    use super::helpers::parse_xml;
    let xml = r#"<root><a>1</a><b>2</b><a>3</a></root>"#;
    let node = parse_xml(xml).unwrap();
    assert_eq!(node.find("a").unwrap().text_content(), "1");
    assert_eq!(node.find_all("a").len(), 2);
    assert!(node.find("missing").is_none());
}

#[test]
fn test_xml_node_text_i64() {
    use super::helpers::parse_xml;
    let xml = r#"<root><num>42</num><bad>abc</bad></root>"#;
    let node = parse_xml(xml).unwrap();
    assert_eq!(node.find("num").unwrap().text_i64(0), 42);
    assert_eq!(node.find("bad").unwrap().text_i64(-1), -1);
}

#[test]
fn test_xml_node_child_i64_and_child_text() {
    use super::helpers::parse_xml;
    let xml = r#"<root><val>99</val><name>hello</name></root>"#;
    let node = parse_xml(xml).unwrap();
    assert_eq!(node.child_i64("val", 0), 99);
    assert_eq!(node.child_i64("missing", 5), 5);
    assert_eq!(node.child_text("name"), Some("hello"));
    assert_eq!(node.child_text("missing"), None);
}

#[test]
fn test_parse_xml_empty_text() {
    use super::helpers::parse_xml;
    let xml = r#"<root><empty></empty><ws>  </ws></root>"#;
    let node = parse_xml(xml).unwrap();
    assert_eq!(node.find("empty").unwrap().text_content(), "");
    assert_eq!(node.find("ws").unwrap().text_content(), "");
}

// --- Note parsing edge cases ---

#[test]
fn test_parse_dotted_note() {
    let xml = r#"<?xml version="1.0"?>
<score-partwise>
  <part-list><score-part id="P1"><part-name>P</part-name></score-part></part-list>
  <part id="P1">
<measure number="1">
  <attributes><divisions>4</divisions></attributes>
  <note>
    <pitch><step>C</step><octave>4</octave></pitch>
    <duration>6</duration><voice>1</voice><type>half</type><dot/>
  </note>
</measure>
  </part>
</score-partwise>"#;

    let score = MxmlToIrAdapter::new().convert_str(xml).unwrap();
    let note = match &score.parts()[0].measures[0].voices[0].elements[0] {
        VoiceElement::Note(n) => n,
        _ => panic!("expected Note"),
    };
    assert_eq!(note.duration.dots, 1);
}

#[test]
fn test_parse_tuplet_time_modification() {
    let xml = r#"<?xml version="1.0"?>
<score-partwise>
  <part-list><score-part id="P1"><part-name>P</part-name></score-part></part-list>
  <part id="P1">
<measure number="1">
  <attributes><divisions>4</divisions></attributes>
  <note>
    <pitch><step>C</step><octave>4</octave></pitch>
    <duration>2</duration><voice>1</voice><type>eighth</type>
    <time-modification><actual-notes>3</actual-notes><normal-notes>2</normal-notes></time-modification>
    <notations><tuplet type="start" bracket="yes" show-number="actual"/></notations>
  </note>
</measure>
  </part>
</score-partwise>"#;

    let score = MxmlToIrAdapter::new().convert_str(xml).unwrap();
    let note = match &score.parts()[0].measures[0].voices[0].elements[0] {
        VoiceElement::Note(n) => n,
        _ => panic!("expected Note"),
    };
    assert_eq!(note.duration.tuplet_actual, 3);
    assert_eq!(note.duration.tuplet_normal, 2);
    let tuplet = note.tuplet.as_ref().expect("should have tuplet display");
    assert_eq!(tuplet.tuplet_type, StartStop::Start);
    assert!(tuplet.bracket);
}

#[test]
fn test_parse_tie() {
    let xml = r#"<?xml version="1.0"?>
<score-partwise>
  <part-list><score-part id="P1"><part-name>P</part-name></score-part></part-list>
  <part id="P1">
<measure number="1">
  <attributes><divisions>4</divisions></attributes>
  <note>
    <pitch><step>C</step><octave>4</octave></pitch>
    <duration>4</duration><voice>1</voice><type>quarter</type>
    <notations><tied type="start"/></notations>
  </note>
  <note>
    <pitch><step>C</step><octave>4</octave></pitch>
    <duration>4</duration><voice>1</voice><type>quarter</type>
    <notations><tied type="stop"/></notations>
  </note>
</measure>
  </part>
</score-partwise>"#;

    let score = MxmlToIrAdapter::new().convert_str(xml).unwrap();
    let elems = &score.parts()[0].measures[0].voices[0].elements;
    match &elems[0] {
        VoiceElement::Note(n) => {
            assert_eq!(n.ties.len(), 1);
            assert_eq!(n.ties[0].tie_type, StartStop::Start);
        }
        _ => panic!("expected Note"),
    }
    match &elems[1] {
        VoiceElement::Note(n) => {
            assert_eq!(n.ties.len(), 1);
            assert_eq!(n.ties[0].tie_type, StartStop::Stop);
        }
        _ => panic!("expected Note"),
    }
}

#[test]
fn test_parse_slur() {
    let xml = r#"<?xml version="1.0"?>
<score-partwise>
  <part-list><score-part id="P1"><part-name>P</part-name></score-part></part-list>
  <part id="P1">
<measure number="1">
  <attributes><divisions>4</divisions></attributes>
  <note>
    <pitch><step>C</step><octave>4</octave></pitch>
    <duration>4</duration><voice>1</voice><type>quarter</type>
    <notations><slur type="start" number="1" placement="above"/></notations>
  </note>
  <note>
    <pitch><step>D</step><octave>4</octave></pitch>
    <duration>4</duration><voice>1</voice><type>quarter</type>
    <notations><slur type="stop" number="1"/></notations>
  </note>
</measure>
  </part>
</score-partwise>"#;

    let score = MxmlToIrAdapter::new().convert_str(xml).unwrap();
    let elems = &score.parts()[0].measures[0].voices[0].elements;
    match &elems[0] {
        VoiceElement::Note(n) => {
            assert_eq!(n.slurs.len(), 1);
            assert_eq!(n.slurs[0].slur_type, StartStop::Start);
            assert_eq!(n.slurs[0].number, 1);
            assert_eq!(n.slurs[0].placement, crate::ir::Placement::Above);
        }
        _ => panic!("expected Note"),
    }
}

#[test]
fn test_parse_articulations() {
    let xml = r#"<?xml version="1.0"?>
<score-partwise>
  <part-list><score-part id="P1"><part-name>P</part-name></score-part></part-list>
  <part id="P1">
<measure number="1">
  <attributes><divisions>4</divisions></attributes>
  <note>
    <pitch><step>C</step><octave>4</octave></pitch>
    <duration>4</duration><voice>1</voice><type>quarter</type>
    <notations>
      <articulations>
        <staccato placement="above"/>
        <accent/>
        <tenuto placement="below"/>
      </articulations>
    </notations>
  </note>
</measure>
  </part>
</score-partwise>"#;

    let score = MxmlToIrAdapter::new().convert_str(xml).unwrap();
    let note = match &score.parts()[0].measures[0].voices[0].elements[0] {
        VoiceElement::Note(n) => n,
        _ => panic!("expected Note"),
    };
    assert_eq!(note.articulations.len(), 3);
    assert_eq!(note.articulations[0].name, "staccato");
    assert_eq!(note.articulations[0].placement, crate::ir::Placement::Above);
    assert_eq!(note.articulations[1].name, "accent");
    assert_eq!(note.articulations[2].name, "tenuto");
    assert_eq!(note.articulations[2].placement, crate::ir::Placement::Below);
}

#[test]
fn test_parse_ornaments() {
    let xml = r#"<?xml version="1.0"?>
<score-partwise>
  <part-list><score-part id="P1"><part-name>P</part-name></score-part></part-list>
  <part id="P1">
<measure number="1">
  <attributes><divisions>4</divisions></attributes>
  <note>
    <pitch><step>C</step><octave>4</octave></pitch>
    <duration>4</duration><voice>1</voice><type>quarter</type>
    <notations><ornaments>
      <trill-mark/>
      <mordent/>
      <turn placement="above"/>
    </ornaments></notations>
  </note>
</measure>
  </part>
</score-partwise>"#;

    let score = MxmlToIrAdapter::new().convert_str(xml).unwrap();
    let note = match &score.parts()[0].measures[0].voices[0].elements[0] {
        VoiceElement::Note(n) => n,
        _ => panic!("expected Note"),
    };
    let orn_names: Vec<&str> = note.ornaments.iter().map(|o| o.name.as_str()).collect();
    assert!(orn_names.contains(&"trill-mark"));
    assert!(orn_names.contains(&"mordent"));
    assert!(orn_names.contains(&"turn"));
}

#[test]
fn test_parse_technicals() {
    let xml = r#"<?xml version="1.0"?>
<score-partwise>
  <part-list><score-part id="P1"><part-name>P</part-name></score-part></part-list>
  <part id="P1">
<measure number="1">
  <attributes><divisions>4</divisions></attributes>
  <note>
    <pitch><step>C</step><octave>4</octave></pitch>
    <duration>4</duration><voice>1</voice><type>quarter</type>
    <notations><technical>
      <fingering>3</fingering>
      <up-bow/>
    </technical></notations>
  </note>
</measure>
  </part>
</score-partwise>"#;

    let score = MxmlToIrAdapter::new().convert_str(xml).unwrap();
    let note = match &score.parts()[0].measures[0].voices[0].elements[0] {
        VoiceElement::Note(n) => n,
        _ => panic!("expected Note"),
    };
    assert_eq!(note.technicals.len(), 2);
    assert!(note
        .technicals
        .iter()
        .any(|t| t.name == "fingering" && t.value == "3"));
    assert!(note.technicals.iter().any(|t| t.name == "up-bow"));
}

#[test]
fn test_parse_fermata() {
    let xml = r#"<?xml version="1.0"?>
<score-partwise>
  <part-list><score-part id="P1"><part-name>P</part-name></score-part></part-list>
  <part id="P1">
<measure number="1">
  <attributes><divisions>4</divisions></attributes>
  <note>
    <pitch><step>C</step><octave>4</octave></pitch>
    <duration>4</duration><voice>1</voice><type>quarter</type>
    <notations><fermata type="inverted">angled</fermata></notations>
  </note>
</measure>
  </part>
</score-partwise>"#;

    let score = MxmlToIrAdapter::new().convert_str(xml).unwrap();
    let note = match &score.parts()[0].measures[0].voices[0].elements[0] {
        VoiceElement::Note(n) => n,
        _ => panic!("expected Note"),
    };
    let fermata = note.fermata.as_ref().expect("should have fermata");
    assert_eq!(fermata.shape, "angled");
    assert!(fermata.inverted);
}

#[test]
fn test_parse_lyrics() {
    use crate::ir::articulation::SyllabicType;
    let xml = r#"<?xml version="1.0"?>
<score-partwise>
  <part-list><score-part id="P1"><part-name>P</part-name></score-part></part-list>
  <part id="P1">
<measure number="1">
  <attributes><divisions>4</divisions></attributes>
  <note>
    <pitch><step>C</step><octave>4</octave></pitch>
    <duration>4</duration><voice>1</voice><type>quarter</type>
    <lyric number="1"><syllabic>begin</syllabic><text>Hel</text></lyric>
  </note>
  <note>
    <pitch><step>D</step><octave>4</octave></pitch>
    <duration>4</duration><voice>1</voice><type>quarter</type>
    <lyric number="1"><syllabic>end</syllabic><text>lo</text><extend/></lyric>
  </note>
</measure>
  </part>
</score-partwise>"#;

    let score = MxmlToIrAdapter::new().convert_str(xml).unwrap();
    let elems = &score.parts()[0].measures[0].voices[0].elements;
    match &elems[0] {
        VoiceElement::Note(n) => {
            assert_eq!(n.lyrics.len(), 1);
            assert_eq!(n.lyrics[0].text, "Hel");
            assert_eq!(n.lyrics[0].syllabic, SyllabicType::Begin);
        }
        _ => panic!("expected Note"),
    }
    match &elems[1] {
        VoiceElement::Note(n) => {
            assert_eq!(n.lyrics[0].text, "lo");
            assert_eq!(n.lyrics[0].syllabic, SyllabicType::End);
            assert!(n.lyrics[0].extend);
        }
        _ => panic!("expected Note"),
    }
}

#[test]
fn test_parse_beams() {
    let xml = r#"<?xml version="1.0"?>
<score-partwise>
  <part-list><score-part id="P1"><part-name>P</part-name></score-part></part-list>
  <part id="P1">
<measure number="1">
  <attributes><divisions>4</divisions></attributes>
  <note>
    <pitch><step>C</step><octave>4</octave></pitch>
    <duration>2</duration><voice>1</voice><type>eighth</type>
    <beam number="1">begin</beam>
  </note>
  <note>
    <pitch><step>D</step><octave>4</octave></pitch>
    <duration>2</duration><voice>1</voice><type>eighth</type>
    <beam number="1">end</beam>
  </note>
</measure>
  </part>
</score-partwise>"#;

    let score = MxmlToIrAdapter::new().convert_str(xml).unwrap();
    let elems = &score.parts()[0].measures[0].voices[0].elements;
    match &elems[0] {
        VoiceElement::Note(n) => {
            assert_eq!(n.beams.len(), 1);
            assert_eq!(n.beams[0].beam_type, "begin");
            assert_eq!(n.beams[0].number, 1);
        }
        _ => panic!("expected Note"),
    }
}

#[test]
fn test_parse_editorial_accidental() {
    let xml = r#"<?xml version="1.0"?>
<score-partwise>
  <part-list><score-part id="P1"><part-name>P</part-name></score-part></part-list>
  <part id="P1">
<measure number="1">
  <attributes><divisions>4</divisions></attributes>
  <note>
    <pitch><step>F</step><alter>1</alter><octave>4</octave></pitch>
    <duration>4</duration><voice>1</voice><type>quarter</type>
    <accidental editorial="yes">sharp</accidental>
  </note>
</measure>
  </part>
</score-partwise>"#;

    let score = MxmlToIrAdapter::new().convert_str(xml).unwrap();
    match &score.parts()[0].measures[0].voices[0].elements[0] {
        VoiceElement::Note(n) => {
            assert_eq!(n.pitch.accidental, AccidentalDisplay::Editorial);
        }
        _ => panic!("expected Note"),
    }
}

#[test]
fn test_parse_stem_direction() {
    let xml = r#"<?xml version="1.0"?>
<score-partwise>
  <part-list><score-part id="P1"><part-name>P</part-name></score-part></part-list>
  <part id="P1">
<measure number="1">
  <attributes><divisions>4</divisions></attributes>
  <note>
    <pitch><step>C</step><octave>4</octave></pitch>
    <duration>4</duration><voice>1</voice><type>quarter</type>
    <stem>down</stem>
  </note>
</measure>
  </part>
</score-partwise>"#;

    let score = MxmlToIrAdapter::new().convert_str(xml).unwrap();
    match &score.parts()[0].measures[0].voices[0].elements[0] {
        VoiceElement::Note(n) => assert_eq!(n.stem_direction, "down"),
        _ => panic!("expected Note"),
    }
}

#[test]
fn test_parse_cue_note() {
    let xml = r#"<?xml version="1.0"?>
<score-partwise>
  <part-list><score-part id="P1"><part-name>P</part-name></score-part></part-list>
  <part id="P1">
<measure number="1">
  <attributes><divisions>4</divisions></attributes>
  <note>
    <cue/>
    <pitch><step>C</step><octave>4</octave></pitch>
    <duration>4</duration><voice>1</voice><type>quarter</type>
  </note>
</measure>
  </part>
</score-partwise>"#;

    let score = MxmlToIrAdapter::new().convert_str(xml).unwrap();
    match &score.parts()[0].measures[0].voices[0].elements[0] {
        VoiceElement::Note(n) => assert!(n.is_cue),
        _ => panic!("expected Note"),
    }
}

// --- Direction parsing edge cases ---

#[test]
fn test_parse_wedge() {
    let xml = r#"<?xml version="1.0"?>
<score-partwise>
  <part-list><score-part id="P1"><part-name>P</part-name></score-part></part-list>
  <part id="P1">
<measure number="1">
  <attributes><divisions>4</divisions></attributes>
  <direction placement="below">
    <direction-type><wedge type="crescendo"/></direction-type>
  </direction>
  <note>
    <pitch><step>C</step><octave>4</octave></pitch>
    <duration>4</duration><voice>1</voice><type>quarter</type>
  </note>
</measure>
  </part>
</score-partwise>"#;

    let score = MxmlToIrAdapter::new().convert_str(xml).unwrap();
    let dir = &score.parts()[0].measures[0].directions[0];
    let wedge = dir.wedge.as_ref().expect("should have wedge");
    assert_eq!(wedge.wedge_type, "crescendo");
    assert_eq!(dir.placement, crate::ir::Placement::Below);
}

#[test]
fn test_parse_pedal() {
    let xml = r#"<?xml version="1.0"?>
<score-partwise>
  <part-list><score-part id="P1"><part-name>P</part-name></score-part></part-list>
  <part id="P1">
<measure number="1">
  <attributes><divisions>4</divisions></attributes>
  <direction>
    <direction-type><pedal type="start" line="yes"/></direction-type>
  </direction>
  <note>
    <pitch><step>C</step><octave>4</octave></pitch>
    <duration>4</duration><voice>1</voice><type>quarter</type>
  </note>
  <direction>
    <direction-type><pedal type="stop" line="yes"/></direction-type>
  </direction>
</measure>
  </part>
</score-partwise>"#;

    let score = MxmlToIrAdapter::new().convert_str(xml).unwrap();
    let dirs = &score.parts()[0].measures[0].directions;
    let start = dirs.iter().find(|d| d.pedal.is_some()).unwrap();
    let pedal = start.pedal.as_ref().unwrap();
    assert_eq!(pedal.pedal_type, "start");
    assert!(pedal.line);
}

#[test]
fn test_parse_octave_shift() {
    let xml = r#"<?xml version="1.0"?>
<score-partwise>
  <part-list><score-part id="P1"><part-name>P</part-name></score-part></part-list>
  <part id="P1">
<measure number="1">
  <attributes><divisions>4</divisions></attributes>
  <direction>
    <direction-type><octave-shift type="down" size="8"/></direction-type>
  </direction>
  <note>
    <pitch><step>C</step><octave>4</octave></pitch>
    <duration>4</duration><voice>1</voice><type>quarter</type>
  </note>
</measure>
  </part>
</score-partwise>"#;

    let score = MxmlToIrAdapter::new().convert_str(xml).unwrap();
    let dir = &score.parts()[0].measures[0].directions[0];
    let ott = dir.octave_shift.as_ref().expect("should have octave shift");
    assert_eq!(ott.shift_type, "down");
    assert_eq!(ott.size, 8);
}

#[test]
fn test_parse_rehearsal_mark() {
    let xml = r#"<?xml version="1.0"?>
<score-partwise>
  <part-list><score-part id="P1"><part-name>P</part-name></score-part></part-list>
  <part id="P1">
<measure number="1">
  <attributes><divisions>4</divisions></attributes>
  <direction>
    <direction-type><rehearsal>A</rehearsal></direction-type>
  </direction>
  <note>
    <pitch><step>C</step><octave>4</octave></pitch>
    <duration>4</duration><voice>1</voice><type>quarter</type>
  </note>
</measure>
  </part>
</score-partwise>"#;

    let score = MxmlToIrAdapter::new().convert_str(xml).unwrap();
    let dir = &score.parts()[0].measures[0].directions[0];
    let rehearsal = dir.rehearsal.as_ref().expect("should have rehearsal mark");
    assert_eq!(rehearsal.text, "A");
}

#[test]
fn test_parse_words_direction() {
    let xml = r#"<?xml version="1.0"?>
<score-partwise>
  <part-list><score-part id="P1"><part-name>P</part-name></score-part></part-list>
  <part id="P1">
<measure number="1">
  <attributes><divisions>4</divisions></attributes>
  <direction placement="above">
    <direction-type><words font-style="italic" font-weight="bold">dolce</words></direction-type>
  </direction>
  <note>
    <pitch><step>C</step><octave>4</octave></pitch>
    <duration>4</duration><voice>1</voice><type>quarter</type>
  </note>
</measure>
  </part>
</score-partwise>"#;

    let score = MxmlToIrAdapter::new().convert_str(xml).unwrap();
    let dir = &score.parts()[0].measures[0].directions[0];
    let text = dir.text.as_ref().expect("should have text direction");
    assert_eq!(text.text, "dolce");
    assert_eq!(text.font_style.as_deref(), Some("italic"));
    assert_eq!(text.font_weight.as_deref(), Some("bold"));
}

#[test]
fn test_parse_da_capo_from_sound() {
    let xml = r#"<?xml version="1.0"?>
<score-partwise>
  <part-list><score-part id="P1"><part-name>P</part-name></score-part></part-list>
  <part id="P1">
<measure number="1">
  <attributes><divisions>4</divisions></attributes>
  <note>
    <pitch><step>C</step><octave>4</octave></pitch>
    <duration>4</duration><voice>1</voice><type>quarter</type>
  </note>
  <direction>
    <direction-type><words>D.C. al Fine</words></direction-type>
    <sound dacapo="yes"/>
  </direction>
</measure>
  </part>
</score-partwise>"#;

    let score = MxmlToIrAdapter::new().convert_str(xml).unwrap();
    let dirs = &score.parts()[0].measures[0].directions;
    assert!(
        dirs.iter().any(|d| d.da_capo.is_some()),
        "should have da capo"
    );
}

#[test]
fn test_parse_tempo_from_sound_only() {
    let xml = r#"<?xml version="1.0"?>
<score-partwise>
  <part-list><score-part id="P1"><part-name>P</part-name></score-part></part-list>
  <part id="P1">
<measure number="1">
  <attributes><divisions>4</divisions></attributes>
  <direction>
    <direction-type><words>Allegro</words></direction-type>
    <sound tempo="132"/>
  </direction>
  <note>
    <pitch><step>C</step><octave>4</octave></pitch>
    <duration>4</duration><voice>1</voice><type>quarter</type>
  </note>
</measure>
  </part>
</score-partwise>"#;

    let score = MxmlToIrAdapter::new().convert_str(xml).unwrap();
    let dir = &score.parts()[0].measures[0].directions[0];
    let tempo = dir.tempo.as_ref().expect("should have tempo");
    assert_eq!(tempo.per_minute, Some(132.0));
    assert_eq!(tempo.text.as_deref(), Some("Allegro"));
}

// --- Barline parsing ---

#[test]
fn test_parse_barline_styles() {
    use crate::ir::direction::BarlineType;
    let xml = r#"<?xml version="1.0"?>
<score-partwise>
  <part-list><score-part id="P1"><part-name>P</part-name></score-part></part-list>
  <part id="P1">
<measure number="1">
  <attributes><divisions>4</divisions></attributes>
  <note>
    <pitch><step>C</step><octave>4</octave></pitch>
    <duration>4</duration><voice>1</voice><type>quarter</type>
  </note>
  <barline location="right"><bar-style>light-heavy</bar-style></barline>
</measure>
  </part>
</score-partwise>"#;

    let score = MxmlToIrAdapter::new().convert_str(xml).unwrap();
    let barline = score.parts()[0].measures[0]
        .right_barline
        .as_ref()
        .expect("should have right barline");
    assert_eq!(barline.style, BarlineType::Final);
}

#[test]
fn test_parse_repeat_barline() {
    use crate::ir::direction::{BarlineType, RepeatDirection};
    let xml = r#"<?xml version="1.0"?>
<score-partwise>
  <part-list><score-part id="P1"><part-name>P</part-name></score-part></part-list>
  <part id="P1">
<measure number="1">
  <attributes><divisions>4</divisions></attributes>
  <barline location="left">
    <bar-style>light-heavy</bar-style>
    <repeat direction="forward"/>
  </barline>
  <note>
    <pitch><step>C</step><octave>4</octave></pitch>
    <duration>4</duration><voice>1</voice><type>quarter</type>
  </note>
  <barline location="right">
    <bar-style>light-heavy</bar-style>
    <repeat direction="backward"/>
  </barline>
</measure>
  </part>
</score-partwise>"#;

    let score = MxmlToIrAdapter::new().convert_str(xml).unwrap();
    let m = &score.parts()[0].measures[0];
    assert_eq!(
        m.left_barline.as_ref().unwrap().repeat_direction,
        Some(RepeatDirection::Forward)
    );
    assert_eq!(
        m.right_barline.as_ref().unwrap().repeat_direction,
        Some(RepeatDirection::Backward)
    );
}

#[test]
fn test_parse_volta_ending() {
    let xml = r#"<?xml version="1.0"?>
<score-partwise>
  <part-list><score-part id="P1"><part-name>P</part-name></score-part></part-list>
  <part id="P1">
<measure number="1">
  <attributes><divisions>4</divisions></attributes>
  <barline location="left">
    <ending number="1" type="start"/>
  </barline>
  <note>
    <pitch><step>C</step><octave>4</octave></pitch>
    <duration>4</duration><voice>1</voice><type>quarter</type>
  </note>
</measure>
  </part>
</score-partwise>"#;

    let score = MxmlToIrAdapter::new().convert_str(xml).unwrap();
    let barline = score.parts()[0].measures[0].left_barline.as_ref().unwrap();
    assert_eq!(barline.ending_number, Some(1));
    assert_eq!(barline.ending_type.as_deref(), Some("start"));
}

// --- Attributes parsing ---

#[test]
fn test_parse_transpose_attribute() {
    let xml = r#"<?xml version="1.0"?>
<score-partwise>
  <part-list><score-part id="P1"><part-name>P</part-name></score-part></part-list>
  <part id="P1">
<measure number="1">
  <attributes>
    <divisions>4</divisions>
    <transpose><diatonic>-1</diatonic><chromatic>-2</chromatic><octave-change>0</octave-change></transpose>
  </attributes>
  <note>
    <pitch><step>C</step><octave>4</octave></pitch>
    <duration>4</duration><voice>1</voice><type>quarter</type>
  </note>
</measure>
  </part>
</score-partwise>"#;

    let score = MxmlToIrAdapter::new().convert_str(xml).unwrap();
    let attrs = score.parts()[0].measures[0].attributes.as_ref().unwrap();
    let transpose = attrs.transpose.as_ref().expect("should have transpose");
    assert_eq!(transpose.diatonic, -1);
    assert_eq!(transpose.chromatic, -2);
}

#[test]
fn test_parse_multi_staff() {
    let xml = r#"<?xml version="1.0"?>
<score-partwise>
  <part-list><score-part id="P1"><part-name>Piano</part-name></score-part></part-list>
  <part id="P1">
<measure number="1">
  <attributes>
    <divisions>4</divisions>
    <staves>2</staves>
    <clef number="1"><sign>G</sign><line>2</line></clef>
    <clef number="2"><sign>F</sign><line>4</line></clef>
  </attributes>
  <note>
    <pitch><step>C</step><octave>5</octave></pitch>
    <duration>4</duration><voice>1</voice><staff>1</staff><type>quarter</type>
  </note>
  <backup><duration>4</duration></backup>
  <note>
    <pitch><step>C</step><octave>3</octave></pitch>
    <duration>4</duration><voice>2</voice><staff>2</staff><type>quarter</type>
  </note>
</measure>
  </part>
</score-partwise>"#;

    let score = MxmlToIrAdapter::new().convert_str(xml).unwrap();
    let part = &score.parts()[0];
    assert_eq!(part.staves, 2);
    let attrs = part.measures[0].attributes.as_ref().unwrap();
    assert_eq!(attrs.clefs.len(), 2);
    assert_eq!(attrs.clefs[&1].sign, crate::ir::measure::ClefSign::G);
    assert_eq!(attrs.clefs[&2].sign, crate::ir::measure::ClefSign::F);
}

#[test]
fn test_parse_compound_time_signature() {
    let xml = r#"<?xml version="1.0"?>
<score-partwise>
  <part-list><score-part id="P1"><part-name>P</part-name></score-part></part-list>
  <part id="P1">
<measure number="1">
  <attributes>
    <divisions>4</divisions>
    <time><beats>3</beats><beats>2</beats><beat-type>8</beat-type></time>
  </attributes>
  <note>
    <pitch><step>C</step><octave>4</octave></pitch>
    <duration>4</duration><voice>1</voice><type>quarter</type>
  </note>
</measure>
  </part>
</score-partwise>"#;

    let score = MxmlToIrAdapter::new().convert_str(xml).unwrap();
    let time = score.parts()[0].measures[0]
        .attributes
        .as_ref()
        .unwrap()
        .time
        .as_ref()
        .unwrap();
    assert_eq!(time.beats, "3+2");
    assert_eq!(time.beat_type, 8);
}

#[test]
fn test_parse_forward_as_spacer() {
    let xml = r#"<?xml version="1.0"?>
<score-partwise>
  <part-list><score-part id="P1"><part-name>P</part-name></score-part></part-list>
  <part id="P1">
<measure number="1">
  <attributes><divisions>4</divisions></attributes>
  <forward><duration>4</duration><voice>2</voice></forward>
  <note>
    <pitch><step>E</step><octave>4</octave></pitch>
    <duration>4</duration><voice>2</voice><type>quarter</type>
  </note>
</measure>
  </part>
</score-partwise>"#;

    let score = MxmlToIrAdapter::new().convert_str(xml).unwrap();
    let voice = score.parts()[0].measures[0]
        .voices
        .iter()
        .find(|v| v.number == 2)
        .unwrap();
    // First element should be a spacer rest from the <forward>
    match &voice.elements[0] {
        VoiceElement::Rest(r) => {
            assert!(r.is_spacer, "forward should become spacer rest");
        }
        _ => panic!("expected spacer Rest for <forward>"),
    }
}

#[test]
fn test_parse_arpeggio() {
    let xml = r#"<?xml version="1.0"?>
<score-partwise>
  <part-list><score-part id="P1"><part-name>P</part-name></score-part></part-list>
  <part id="P1">
<measure number="1">
  <attributes><divisions>4</divisions></attributes>
  <note>
    <pitch><step>C</step><octave>4</octave></pitch>
    <duration>4</duration><voice>1</voice><type>quarter</type>
    <notations><arpeggiate direction="down"/></notations>
  </note>
  <note>
    <chord/>
    <pitch><step>E</step><octave>4</octave></pitch>
    <duration>4</duration><voice>1</voice><type>quarter</type>
    <notations><arpeggiate direction="down"/></notations>
  </note>
</measure>
  </part>
</score-partwise>"#;

    let score = MxmlToIrAdapter::new().convert_str(xml).unwrap();
    match &score.parts()[0].measures[0].voices[0].elements[0] {
        VoiceElement::Chord(c) => {
            assert_eq!(c.arpeggio, Some(crate::ir::note::ArpeggioType::Down));
        }
        _ => panic!("expected Chord with arpeggio"),
    }
}

#[test]
fn test_parse_slide_portamento() {
    let xml = r#"<?xml version="1.0"?>
<score-partwise>
  <part-list><score-part id="P1"><part-name>P</part-name></score-part></part-list>
  <part id="P1">
<measure number="1">
  <attributes><divisions>4</divisions></attributes>
  <note>
    <pitch><step>C</step><octave>4</octave></pitch>
    <duration>4</duration><voice>1</voice><type>quarter</type>
    <notations><slide type="start"/></notations>
  </note>
</measure>
  </part>
</score-partwise>"#;

    let score = MxmlToIrAdapter::new().convert_str(xml).unwrap();
    match &score.parts()[0].measures[0].voices[0].elements[0] {
        VoiceElement::Note(n) => {
            assert_eq!(n.slide, Some(StartStop::Start));
        }
        _ => panic!("expected Note"),
    }
}

// --- Part info / metadata ---

#[test]
fn test_parse_midi_instrument_info() {
    let xml = r#"<?xml version="1.0"?>
<score-partwise>
  <part-list>
    <score-part id="P1">
      <part-name>Violin I</part-name>
      <part-abbreviation>Vln. I</part-abbreviation>
      <midi-instrument id="P1-I1">
        <midi-channel>1</midi-channel>
        <midi-program>41</midi-program>
        <midi-name>Violin</midi-name>
      </midi-instrument>
    </score-part>
  </part-list>
  <part id="P1">
<measure number="1">
  <attributes><divisions>4</divisions></attributes>
  <note>
    <pitch><step>C</step><octave>4</octave></pitch>
    <duration>4</duration><voice>1</voice><type>quarter</type>
  </note>
</measure>
  </part>
</score-partwise>"#;

    let score = MxmlToIrAdapter::new().convert_str(xml).unwrap();
    let part = &score.parts()[0];
    assert_eq!(part.name, "Violin I");
    assert_eq!(part.abbreviation, "Vln. I");
    assert_eq!(part.midi_instrument, "Violin");
    assert_eq!(part.midi_channel, 1);
    assert_eq!(part.midi_program, 41);
}

#[test]
fn test_parse_part_group() {
    use crate::ir::score::ScoreChild;
    let xml = r#"<?xml version="1.0"?>
<score-partwise>
  <part-list>
    <part-group type="start" number="1">
      <group-name>Piano</group-name>
      <group-symbol>brace</group-symbol>
    </part-group>
    <score-part id="P1"><part-name>Treble</part-name></score-part>
    <score-part id="P2"><part-name>Bass</part-name></score-part>
    <part-group type="stop" number="1"/>
  </part-list>
  <part id="P1">
<measure number="1">
  <attributes><divisions>4</divisions></attributes>
  <note><pitch><step>C</step><octave>5</octave></pitch><duration>4</duration><voice>1</voice><type>quarter</type></note>
</measure>
  </part>
  <part id="P2">
<measure number="1">
  <attributes><divisions>4</divisions></attributes>
  <note><pitch><step>C</step><octave>3</octave></pitch><duration>4</duration><voice>1</voice><type>quarter</type></note>
</measure>
  </part>
</score-partwise>"#;

    let score = MxmlToIrAdapter::new().convert_str(xml).unwrap();
    // Parts should be in a PartGroup
    assert_eq!(score.children.len(), 1);
    match &score.children[0] {
        ScoreChild::PartGroup(pg) => {
            assert_eq!(pg.bracket, "brace");
            assert_eq!(pg.group_type, "PianoStaff");
            assert_eq!(pg.children.len(), 2);
        }
        _ => panic!("expected PartGroup"),
    }
}

// --- Harmony parsing edge cases ---

#[test]
fn test_parse_harmony_with_degrees() {
    let xml = r#"<?xml version="1.0"?>
<score-partwise>
  <part-list><score-part id="P1"><part-name>P</part-name></score-part></part-list>
  <part id="P1">
<measure number="1">
  <attributes><divisions>4</divisions></attributes>
  <harmony>
    <root><root-step>G</root-step><root-alter>-1</root-alter></root>
    <kind>dominant</kind>
    <degree>
      <degree-value>9</degree-value>
      <degree-alter>1</degree-alter>
      <degree-type>add</degree-type>
    </degree>
  </harmony>
  <note><pitch><step>C</step><octave>4</octave></pitch><duration>4</duration><type>whole</type></note>
</measure>
  </part>
</score-partwise>"#;

    let score = MxmlToIrAdapter::new().convert_str(xml).unwrap();
    let h = &score.parts()[0].measures[0].harmonies[0];
    assert_eq!(h.root.step, "G");
    assert_eq!(h.root.alter, -1.0);
    assert_eq!(h.kind, "dominant");
    assert_eq!(h.degrees.len(), 1);
    assert_eq!(h.degrees[0].value, 9);
    assert_eq!(h.degrees[0].alter, 1.0);
    assert_eq!(h.degrees[0].degree_type, "add");
}

#[test]
fn test_parse_figured_bass_parentheses() {
    let xml = r#"<?xml version="1.0"?>
<score-partwise>
  <part-list><score-part id="P1"><part-name>P</part-name></score-part></part-list>
  <part id="P1">
<measure number="1">
  <attributes><divisions>4</divisions></attributes>
  <figured-bass parentheses="yes">
    <figure><figure-number>5</figure-number><suffix>sharp</suffix></figure>
    <duration>4</duration>
  </figured-bass>
  <note><pitch><step>C</step><octave>3</octave></pitch><duration>4</duration><type>whole</type></note>
</measure>
  </part>
</score-partwise>"#;

    let score = MxmlToIrAdapter::new().convert_str(xml).unwrap();
    let fb = &score.parts()[0].measures[0].figured_bass[0];
    assert!(fb.parentheses);
    assert_eq!(fb.figures[0].suffix.as_deref(), Some("sharp"));
}

#[test]
fn test_parse_rest_with_display_step() {
    let xml = r#"<?xml version="1.0"?>
<score-partwise>
  <part-list><score-part id="P1"><part-name>P</part-name></score-part></part-list>
  <part id="P1">
<measure number="1">
  <attributes><divisions>4</divisions></attributes>
  <note>
    <rest><display-step>B</display-step><display-octave>4</display-octave></rest>
    <duration>4</duration><voice>1</voice><type>quarter</type>
  </note>
</measure>
  </part>
</score-partwise>"#;

    let score = MxmlToIrAdapter::new().convert_str(xml).unwrap();
    match &score.parts()[0].measures[0].voices[0].elements[0] {
        VoiceElement::Rest(r) => {
            assert_eq!(r.display_step.as_deref(), Some("B"));
            assert_eq!(r.display_octave, Some(4));
        }
        _ => panic!("expected Rest"),
    }
}

#[test]
fn test_parse_quarter_tone_alter() {
    let xml = r#"<?xml version="1.0"?>
<score-partwise>
  <part-list><score-part id="P1"><part-name>P</part-name></score-part></part-list>
  <part id="P1">
<measure number="1">
  <attributes><divisions>4</divisions></attributes>
  <note>
    <pitch><step>C</step><alter>0.5</alter><octave>4</octave></pitch>
    <duration>4</duration><voice>1</voice><type>quarter</type>
  </note>
</measure>
  </part>
</score-partwise>"#;

    let score = MxmlToIrAdapter::new().convert_str(xml).unwrap();
    match &score.parts()[0].measures[0].voices[0].elements[0] {
        VoiceElement::Note(n) => {
            // 0.5 semitones = quarter-tone sharp = Ratio(1,2)
            assert_eq!(n.pitch.alter, Alter::new(1, 2));
        }
        _ => panic!("expected Note"),
    }
}

// --- MXL (compressed) parsing ---

#[test]
fn test_parse_all_mxl_fixtures() {
    let fixture_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("mxl");
    let adapter = MxmlToIrAdapter::new();
    let mut failures: Vec<(String, String)> = Vec::new();
    let mut count = 0;

    for entry in std::fs::read_dir(&fixture_dir).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();
        if path.extension().is_some_and(|e| e == "mxl") {
            match adapter.convert_file(&path) {
                Ok(score) => {
                    assert!(!score.parts().is_empty(), "no parts in {}", path.display());
                    count += 1;
                }
                Err(e) => {
                    failures.push((
                        path.file_name().unwrap().to_string_lossy().to_string(),
                        e.to_string(),
                    ));
                }
            }
        }
    }

    if !failures.is_empty() {
        let report: Vec<String> = failures
            .iter()
            .map(|(f, e)| format!("  {f}: {e}"))
            .collect();
        panic!(
            "{} of {} MXL fixtures failed:\n{}",
            failures.len(),
            count + failures.len(),
            report.join("\n")
        );
    }
    assert!(count >= 8, "expected at least 8 mxl fixtures, got {count}");
}

#[test]
fn test_parse_notehead() {
    let xml = r#"<?xml version="1.0"?>
<score-partwise>
  <part-list><score-part id="P1"><part-name>P</part-name></score-port></part-list>
  <part id="P1">
<measure number="1">
  <attributes><divisions>4</divisions></attributes>
  <note>
    <pitch><step>C</step><octave>4</octave></pitch>
    <duration>4</duration><voice>1</voice><type>quarter</type>
    <notehead>x</notehead>
  </note>
</measure>
  </part>
</score-partwise>"#;

    // Allow parse errors from the malformed closing tag — just test notehead parse path
    let score = MxmlToIrAdapter::new().convert_str(xml);
    // The XML has a typo (score-port vs score-part) so it may fail; test passing XML instead
    let xml2 = xml.replace("</score-port>", "</score-part>");
    let score = MxmlToIrAdapter::new().convert_str(&xml2).unwrap();
    match &score.parts()[0].measures[0].voices[0].elements[0] {
        VoiceElement::Note(n) => assert_eq!(n.notehead, "x"),
        _ => panic!("expected Note"),
    }
}

#[test]
fn test_parse_print_object_no() {
    let xml = r#"<?xml version="1.0"?>
<score-partwise>
  <part-list><score-part id="P1"><part-name>P</part-name></score-part></part-list>
  <part id="P1">
<measure number="1">
  <attributes><divisions>4</divisions></attributes>
  <note print-object="no">
    <pitch><step>C</step><octave>4</octave></pitch>
    <duration>4</duration><voice>1</voice><type>quarter</type>
  </note>
</measure>
  </part>
</score-partwise>"#;

    let score = MxmlToIrAdapter::new().convert_str(xml).unwrap();
    match &score.parts()[0].measures[0].voices[0].elements[0] {
        VoiceElement::Note(n) => assert!(!n.print_object),
        _ => panic!("expected Note"),
    }
}

#[test]
fn test_parse_after_grace_steal_time() {
    let xml = r#"<?xml version="1.0"?>
<score-partwise>
  <part-list><score-part id="P1"><part-name>P</part-name></score-part></part-list>
  <part id="P1">
<measure number="1">
  <attributes><divisions>4</divisions></attributes>
  <note>
    <grace steal-time-previous="33"/>
    <pitch><step>D</step><octave>5</octave></pitch>
    <type>16th</type>
  </note>
  <note>
    <pitch><step>C</step><octave>4</octave></pitch>
    <duration>4</duration><voice>1</voice><type>quarter</type>
  </note>
</measure>
  </part>
</score-partwise>"#;

    let score = MxmlToIrAdapter::new().convert_str(xml).unwrap();
    let elems: Vec<_> = score.parts()[0]
        .measures
        .iter()
        .flat_map(|m| &m.voices)
        .flat_map(|v| &v.elements)
        .collect();
    match &elems[0] {
        VoiceElement::Note(n) => {
            assert!(n.is_grace);
            assert!(n.after_grace, "steal-time-previous should set after_grace");
        }
        _ => panic!("expected Note"),
    }
}

// --- Unpitched / percussion ---

#[test]
fn test_parse_unpitched_note() {
    let xml = r#"<?xml version="1.0"?>
<score-partwise>
  <part-list><score-part id="P1"><part-name>Drums</part-name></score-part></part-list>
  <part id="P1">
<measure number="1">
  <attributes><divisions>4</divisions></attributes>
  <note>
    <unpitched><display-step>E</display-step><display-octave>4</display-octave></unpitched>
    <duration>4</duration><voice>1</voice><type>quarter</type>
  </note>
</measure>
  </part>
</score-partwise>"#;

    let score = MxmlToIrAdapter::new().convert_str(xml).unwrap();
    match &score.parts()[0].measures[0].voices[0].elements[0] {
        VoiceElement::Note(n) => {
            assert_eq!(n.pitch.step, PitchStep::E);
            assert_eq!(n.pitch.octave, 4);
        }
        _ => panic!("expected Note"),
    }
}

// --- Identification / metadata edge cases ---

#[test]
fn test_parse_work_title() {
    let xml = r#"<?xml version="1.0"?>
<score-partwise>
  <work><work-title>Symphony No. 5</work-title></work>
  <identification>
    <creator type="composer">Beethoven</creator>
    <creator type="arranger">Someone</creator>
    <rights>Copyright 2024</rights>
  </identification>
  <part-list><score-part id="P1"><part-name>P</part-name></score-part></part-list>
  <part id="P1">
<measure number="1">
  <attributes><divisions>4</divisions></attributes>
  <note><pitch><step>C</step><octave>4</octave></pitch><duration>4</duration><type>whole</type></note>
</measure>
  </part>
</score-partwise>"#;

    let score = MxmlToIrAdapter::new().convert_str(xml).unwrap();
    assert_eq!(score.metadata.title.as_deref(), Some("Symphony No. 5"));
    assert_eq!(score.metadata.composer.as_deref(), Some("Beethoven"));
    assert_eq!(score.metadata.arranger.as_deref(), Some("Someone"));
    assert!(!score.metadata.rights.is_empty());
}

#[test]
fn test_parse_movement_title_fallback() {
    let xml = r#"<?xml version="1.0"?>
<score-partwise>
  <movement-title>Allegro con brio</movement-title>
  <part-list><score-part id="P1"><part-name>P</part-name></score-part></part-list>
  <part id="P1">
<measure number="1">
  <attributes><divisions>4</divisions></attributes>
  <note><pitch><step>C</step><octave>4</octave></pitch><duration>4</duration><type>whole</type></note>
</measure>
  </part>
</score-partwise>"#;

    let score = MxmlToIrAdapter::new().convert_str(xml).unwrap();
    assert_eq!(score.metadata.title.as_deref(), Some("Allegro con brio"));
}

#[test]
fn test_parse_time_symbol() {
    let xml = r#"<?xml version="1.0"?>
<score-partwise>
  <part-list><score-part id="P1"><part-name>P</part-name></score-part></part-list>
  <part id="P1">
<measure number="1">
  <attributes>
    <divisions>4</divisions>
    <time symbol="common"><beats>4</beats><beat-type>4</beat-type></time>
  </attributes>
  <note><pitch><step>C</step><octave>4</octave></pitch><duration>16</duration><type>whole</type></note>
</measure>
  </part>
</score-partwise>"#;

    let score = MxmlToIrAdapter::new().convert_str(xml).unwrap();
    let time = score.parts()[0].measures[0]
        .attributes
        .as_ref()
        .unwrap()
        .time
        .as_ref()
        .unwrap();
    assert_eq!(time.symbol.as_deref(), Some("common"));
}
