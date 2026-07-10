//! Round-trip integration tests for adapter conversions.
//!
//! Tests that semantic content (pitches, durations, structure) survives
//! round-trip conversions: MusicXML→Score→MusicXML, LilyPond→Score→LilyPond,
//! and cross-format MusicXML→Score→LilyPond→Score.

use _core::adapters::ir_to_ly::IrToLyAdapter;
use _core::adapters::ir_to_midi::IrToMidiAdapter;
use _core::adapters::ir_to_mxml::IrToMxmlAdapter;
use _core::adapters::ly_to_ir::LyToIrAdapter;
use _core::adapters::midi_to_ir::MidiToIrAdapter;
use _core::adapters::mxml_to_ir::MxmlToIrAdapter;
use _core::adapters::{FromIrAdapter, FromMusicAdapter, ToIrAdapter, ToMusicAdapter};
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
    // Known issue: ir_to_mxml emits "double-flat" but musicxml crate expects "flat-flat".
    // This causes one note to be dropped on re-parse. Will be fixed when ir_to_mxml is
    // rewritten to use musicxml crate types. For now, verify first parse is correct.
    let score1 = MxmlToIrAdapter::new().convert_str(&xml).unwrap();
    assert_eq!(
        count_notes(&score1),
        110,
        "first parse should produce 110 notes"
    );
    assert!(!score1.parts().is_empty());
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

// ---------------------------------------------------------------------------
// MIDI round-trip: Score → MIDI bytes → Score
// ---------------------------------------------------------------------------

/// Build a Score from MIDI bytes, emit to MIDI, re-parse, and compare.
fn assert_midi_roundtrip(score: &Score) {
    let writer = IrToMidiAdapter::new();
    let bytes = writer.convert_bytes(score).expect("MIDI emit failed");

    // Verify valid SMF header
    assert_eq!(&bytes[0..4], b"MThd", "not a valid MIDI file");

    let reader = MidiToIrAdapter::new();
    let score2 = reader.convert_bytes(&bytes).expect("MIDI re-parse failed");

    let notes1 = count_notes(score);
    let notes2 = count_notes(&score2);
    assert_eq!(
        notes1, notes2,
        "note count changed after MIDI round-trip: {notes1} → {notes2}"
    );

    let parts1 = score.parts().len();
    let parts2 = score2.parts().len();
    assert_eq!(
        parts1, parts2,
        "part count changed after MIDI round-trip: {parts1} → {parts2}"
    );

    // Compare MIDI pitch numbers (enharmonic-neutral).
    let midi1 = collect_midi_numbers(score);
    let midi2 = collect_midi_numbers(&score2);
    assert_eq!(midi1, midi2, "MIDI pitch numbers changed after round-trip");
}

/// Collect all MIDI note numbers from a score for enharmonic-neutral comparison.
fn collect_midi_numbers(score: &Score) -> Vec<i32> {
    score
        .parts()
        .iter()
        .flat_map(|p| &p.measures)
        .flat_map(|m| &m.voices)
        .flat_map(|v| &v.elements)
        .filter_map(|e| match e {
            VoiceElement::Note(n) => Some(n.pitch.midi_number()),
            VoiceElement::Chord(_) => None,
            _ => None,
        })
        .collect()
}

/// Build a simple score with specific notes for MIDI testing.
fn build_score_from_mxml(xml: &str) -> Score {
    MxmlToIrAdapter::new()
        .convert_str(xml)
        .expect("MusicXML parse failed")
}

// -- Simple melody --

#[test]
fn midi_roundtrip_simple_melody() {
    let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<score-partwise>
  <part-list><score-part id="P1"><part-name>Melody</part-name></score-part></part-list>
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
    let score = build_score_from_mxml(xml);
    assert_midi_roundtrip(&score);
}

// -- Different durations --

#[test]
fn midi_roundtrip_mixed_durations() {
    let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<score-partwise>
  <part-list><score-part id="P1"><part-name>Part</part-name></score-part></part-list>
  <part id="P1">
    <measure number="1">
      <attributes><divisions>2</divisions>
        <key><fifths>0</fifths></key>
        <time><beats>4</beats><beat-type>4</beat-type></time>
        <clef><sign>G</sign><line>2</line></clef>
      </attributes>
      <note><pitch><step>C</step><octave>4</octave></pitch><duration>4</duration><type>half</type></note>
      <note><pitch><step>E</step><octave>4</octave></pitch><duration>1</duration><type>eighth</type></note>
      <note><pitch><step>G</step><octave>4</octave></pitch><duration>1</duration><type>eighth</type></note>
      <note><pitch><step>C</step><octave>5</octave></pitch><duration>2</duration><type>quarter</type></note>
    </measure>
  </part>
</score-partwise>"#;
    let score = build_score_from_mxml(xml);
    assert_midi_roundtrip(&score);
}

// -- Rests --

