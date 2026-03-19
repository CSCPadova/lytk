//! Round-trip integration tests for adapter conversions.
//!
//! Tests that semantic content (pitches, durations, structure) survives
//! round-trip conversions: MusicXML→Score→MusicXML, LilyPond→Score→LilyPond,
//! and cross-format MusicXML→Score→LilyPond→Score.

use _core::adapters::ir_to_ly::IrToLyAdapter;
use _core::adapters::ir_to_mxml::IrToMxmlAdapter;
use _core::adapters::ly_to_ir::LyToIrAdapter;
use _core::adapters::mxml_to_ir::MxmlToIrAdapter;
use _core::adapters::{FromIrAdapter, ToIrAdapter};
use _core::ir::note::VoiceElement;
use _core::ir::pitch::PitchStep;
use _core::ir::score::Score;

use std::path::Path;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Count total notes across all parts/measures/voices.
fn count_notes(score: &Score) -> usize {
    score
        .parts()
        .iter()
        .flat_map(|p| &p.measures)
        .flat_map(|m| &m.voices)
        .flat_map(|v| &v.elements)
        .filter(|e| matches!(e, VoiceElement::Note(_) | VoiceElement::Chord(_)))
        .count()
}

/// Count total rests across all parts/measures/voices.
fn count_rests(score: &Score) -> usize {
    score
        .parts()
        .iter()
        .flat_map(|p| &p.measures)
        .flat_map(|m| &m.voices)
        .flat_map(|v| &v.elements)
        .filter(|e| matches!(e, VoiceElement::Rest(_)))
        .count()
}

/// Collect all (step, octave) pairs from all notes.
fn collect_pitches(score: &Score) -> Vec<(PitchStep, i32)> {
    score
        .parts()
        .iter()
        .flat_map(|p| &p.measures)
        .flat_map(|m| &m.voices)
        .flat_map(|v| &v.elements)
        .filter_map(|e| {
            if let VoiceElement::Note(n) = e {
                Some((n.pitch.step, n.pitch.octave))
            } else {
                None
            }
        })
        .collect()
}

// ---------------------------------------------------------------------------
// MusicXML → Score → MusicXML round-trip
// ---------------------------------------------------------------------------

/// Parse XML, emit XML, re-parse, and compare note counts and pitches.
fn assert_mxml_roundtrip(xml: &str) {
    let score1 = MxmlToIrAdapter::new()
        .convert_str(xml)
        .expect("first parse failed");

    let xml2 = IrToMxmlAdapter::new()
        .convert(&score1)
        .expect("emit failed");

    let score2 = MxmlToIrAdapter::new()
        .convert_str(&xml2)
        .expect("re-parse failed");

    let notes1 = count_notes(&score1);
    let notes2 = count_notes(&score2);
    assert_eq!(
        notes1, notes2,
        "note count changed after round-trip: {notes1} → {notes2}"
    );

    let pitches1 = collect_pitches(&score1);
    let pitches2 = collect_pitches(&score2);
    assert_eq!(pitches1, pitches2, "pitches changed after round-trip");

    let parts1 = score1.parts().len();
    let parts2 = score2.parts().len();
    assert_eq!(parts1, parts2, "part count changed: {parts1} → {parts2}");
}

#[test]
fn mxml_roundtrip_simple_melody() {
    let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE score-partwise PUBLIC "-//Recordare//DTD MusicXML 4.0 Partwise//EN" "http://www.musicxml.org/dtds/partwise.dtd">
<score-partwise>
  <part-list><score-part id="P1"><part-name>Part</part-name></score-part></part-list>
  <part id="P1">
    <measure number="1">
      <attributes><divisions>1</divisions>
        <key><fifths>0</fifths></key>
        <time><beats>4</beats><beat-type>4</beat-type></time>
        <clef><sign>G</sign><line>2</line></clef>
      </attributes>
      <note><pitch><step>C</step><octave>4</octave></pitch><duration>1</duration><type>quarter</type></note>
      <note><pitch><step>D</step><octave>4</octave></pitch><duration>1</duration><type>quarter</type></note>
      <note><pitch><step>E</step><octave>4</octave></pitch><duration>1</duration><type>quarter</type></note>
      <note><pitch><step>F</step><octave>4</octave></pitch><duration>1</duration><type>quarter</type></note>
    </measure>
  </part>
</score-partwise>"#;
    assert_mxml_roundtrip(xml);
}

