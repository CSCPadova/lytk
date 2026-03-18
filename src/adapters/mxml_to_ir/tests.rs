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
    let elems: Vec<_> = score.parts()[0].measures.iter()
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
    let elems: Vec<_> = score.parts()[0].measures.iter()
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
    assert!(m1.directions.iter().any(|d| d.coda), "measure 1 should have coda");
    let m2 = &score.parts()[0].measures[1];
    assert!(m2.directions.iter().any(|d| d.segno), "measure 2 should have segno");
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
    assert!((h - 29.7).abs() < 0.1, "page height should be ~29.7cm, got {h}");
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
    let partial = score.metadata.partial_duration.as_ref()
        .expect("should detect anacrusis");
    // 1 quarter note in a 3/4 measure → partial duration = 1/4
    assert_eq!(partial.base, Ratio::new(1, 4), "partial should be a quarter note");
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
    assert!(dirs.iter().any(|d| d.layout_break == Some(crate::ir::direction::LayoutBreakType::System)),
        "should parse system break from <print>");
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
        assert!(n.ornaments.iter().any(|o| o.name == "trill-mark"), "should have trill-mark");
        assert!(n.ornaments.iter().any(|o| o.name == "wavy-line-start"), "should have wavy-line-start");
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
    assert!(out_xml.contains("<tremolo type=\"single\">3</tremolo>"),
        "tremolo should round-trip: {out_xml}");
}