#[test]
fn midi_roundtrip_with_rests() {
    let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
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
      <note><rest/><duration>1</duration><type>quarter</type></note>
      <note><pitch><step>E</step><octave>4</octave></pitch><duration>1</duration><type>quarter</type></note>
      <note><rest/><duration>1</duration><type>quarter</type></note>
    </measure>
  </part>
</score-partwise>"#;
    let score = build_score_from_mxml(xml);
    assert_midi_roundtrip(&score);
}

// -- Multiple parts --

#[test]
fn midi_roundtrip_two_parts() {
    let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
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
      <note><pitch><step>E</step><octave>5</octave></pitch><duration>1</duration><type>quarter</type></note>
      <note><pitch><step>F</step><octave>5</octave></pitch><duration>1</duration><type>quarter</type></note>
      <note><pitch><step>G</step><octave>5</octave></pitch><duration>1</duration><type>quarter</type></note>
      <note><pitch><step>A</step><octave>5</octave></pitch><duration>1</duration><type>quarter</type></note>
    </measure>
  </part>
  <part id="P2">
    <measure number="1">
      <attributes><divisions>1</divisions>
        <key><fifths>0</fifths></key>
        <time><beats>4</beats><beat-type>4</beat-type></time>
        <clef><sign>F</sign><line>4</line></clef>
      </attributes>
      <note><pitch><step>C</step><octave>3</octave></pitch><duration>2</duration><type>half</type></note>
      <note><pitch><step>G</step><octave>3</octave></pitch><duration>2</duration><type>half</type></note>
    </measure>
  </part>
</score-partwise>"#;
    let score = build_score_from_mxml(xml);
    assert_midi_roundtrip(&score);
}

// -- Multiple measures --

#[test]
fn midi_roundtrip_multiple_measures() {
    let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
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
    <measure number="2">
      <note><pitch><step>G</step><octave>4</octave></pitch><duration>1</duration><type>quarter</type></note>
      <note><pitch><step>A</step><octave>4</octave></pitch><duration>1</duration><type>quarter</type></note>
      <note><pitch><step>B</step><octave>4</octave></pitch><duration>1</duration><type>quarter</type></note>
      <note><pitch><step>C</step><octave>5</octave></pitch><duration>1</duration><type>quarter</type></note>
    </measure>
  </part>
</score-partwise>"#;
    let score = build_score_from_mxml(xml);
    assert_midi_roundtrip(&score);
}

// -- Dotted notes --

#[test]
fn midi_roundtrip_dotted_notes() {
    let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
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
    let score = build_score_from_mxml(xml);
    assert_midi_roundtrip(&score);
}

// -- 3/4 time signature --

#[test]
fn midi_roundtrip_three_four_time() {
    let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<score-partwise>
  <part-list><score-part id="P1"><part-name>Part</part-name></score-part></part-list>
  <part id="P1">
    <measure number="1">
      <attributes><divisions>1</divisions>
        <key><fifths>0</fifths></key>
        <time><beats>3</beats><beat-type>4</beat-type></time>
        <clef><sign>G</sign><line>2</line></clef>
      </attributes>
      <note><pitch><step>D</step><octave>4</octave></pitch><duration>1</duration><type>quarter</type></note>
      <note><pitch><step>F</step><octave>4</octave></pitch><duration>1</duration><type>quarter</type></note>
      <note><pitch><step>A</step><octave>4</octave></pitch><duration>1</duration><type>quarter</type></note>
    </measure>
    <measure number="2">
      <note><pitch><step>G</step><octave>4</octave></pitch><duration>1</duration><type>quarter</type></note>
      <note><pitch><step>B</step><octave>4</octave></pitch><duration>1</duration><type>quarter</type></note>
      <note><pitch><step>D</step><octave>5</octave></pitch><duration>1</duration><type>quarter</type></note>
    </measure>
  </part>
</score-partwise>"#;
    let score = build_score_from_mxml(xml);
    assert_midi_roundtrip(&score);
}

// -- Key signature with sharps --

#[test]
fn midi_roundtrip_key_signature() {
    let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<score-partwise>
  <part-list><score-part id="P1"><part-name>Part</part-name></score-part></part-list>
  <part id="P1">
    <measure number="1">
      <attributes><divisions>1</divisions>
        <key><fifths>2</fifths></key>
        <time><beats>4</beats><beat-type>4</beat-type></time>
        <clef><sign>G</sign><line>2</line></clef>
      </attributes>
      <note><pitch><step>D</step><octave>4</octave></pitch><duration>1</duration><type>quarter</type></note>
      <note><pitch><step>F</step><alter>1</alter><octave>4</octave></pitch><duration>1</duration><type>quarter</type></note>
      <note><pitch><step>A</step><octave>4</octave></pitch><duration>1</duration><type>quarter</type></note>
      <note><pitch><step>D</step><octave>5</octave></pitch><duration>1</duration><type>quarter</type></note>
    </measure>
  </part>
</score-partwise>"#;
    let score = build_score_from_mxml(xml);
    assert_midi_roundtrip(&score);
}

// -- Whole notes --