#[test]
fn mxml_roundtrip_chords() {
    let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE score-partwise PUBLIC "-//Recordare//DTD MusicXML 4.0 Partwise//EN" "http://www.musicxml.org/dtds/partwise.dtd">
<score-partwise>
  <part-list><score-part id="P1"><part-name>Part</part-name></score-part></part-list>
  <part id="P1">
    <measure number="1">
      <attributes><divisions>1</divisions>
        <key><fifths>0</fifths></key>
        <time><beats>4</beats><beat-type>4</beat-type></time>
        <clef><sign>G</sign><line>2</line></clef>
      </attributes>
      <note><pitch><step>C</step><octave>4</octave></pitch><duration>1</duration><type>quarter</type></note>
      <note><chord/><pitch><step>E</step><octave>4</octave></pitch><duration>1</duration><type>quarter</type></note>
      <note><chord/><pitch><step>G</step><octave>4</octave></pitch><duration>1</duration><type>quarter</type></note>
      <note><pitch><step>D</step><octave>4</octave></pitch><duration>1</duration><type>quarter</type></note>
      <note><rest/><duration>2</duration><type>half</type></note>
    </measure>
  </part>
</score-partwise>"#;

    let score1 = MxmlToIrAdapter::new().convert_str(xml).unwrap();
    let xml2 = IrToMxmlAdapter::new().convert(&score1).unwrap();
    let score2 = MxmlToIrAdapter::new().convert_str(&xml2).unwrap();

    // Chord should survive: 1 chord + 1 note + 1 rest
    let parts1 = score1.parts();
    let elems1 = &parts1[0].measures[0].voices[0].elements;
    let parts2 = score2.parts();
    let elems2 = &parts2[0].measures[0].voices[0].elements;
    assert_eq!(elems1.len(), elems2.len(), "element count should match");
}

#[test]
fn mxml_roundtrip_dotted_notes() {
    let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE score-partwise PUBLIC "-//Recordare//DTD MusicXML 4.0 Partwise//EN" "http://www.musicxml.org/dtds/partwise.dtd">
<score-partwise>
  <part-list><score-part id="P1"><part-name>Part</part-name></score-part></part-list>
  <part id="P1">
    <measure number="1">
      <attributes><divisions>2</divisions>
        <key><fifths>0</fifths></key>
        <time><beats>4</beats><beat-type>4</beat-type></time>
        <clef><sign>G</sign><line>2</line></clef>
      </attributes>
      <note><pitch><step>C</step><octave>4</octave></pitch><duration>3</duration><type>quarter</type><dot/></note>
      <note><pitch><step>D</step><octave>4</octave></pitch><duration>1</duration><type>eighth</type></note>
      <note><pitch><step>E</step><octave>4</octave></pitch><duration>4</duration><type>half</type></note>
    </measure>
  </part>
</score-partwise>"#;
    assert_mxml_roundtrip(xml);
}

#[test]
fn mxml_roundtrip_two_parts() {
    let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE score-partwise PUBLIC "-//Recordare//DTD MusicXML 4.0 Partwise//EN" "http://www.musicxml.org/dtds/partwise.dtd">
<score-partwise>
  <part-list>
    <score-part id="P1"><part-name>Violin</part-name></score-part>
    <score-part id="P2"><part-name>Cello</part-name></score-part>
  </part-list>
  <part id="P1">
    <measure number="1">
      <attributes><divisions>1</divisions>
        <key><fifths>0</fifths></key>
        <time><beats>4</beats><beat-type>4</beat-type></time>
        <clef><sign>G</sign><line>2</line></clef>
      </attributes>
      <note><pitch><step>C</step><octave>5</octave></pitch><duration>1</duration><type>quarter</type></note>
      <note><pitch><step>D</step><octave>5</octave></pitch><duration>1</duration><type>quarter</type></note>
      <note><pitch><step>E</step><octave>5</octave></pitch><duration>1</duration><type>quarter</type></note>
      <note><pitch><step>F</step><octave>5</octave></pitch><duration>1</duration><type>quarter</type></note>
    </measure>
  </part>
  <part id="P2">
    <measure number="1">
      <attributes><divisions>1</divisions>
        <key><fifths>0</fifths></key>
        <time><beats>4</beats><beat-type>4</beat-type></time>
        <clef><sign>F</sign><line>4</line></clef>
      </attributes>
      <note><pitch><step>C</step><octave>3</octave></pitch><duration>1</duration><type>quarter</type></note>
      <note><pitch><step>D</step><octave>3</octave></pitch><duration>1</duration><type>quarter</type></note>
      <note><pitch><step>E</step><octave>3</octave></pitch><duration>1</duration><type>quarter</type></note>
      <note><pitch><step>F</step><octave>3</octave></pitch><duration>1</duration><type>quarter</type></note>
    </measure>
  </part>
</score-partwise>"#;
    assert_mxml_roundtrip(xml);
}