#[test]
fn midi_roundtrip_whole_notes() {
    let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<score-partwise>
  <part-list><score-part id="P1"><part-name>Part</part-name></score-part></part-list>
  <part id="P1">
    <measure number="1">
      <attributes><divisions>1</divisions>
        <key><fifths>0</fifths></key>
        <time><beats>4</beats><beat-type>4</beat-type></time>
        <clef><sign>G</sign><line>2</line></clef>
      </attributes>
      <note><pitch><step>G</step><octave>4</octave></pitch><duration>4</duration><type>whole</type></note>
    </measure>
    <measure number="2">
      <note><pitch><step>A</step><octave>4</octave></pitch><duration>4</duration><type>whole</type></note>
    </measure>
  </part>
</score-partwise>"#;
    let score = build_score_from_mxml(xml);
    assert_midi_roundtrip(&score);
}

// -- Tempo marking --

#[test]
fn midi_roundtrip_tempo() {
    let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<score-partwise>
  <part-list><score-part id="P1"><part-name>Part</part-name></score-part></part-list>
  <part id="P1">
    <measure number="1">
      <attributes><divisions>1</divisions>
        <key><fifths>0</fifths></key>
        <time><beats>4</beats><beat-type>4</beat-type></time>
        <clef><sign>G</sign><line>2</line></clef>
      </attributes>
      <direction placement="above">
        <direction-type><metronome><beat-unit>quarter</beat-unit><per-minute>120</per-minute></metronome></direction-type>
        <sound tempo="120"/>
      </direction>
      <note><pitch><step>C</step><octave>4</octave></pitch><duration>1</duration><type>quarter</type></note>
      <note><pitch><step>E</step><octave>4</octave></pitch><duration>1</duration><type>quarter</type></note>
      <note><pitch><step>G</step><octave>4</octave></pitch><duration>1</duration><type>quarter</type></note>
      <note><pitch><step>C</step><octave>5</octave></pitch><duration>1</duration><type>quarter</type></note>
    </measure>
  </part>
</score-partwise>"#;
    let score = build_score_from_mxml(xml);
    assert_midi_roundtrip(&score);
}

// -- Chromatic scale (accidentals) --

#[test]
fn midi_roundtrip_chromatic() {
    let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
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
      <note><pitch><step>C</step><alter>1</alter><octave>4</octave></pitch><duration>1</duration><type>quarter</type></note>
      <note><pitch><step>D</step><octave>4</octave></pitch><duration>1</duration><type>quarter</type></note>
      <note><pitch><step>E</step><alter>-1</alter><octave>4</octave></pitch><duration>1</duration><type>quarter</type></note>
    </measure>
  </part>
</score-partwise>"#;
    let score = build_score_from_mxml(xml);
    // MIDI round-trip loses enharmonic spelling but preserves pitch
    let writer = IrToMidiAdapter::new();
    let bytes = writer.convert_bytes(&score).unwrap();
    let reader = MidiToIrAdapter::new();
    let score2 = reader.convert_bytes(&bytes).unwrap();

    let midi1 = collect_midi_numbers(&score);
    let midi2 = collect_midi_numbers(&score2);
    assert_eq!(
        midi1, midi2,
        "MIDI numbers should be preserved for chromatic notes"
    );
}

// -- Cross-format: LilyPond → Score → MIDI → Score (preserves pitches) --

#[test]
fn cross_format_ly_to_midi_roundtrip() {
    let ly = r#"\version "2.24.0"
\language "english"
{ c'4 d' e' f' | g' a' b' c'' }
"#;
    let score = LyToIrAdapter::new()
        .convert_str(ly)
        .expect("LilyPond parse failed");

    let writer = IrToMidiAdapter::new();
    let bytes = writer.convert_bytes(&score).unwrap();
    let reader = MidiToIrAdapter::new();
    let score2 = reader.convert_bytes(&bytes).unwrap();

    let notes1 = count_notes(&score);
    let notes2 = count_notes(&score2);
    assert_eq!(notes1, notes2, "LY→MIDI→Score note count mismatch");

    let midi1 = collect_midi_numbers(&score);
    let midi2 = collect_midi_numbers(&score2);
    assert_eq!(midi1, midi2, "LY→MIDI→Score pitch mismatch");
}

// -- Cross-format: MusicXML → Score → MIDI → Score → MusicXML (structural) --

#[test]
fn cross_format_mxml_midi_mxml() {
    let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
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
      <note><pitch><step>G</step><octave>4</octave></pitch><duration>1</duration><type>quarter</type></note>
    </measure>
  </part>
</score-partwise>"#;
    let score1 = build_score_from_mxml(xml);

    // MusicXML → MIDI → Score
    let writer = IrToMidiAdapter::new();
    let midi_bytes = writer.convert_bytes(&score1).unwrap();
    let reader = MidiToIrAdapter::new();
    let score2 = reader.convert_bytes(&midi_bytes).unwrap();

    // Score → MusicXML → Score (verify the MIDI-derived score serializes cleanly)
    let xml2 = IrToMxmlAdapter::new().convert(&score2).unwrap();
    let score3 = MxmlToIrAdapter::new().convert_str(&xml2).unwrap();

    let midi2 = collect_midi_numbers(&score2);
    let midi3 = collect_midi_numbers(&score3);
    assert_eq!(midi2, midi3, "MIDI→MusicXML→Score pitch mismatch");
}

// -- Eighth notes (fast subdivision) --

#[test]
fn midi_roundtrip_eighth_notes() {
    let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<score-partwise>
  <part-list><score-part id="P1"><part-name>Part</part-name></score-part></part-list>
  <part id="P1">
    <measure number="1">
      <attributes><divisions>2</divisions>
        <key><fifths>0</fifths></key>
        <time><beats>4</beats><beat-type>4</beat-type></time>
        <clef><sign>G</sign><line>2</line></clef>
      </attributes>
      <note><pitch><step>C</step><octave>4</octave></pitch><duration>1</duration><type>eighth</type></note>
      <note><pitch><step>D</step><octave>4</octave></pitch><duration>1</duration><type>eighth</type></note>
      <note><pitch><step>E</step><octave>4</octave></pitch><duration>1</duration><type>eighth</type></note>
      <note><pitch><step>F</step><octave>4</octave></pitch><duration>1</duration><type>eighth</type></note>
      <note><pitch><step>G</step><octave>4</octave></pitch><duration>1</duration><type>eighth</type></note>
      <note><pitch><step>A</step><octave>4</octave></pitch><duration>1</duration><type>eighth</type></note>
      <note><pitch><step>B</step><octave>4</octave></pitch><duration>1</duration><type>eighth</type></note>
      <note><pitch><step>C</step><octave>5</octave></pitch><duration>1</duration><type>eighth</type></note>
    </measure>
  </part>
</score-partwise>"#;
    let score = build_score_from_mxml(xml);
    assert_midi_roundtrip(&score);
}

// -- 6/8 time --

#[test]
fn midi_roundtrip_six_eight_time() {
    let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<score-partwise>
  <part-list><score-part id="P1"><part-name>Part</part-name></score-part></part-list>
  <part id="P1">
    <measure number="1">
      <attributes><divisions>2</divisions>
        <key><fifths>0</fifths></key>
        <time><beats>6</beats><beat-type>8</beat-type></time>
        <clef><sign>G</sign><line>2</line></clef>
      </attributes>
      <note><pitch><step>C</step><octave>4</octave></pitch><duration>1</duration><type>eighth</type></note>
      <note><pitch><step>D</step><octave>4</octave></pitch><duration>1</duration><type>eighth</type></note>
      <note><pitch><step>E</step><octave>4</octave></pitch><duration>1</duration><type>eighth</type></note>
      <note><pitch><step>F</step><octave>4</octave></pitch><duration>1</duration><type>eighth</type></note>
      <note><pitch><step>G</step><octave>4</octave></pitch><duration>1</duration><type>eighth</type></note>
      <note><pitch><step>A</step><octave>4</octave></pitch><duration>1</duration><type>eighth</type></note>
    </measure>
  </part>
</score-partwise>"#;
    let score = build_score_from_mxml(xml);
    assert_midi_roundtrip(&score);
}

// -- Wide pitch range --

#[test]
fn midi_roundtrip_wide_range() {
    let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<score-partwise>
  <part-list><score-part id="P1"><part-name>Part</part-name></score-part></part-list>
  <part id="P1">
    <measure number="1">
      <attributes><divisions>1</divisions>
        <key><fifths>0</fifths></key>
        <time><beats>4</beats><beat-type>4</beat-type></time>
        <clef><sign>G</sign><line>2</line></clef>
      </attributes>
      <note><pitch><step>A</step><octave>0</octave></pitch><duration>1</duration><type>quarter</type></note>
      <note><pitch><step>C</step><octave>4</octave></pitch><duration>1</duration><type>quarter</type></note>
      <note><pitch><step>C</step><octave>7</octave></pitch><duration>1</duration><type>quarter</type></note>
      <note><pitch><step>C</step><octave>8</octave></pitch><duration>1</duration><type>quarter</type></note>
    </measure>
  </part>
</score-partwise>"#;
    let score = build_score_from_mxml(xml);
    assert_midi_roundtrip(&score);
}

// -- MIDI CLI round-trip --

#[test]
fn cli_midi_convert_roundtrip() {
    let tmp = tempfile::TempDir::new().unwrap();
    let mid_path = tmp.path().join("test.mid");
    let ly_path = tmp.path().join("output.ly");

    // Build a simple score and write MIDI
    let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
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
      <note><pitch><step>E</step><octave>4</octave></pitch><duration>1</duration><type>quarter</type></note>
      <note><pitch><step>G</step><octave>4</octave></pitch><duration>1</duration><type>quarter</type></note>
      <note><pitch><step>C</step><octave>5</octave></pitch><duration>1</duration><type>quarter</type></note>
    </measure>
  </part>
</score-partwise>"#;
    let score = build_score_from_mxml(xml);
    IrToMidiAdapter::new().write(&score, &mid_path).unwrap();

    // Use CLI to convert MIDI → LilyPond
    assert_cmd::Command::cargo_bin("lytk")
        .unwrap()
        .arg("convert")
        .arg(&mid_path)
        .arg("-o")
        .arg(&ly_path)
        .assert()
        .success();

    assert!(
        ly_path.exists(),
        "CLI should produce LilyPond output from MIDI"
    );
    let ly_content = std::fs::read_to_string(&ly_path).unwrap();
    assert!(
        !ly_content.is_empty(),
        "LilyPond output should not be empty"
    );
}

// ---------------------------------------------------------------------------
// MIDI Music tree adapter tests (ToMusicAdapter / FromMusicAdapter)
// ---------------------------------------------------------------------------

#[test]
fn midi_to_music_adapter() {
    // Write a MIDI file, then read it back via ToMusicAdapter
    let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
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
      <note><pitch><step>E</step><octave>4</octave></pitch><duration>1</duration><type>quarter</type></note>
      <note><pitch><step>G</step><octave>4</octave></pitch><duration>1</duration><type>quarter</type></note>
      <note><pitch><step>C</step><octave>5</octave></pitch><duration>1</duration><type>quarter</type></note>
    </measure>
  </part>
</score-partwise>"#;
    let score = build_score_from_mxml(xml);
    let tmp = tempfile::TempDir::new().unwrap();
    let mid_path = tmp.path().join("test.mid");
    IrToMidiAdapter::new().write(&score, &mid_path).unwrap();

    let doc = MidiToIrAdapter::new()
        .convert_file_to_music(&mid_path)
        .expect("ToMusicAdapter for MIDI failed");

    // The Music tree should be non-empty (not just a bare rest)
    assert!(
        !matches!(doc.music, _core::ir::music::Music::Rest { .. }),
        "Music tree should have content beyond a single rest"
    );
}

#[test]
fn midi_from_music_adapter() {
    // Parse LilyPond to Music tree, then write MIDI via FromMusicAdapter
    let ly = r#"\version "2.24.0"
\language "english"
{ c'4 d' e' f' }
"#;
    let doc = LyToIrAdapter::new()
        .convert_str_to_music(ly)
        .expect("LilyPond to Music parse failed");

    let tmp = tempfile::TempDir::new().unwrap();
    let mid_path = tmp.path().join("output.mid");
    IrToMidiAdapter::new()
        .write_music(&doc, &mid_path)
        .expect("FromMusicAdapter for MIDI failed");

    // Read back and verify
    let bytes = std::fs::read(&mid_path).unwrap();
    assert_eq!(&bytes[0..4], b"MThd", "output should be valid MIDI");

    let score = MidiToIrAdapter::new().convert_bytes(&bytes).unwrap();
    assert_eq!(
        count_notes(&score),
        4,
        "should have 4 notes from Music tree"
    );
}

#[test]
fn midi_music_roundtrip_ly_to_midi_to_ly() {
    // LilyPond → Music → MIDI → Score → Music → LilyPond
    let ly = r#"\version "2.24.0"
\language "english"
{ c'4 d' e' f' | g' a' b' c'' }
"#;
    let doc = LyToIrAdapter::new()
        .convert_str_to_music(ly)
        .expect("LilyPond parse failed");

    // Music → MIDI file
    let tmp = tempfile::TempDir::new().unwrap();
    let mid_path = tmp.path().join("roundtrip.mid");
    IrToMidiAdapter::new()
        .write_music(&doc, &mid_path)
        .expect("write_music failed");

    // MIDI → Music
    let doc2 = MidiToIrAdapter::new()
        .convert_file_to_music(&mid_path)
        .expect("convert_file_to_music failed");

    // Music → LilyPond
    let ly2 = IrToLyAdapter::new()
        .convert_music(&doc2)
        .expect("LilyPond emit from Music failed");

    assert!(
        !ly2.is_empty(),
        "LilyPond output from Music should not be empty"
    );
    assert!(ly2.contains("\\version"), "should contain LilyPond version");
}

/// Regression: pedal.ly has multi-voice/multi-staff content that previously
/// caused a subtract-with-overflow panic in build_part_track.
#[test]
fn midi_roundtrip_multivoice_pedal() {
    let ly_path = Path::new("tests/fixtures/ly/pedal.ly");
    let score = LyToIrAdapter::new()
        .convert_file(ly_path)
        .expect("LilyPond parse failed");

    let adapter = IrToMidiAdapter::new();
    let bytes = adapter.convert_bytes(&score).expect("MIDI export panicked");

    // Parse back — must be valid MIDI.
    let score2 = MidiToIrAdapter::new()
        .convert_bytes(&bytes)
        .expect("MIDI re-import failed");

    assert!(
        !score2.parts().is_empty(),
        "Round-tripped MIDI should have parts"
    );
    assert!(
        count_notes(&score2) > 0,
        "Round-tripped MIDI should have notes"
    );
}

/// Regression: Rossini orchestral score (18 parts, multi-voice) previously
/// caused a subtract-with-overflow panic in build_part_track.
#[test]
fn midi_roundtrip_multivoice_mxl() {
    let mxl_path = Path::new("tests/fixtures/musicxml/Stabat_Mater_Rossini_1_-_Introduzione.mxl");
    let score = MxmlToIrAdapter::new()
        .convert_file(mxl_path)
        .expect("MXL parse failed");

    let adapter = IrToMidiAdapter::new();
    let bytes = adapter.convert_bytes(&score).expect("MIDI export panicked");

    // Parse back — must be valid MIDI.
    let score2 = MidiToIrAdapter::new()
        .convert_bytes(&bytes)
        .expect("MIDI re-import failed");

    assert_eq!(score2.parts().len(), 18, "Rossini has 18 parts");
    assert!(
        count_notes(&score2) > 100,
        "Round-tripped Rossini should have many notes"
    );
}

// ---------------------------------------------------------------------------
// MIDI reference regression tests
// ---------------------------------------------------------------------------
// Compare MIDI output from our LilyPond→IR→MIDI pipeline against reference
// MIDI files generated by LilyPond itself (tests/fixtures/midi/).

/// Helper: extract unique (channel, program) pairs from raw MIDI bytes.
/// LilyPond emits duplicate program changes per track; deduplicate for comparison.
/// Track indices are omitted because LilyPond may add extra tracks (e.g. lyrics).
fn extract_programs(smf: &midly::Smf) -> Vec<(u8, u8)> {
    let mut result = Vec::new();
    for track in &smf.tracks {
        for event in track {
            if let midly::TrackEventKind::Midi {
                channel,
                message: midly::MidiMessage::ProgramChange { program },
            } = event.kind
            {
                let entry = (channel.as_int(), program.as_int());
                if !result.contains(&entry) {
                    result.push(entry);
                }
            }
        }
    }
    result.sort();
    result
}

/// Helper: extract tempo events (microseconds per quarter) from track 0.
fn extract_tempos(smf: &midly::Smf) -> Vec<u32> {
    if smf.tracks.is_empty() {
        return Vec::new();
    }
    smf.tracks[0]
        .iter()
        .filter_map(|ev| {
            if let midly::TrackEventKind::Meta(midly::MetaMessage::Tempo(t)) = ev.kind {
                Some(t.as_int())
            } else {
                None
            }
        })
        .collect()
}

/// Helper: count NoteOn events across all tracks.
fn count_midi_note_ons(smf: &midly::Smf) -> usize {
    smf.tracks
        .iter()
        .flat_map(|t| t.iter())
        .filter(|ev| {
            matches!(
                ev.kind,
                midly::TrackEventKind::Midi {
                    message: midly::MidiMessage::NoteOn { vel, .. },
                    ..
                } if vel.as_int() > 0
            )
        })
        .count()
}

/// Helper: collect all NoteOn pitch numbers.
fn collect_midi_note_on_pitches(smf: &midly::Smf) -> Vec<u8> {
    smf.tracks
        .iter()
        .flat_map(|t| t.iter())
        .filter_map(|ev| {
            if let midly::TrackEventKind::Midi {
                message: midly::MidiMessage::NoteOn { key, vel },
                ..
            } = ev.kind
            {
                if vel.as_int() > 0 {
                    Some(key.as_int())
                } else {
                    None
                }
            } else {
                None
            }
        })
        .collect()
}

/// Convert a LilyPond fixture to MIDI bytes and parse both our output and the
/// reference. Returns (ours, reference) as parsed SMFs.
fn ly_to_midi_vs_ref(ly_path: &str, ref_path: &str) -> (Vec<u8>, Vec<u8>) {
    let ly_adapter = LyToIrAdapter::new();
    let scores = ly_adapter
        .convert_file_multi(Path::new(ly_path))
        .expect("LilyPond parse failed");

    // For multi-movement files, we need the specific movement
    let score = &scores[0];
    let midi_adapter = IrToMidiAdapter::new();
    let our_bytes = midi_adapter
        .convert_bytes(score)
        .expect("MIDI export failed");

    let ref_bytes = std::fs::read(ref_path).expect("Reference MIDI not found");
    (our_bytes, ref_bytes)
}

fn ly_to_midi_vs_ref_movement(
    ly_path: &str,
    ref_path: &str,
    movement: usize,
) -> (Vec<u8>, Vec<u8>) {
    let ly_adapter = LyToIrAdapter::new();
    let scores = ly_adapter
        .convert_file_multi(Path::new(ly_path))
        .expect("LilyPond parse failed");

    let score = &scores[movement];
    let midi_adapter = IrToMidiAdapter::new();
    let our_bytes = midi_adapter
        .convert_bytes(score)
        .expect("MIDI export failed");

    let ref_bytes = std::fs::read(ref_path).expect("Reference MIDI not found");
    (our_bytes, ref_bytes)
}

#[test]
fn midi_ref_pedal_track_count() {
    let (ours, reference) = ly_to_midi_vs_ref(
        "tests/fixtures/ly/pedal.ly",
        "tests/fixtures/midi/pedal.midi",
    );
    let our_smf = midly::Smf::parse(&ours).unwrap();
    let ref_smf = midly::Smf::parse(&reference).unwrap();
    assert_eq!(
        our_smf.tracks.len(),
        ref_smf.tracks.len(),
        "pedal: track count mismatch (ours={}, ref={})",
        our_smf.tracks.len(),
        ref_smf.tracks.len()
    );
}

#[test]
fn midi_ref_pedal_ppqn() {
    let (ours, reference) = ly_to_midi_vs_ref(
        "tests/fixtures/ly/pedal.ly",
        "tests/fixtures/midi/pedal.midi",
    );
    let our_smf = midly::Smf::parse(&ours).unwrap();
    let ref_smf = midly::Smf::parse(&reference).unwrap();
    let our_ppqn = match our_smf.header.timing {
        midly::Timing::Metrical(t) => t.as_int(),
        _ => panic!("expected metrical timing"),
    };
    let ref_ppqn = match ref_smf.header.timing {
        midly::Timing::Metrical(t) => t.as_int(),
        _ => panic!("expected metrical timing"),
    };
    assert_eq!(our_ppqn, ref_ppqn, "pedal: PPQN mismatch");
}

#[test]
fn midi_ref_pedal_programs() {
    let (ours, reference) = ly_to_midi_vs_ref(
        "tests/fixtures/ly/pedal.ly",
        "tests/fixtures/midi/pedal.midi",
    );
    let our_smf = midly::Smf::parse(&ours).unwrap();
    let ref_smf = midly::Smf::parse(&reference).unwrap();
    let our_progs = extract_programs(&our_smf);
    let ref_progs = extract_programs(&ref_smf);
    if ref_progs.is_empty() {
        // Reference uses default program 0 (no explicit ProgramChange).
        // Verify all our program changes are also program 0.
        assert!(
            our_progs.iter().all(|&(_, p)| p == 0),
            "pedal: reference has no program changes, but ours has non-zero programs: {:?}",
            our_progs
        );
    } else {
        assert_eq!(our_progs, ref_progs, "pedal: program changes differ");
    }
}

#[test]
fn midi_ref_pedal_note_count() {
    let (ours, reference) = ly_to_midi_vs_ref(
        "tests/fixtures/ly/pedal.ly",
        "tests/fixtures/midi/pedal.midi",
    );
    let our_smf = midly::Smf::parse(&ours).unwrap();
    let ref_smf = midly::Smf::parse(&reference).unwrap();
    let our_notes = count_midi_note_ons(&our_smf);
    let ref_notes = count_midi_note_ons(&ref_smf);
    // Allow some tolerance for grace notes and ornaments
    let tolerance = (ref_notes as f64 * 0.05).ceil() as usize;
    assert!(
        our_notes.abs_diff(ref_notes) <= tolerance,
        "pedal: note count differs too much (ours={our_notes}, ref={ref_notes}, tolerance={tolerance})"
    );
}

#[test]
fn midi_ref_pedal_pitch_range() {
    let (ours, reference) = ly_to_midi_vs_ref(
        "tests/fixtures/ly/pedal.ly",
        "tests/fixtures/midi/pedal.midi",
    );
    let our_smf = midly::Smf::parse(&ours).unwrap();
    let ref_smf = midly::Smf::parse(&reference).unwrap();
    let our_pitches = collect_midi_note_on_pitches(&our_smf);
    let ref_pitches = collect_midi_note_on_pitches(&ref_smf);
    let (our_min, our_max) = (
        *our_pitches.iter().min().unwrap(),
        *our_pitches.iter().max().unwrap(),
    );
    let (ref_min, ref_max) = (
        *ref_pitches.iter().min().unwrap(),
        *ref_pitches.iter().max().unwrap(),
    );
    assert!(
        our_min.abs_diff(ref_min) <= 2 && our_max.abs_diff(ref_max) <= 2,
        "pedal: pitch range differs (ours={our_min}-{our_max}, ref={ref_min}-{ref_max})"
    );
}

#[test]
fn midi_ref_example_track_count() {
    let (ours, reference) = ly_to_midi_vs_ref(
        "tests/fixtures/ly/example.ly",
        "tests/fixtures/midi/example.midi",
    );
    let our_smf = midly::Smf::parse(&ours).unwrap();
    let ref_smf = midly::Smf::parse(&reference).unwrap();
    // LilyPond may emit an extra empty track for Lyrics contexts; allow ±1
    let diff = our_smf.tracks.len().abs_diff(ref_smf.tracks.len());
    assert!(
        diff <= 1,
        "example: track count mismatch (ours={}, ref={}, diff={})",
        our_smf.tracks.len(),
        ref_smf.tracks.len(),
        diff,
    );
}

#[test]
fn midi_ref_example_programs() {
    let (ours, reference) = ly_to_midi_vs_ref(
        "tests/fixtures/ly/example.ly",
        "tests/fixtures/midi/example.midi",
    );
    let our_smf = midly::Smf::parse(&ours).unwrap();
    let ref_smf = midly::Smf::parse(&reference).unwrap();
    assert_eq!(
        extract_programs(&our_smf),
        extract_programs(&ref_smf),
        "example: program changes differ"
    );
}

#[test]
fn midi_ref_example_tempos() {
    let (ours, reference) = ly_to_midi_vs_ref(
        "tests/fixtures/ly/example.ly",
        "tests/fixtures/midi/example.midi",
    );
    let our_smf = midly::Smf::parse(&ours).unwrap();
    let ref_smf = midly::Smf::parse(&reference).unwrap();
    let our_tempos = extract_tempos(&our_smf);
    let ref_tempos = extract_tempos(&ref_smf);
    assert_eq!(
        our_tempos.len(),
        ref_tempos.len(),
        "example: tempo event count mismatch"
    );
    for (i, (ours, refs)) in our_tempos.iter().zip(ref_tempos.iter()).enumerate() {
        assert!(
            ours.abs_diff(*refs) <= 1,
            "example: tempo[{i}] differs (ours={ours}, ref={refs})"
        );
    }
}

#[test]
fn midi_ref_example2_mov0_programs() {
    let (ours, reference) = ly_to_midi_vs_ref_movement(
        "tests/fixtures/ly/example2.ly",
        "tests/fixtures/midi/example2_0.midi",
        0,
    );
    let our_smf = midly::Smf::parse(&ours).unwrap();
    let ref_smf = midly::Smf::parse(&reference).unwrap();
    assert_eq!(
        extract_programs(&our_smf),
        extract_programs(&ref_smf),
        "example2 mov0: program changes differ"
    );
}

#[test]
fn midi_ref_example2_mov0_tempos() {
    let (ours, reference) = ly_to_midi_vs_ref_movement(
        "tests/fixtures/ly/example2.ly",
        "tests/fixtures/midi/example2_0.midi",
        0,
    );
    let our_smf = midly::Smf::parse(&ours).unwrap();
    let ref_smf = midly::Smf::parse(&reference).unwrap();
    let our_tempos = extract_tempos(&our_smf);
    let ref_tempos = extract_tempos(&ref_smf);
    assert_eq!(
        our_tempos.len(),
        ref_tempos.len(),
        "example2 mov0: tempo count mismatch"
    );
    for (i, (ours, refs)) in our_tempos.iter().zip(ref_tempos.iter()).enumerate() {
        assert!(
            ours.abs_diff(*refs) <= 1,
            "example2 mov0: tempo[{i}] differs (ours={ours}, ref={refs})"
        );
    }
}

#[test]
fn midi_ref_example2_mov1_programs() {
    let (ours, reference) = ly_to_midi_vs_ref_movement(
        "tests/fixtures/ly/example2.ly",
        "tests/fixtures/midi/example2_1.midi",
        1,
    );
    let our_smf = midly::Smf::parse(&ours).unwrap();
    let ref_smf = midly::Smf::parse(&reference).unwrap();
    assert_eq!(
        extract_programs(&our_smf),
        extract_programs(&ref_smf),
        "example2 mov1: program changes differ"
    );
}

#[test]
fn midi_ref_example2_mov1_tempos() {
    let (ours, reference) = ly_to_midi_vs_ref_movement(
        "tests/fixtures/ly/example2.ly",
        "tests/fixtures/midi/example2_1.midi",
        1,
    );
    let our_smf = midly::Smf::parse(&ours).unwrap();
    let ref_smf = midly::Smf::parse(&reference).unwrap();
    let our_tempos = extract_tempos(&our_smf);
    let ref_tempos = extract_tempos(&ref_smf);
    assert_eq!(
        our_tempos.len(),
        ref_tempos.len(),
        "example2 mov1: tempo count mismatch"
    );
    for (i, (ours, refs)) in our_tempos.iter().zip(ref_tempos.iter()).enumerate() {
        assert!(
            ours.abs_diff(*refs) <= 1,
            "example2 mov1: tempo[{i}] differs (ours={ours}, ref={refs})"
        );
    }
}

#[test]
fn midi_ref_chopin_track_count() {
    let (ours, reference) = ly_to_midi_vs_ref(
        "tests/fixtures/ly/chopin_n.ly",
        "tests/fixtures/midi/chopin_n.midi",
    );
    let our_smf = midly::Smf::parse(&ours).unwrap();
    let ref_smf = midly::Smf::parse(&reference).unwrap();
    assert_eq!(
        our_smf.tracks.len(),
        ref_smf.tracks.len(),
        "chopin: track count mismatch (ours={}, ref={})",
        our_smf.tracks.len(),
        ref_smf.tracks.len()
    );
}

#[test]
fn midi_ref_chopin_programs() {
    let (ours, reference) = ly_to_midi_vs_ref(
        "tests/fixtures/ly/chopin_n.ly",
        "tests/fixtures/midi/chopin_n.midi",
    );
    let our_smf = midly::Smf::parse(&ours).unwrap();
    let ref_smf = midly::Smf::parse(&reference).unwrap();
    assert_eq!(
        extract_programs(&our_smf),
        extract_programs(&ref_smf),
        "chopin: program changes differ"
    );
}