// ---------------------------------------------------------------------------
// LilyPond → Score → LilyPond round-trip
// ---------------------------------------------------------------------------

/// Parse LY, emit LY, re-parse, and compare note counts.
fn assert_ly_roundtrip(ly: &str) {
    let score1 = LyToIrAdapter::new()
        .convert_str(ly)
        .expect("first LY parse failed");

    let ly2 = IrToLyAdapter::new()
        .convert(&score1)
        .expect("LY emit failed");

    let score2 = LyToIrAdapter::new()
        .convert_str(&ly2)
        .expect("re-parse LY failed");

    let notes1 = count_notes(&score1);
    let notes2 = count_notes(&score2);
    assert_eq!(
        notes1, notes2,
        "LY round-trip note count: {notes1} → {notes2}\nEmitted:\n{ly2}"
    );

    let parts1 = score1.parts().len();
    let parts2 = score2.parts().len();
    assert_eq!(
        parts1, parts2,
        "LY round-trip part count: {parts1} → {parts2}\nEmitted:\n{ly2}"
    );
}

#[test]
fn ly_roundtrip_simple_melody() {
    assert_ly_roundtrip(r#"{ c'4 d' e' f' }"#);
}

#[test]
fn ly_roundtrip_key_and_time() {
    assert_ly_roundtrip(r#"{ \key g \major \time 3/4 g'4 a' b' }"#);
}

#[test]
fn ly_roundtrip_chords() {
    assert_ly_roundtrip(r#"{ <c' e' g'>4 <d' f' a'> <e' g' b'> <f' a' c''> }"#);
}

// ---------------------------------------------------------------------------
// Cross-format: MusicXML → Score → LilyPond → Score
// ---------------------------------------------------------------------------

#[test]
fn cross_format_mxml_to_ly() {
    let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE score-partwise PUBLIC "-//Recordare//DTD MusicXML 4.0 Partwise//EN" "http://www.musicxml.org/dtds/partwise.dtd">
<score-partwise>
  <part-list><score-part id="P1"><part-name>Part</part-name></score-part></part-list>
  <part id="P1">
    <measure number="1">
      <attributes><divisions>1</divisions>
        <key><fifths>0</fifths></key>
        <time><beats>4</beats><beat-type>4</beat-type></time>
        <clef><sign>G</sign><line>2</line></clef>
      </attributes>
      <note><pitch><step>C</step><octave>4</octave></pitch><duration>1</duration><type>quarter</type></note>
      <note><pitch><step>D</step><octave>4</octave></pitch><duration>1</duration><type>quarter</type></note>
      <note><pitch><step>E</step><octave>4</octave></pitch><duration>1</duration><type>quarter</type></note>
      <note><pitch><step>F</step><octave>4</octave></pitch><duration>1</duration><type>quarter</type></note>
    </measure>
  </part>
</score-partwise>"#;

    // MusicXML → Score
    let score_from_xml = MxmlToIrAdapter::new().convert_str(xml).unwrap();
    let notes_xml = count_notes(&score_from_xml);

    // Score → LilyPond
    let ly = IrToLyAdapter::new().convert(&score_from_xml).unwrap();
    assert!(!ly.is_empty(), "emitted LY should not be empty");

    // LilyPond → Score
    let score_from_ly = LyToIrAdapter::new().convert_str(&ly).unwrap();
    let notes_ly = count_notes(&score_from_ly);

    assert_eq!(
        notes_xml, notes_ly,
        "MusicXML→LY note count: {notes_xml} → {notes_ly}\nLY:\n{ly}"
    );
}

// ---------------------------------------------------------------------------
// Fixture-based round-trips: MXL files
// ---------------------------------------------------------------------------

#[test]
fn mxl_roundtrip_single_voice_fingering() {
    let path = Path::new(
        "tests/fixtures/mxl/2160_single_voice_fingering_repeats_slurm_and_bow_symbols.mxl",
    );
    let score1 = MxmlToIrAdapter::new().convert_file(path).unwrap();
    let xml2 = IrToMxmlAdapter::new().convert(&score1).unwrap();
    let score2 = MxmlToIrAdapter::new().convert_str(&xml2).unwrap();

    let n1 = count_notes(&score1);
    let n2 = count_notes(&score2);
    assert_eq!(n1, n2, "MXL fixture round-trip note count: {n1} → {n2}");
}

#[test]
fn mxl_roundtrip_chords() {
    let path = Path::new("tests/fixtures/mxl/2340_single_voice_with_chords.mxl");
    let score1 = MxmlToIrAdapter::new().convert_file(path).unwrap();
    let xml2 = IrToMxmlAdapter::new().convert(&score1).unwrap();
    let score2 = MxmlToIrAdapter::new().convert_str(&xml2).unwrap();

    let n1 = count_notes(&score1);
    let n2 = count_notes(&score2);
    assert_eq!(n1, n2, "MXL chord fixture round-trip: {n1} → {n2}");
}

#[test]
fn mxl_roundtrip_multi_voice() {
    let path =
        Path::new("tests/fixtures/mxl/2840_score_with_multi_voice_parts_text_and_dynamics.mxl");
    let score1 = MxmlToIrAdapter::new().convert_file(path).unwrap();
    let xml2 = IrToMxmlAdapter::new().convert(&score1).unwrap();
    let score2 = MxmlToIrAdapter::new().convert_str(&xml2).unwrap();

    let p1 = score1.parts().len();
    let p2 = score2.parts().len();
    assert_eq!(p1, p2, "part count should survive: {p1} → {p2}");
}

#[test]
fn mxl_roundtrip_piano_score() {
    let path = Path::new("tests/fixtures/mxl/3840_multi_part_piano_score.mxl");
    let score1 = MxmlToIrAdapter::new().convert_file(path).unwrap();
    let xml2 = IrToMxmlAdapter::new().convert(&score1).unwrap();
    let score2 = MxmlToIrAdapter::new().convert_str(&xml2).unwrap();

    let n1 = count_notes(&score1);
    let n2 = count_notes(&score2);
    assert_eq!(n1, n2, "piano fixture round-trip: {n1} → {n2}");
}

// ---------------------------------------------------------------------------
// Fixture-based round-trips: XML test suite files
// ---------------------------------------------------------------------------

#[test]
fn xml_roundtrip_pitches() {
    let xml = std::fs::read_to_string("tests/fixtures/xml/01a-Pitches-Pitches.xml").unwrap();
    assert_mxml_roundtrip(&xml);
}

#[test]
fn xml_roundtrip_rests() {
    let xml = std::fs::read_to_string("tests/fixtures/xml/02a-Rests-Durations.xml").unwrap();
    assert_mxml_roundtrip(&xml);
}

#[test]
fn xml_roundtrip_rhythm_durations() {
    let xml = std::fs::read_to_string("tests/fixtures/xml/03aa-Rhythm-Durations.xml").unwrap();
    assert_mxml_roundtrip(&xml);
}

#[test]
fn xml_roundtrip_time_signatures() {
    let xml = std::fs::read_to_string("tests/fixtures/xml/11a-TimeSignatures.xml").unwrap();
    assert_mxml_roundtrip(&xml);
}

#[test]
fn xml_roundtrip_key_signatures() {
    let xml = std::fs::read_to_string("tests/fixtures/xml/13a-KeySignatures.xml").unwrap();
    assert_mxml_roundtrip(&xml);
}

#[test]
fn xml_roundtrip_basic_chord() {
    let xml = std::fs::read_to_string("tests/fixtures/xml/21a-Chord-Basic.xml").unwrap();
    assert_mxml_roundtrip(&xml);
}

#[test]
fn xml_roundtrip_tuplets() {
    let xml = std::fs::read_to_string("tests/fixtures/xml/23a-Tuplets.xml").unwrap();
    assert_mxml_roundtrip(&xml);
}

#[test]
fn xml_roundtrip_grace_notes() {
    let xml = std::fs::read_to_string("tests/fixtures/xml/24a-GraceNotes.xml").unwrap();
    assert_mxml_roundtrip(&xml);
}

#[test]
fn xml_roundtrip_multi_parts() {
    let xml = std::fs::read_to_string("tests/fixtures/xml/41a-MultiParts-Partorder.xml").unwrap();
    assert_mxml_roundtrip(&xml);
}

#[test]
fn xml_roundtrip_repeats() {
    let xml = std::fs::read_to_string("tests/fixtures/xml/45a-SimpleRepeat.xml").unwrap();
    assert_mxml_roundtrip(&xml);
}

#[test]
fn xml_roundtrip_barlines() {
    let xml = std::fs::read_to_string("tests/fixtures/xml/46a-Barlines.xml").unwrap();
    assert_mxml_roundtrip(&xml);
}

#[test]
fn xml_roundtrip_lyrics() {
    let xml = std::fs::read_to_string("tests/fixtures/xml/61a-Lyrics.xml").unwrap();
    assert_mxml_roundtrip(&xml);
}

#[test]
fn xml_roundtrip_figured_bass() {
    let xml = std::fs::read_to_string("tests/fixtures/xml/74a-FiguredBass.xml").unwrap();
    assert_mxml_roundtrip(&xml);
}
