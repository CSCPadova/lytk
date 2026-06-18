//! MIDI instrument preservation across formats.
//!
//! A part's instrument identity (the GM name and/or program) must survive
//! conversion between LilyPond, MusicXML and MIDI. The formats carry it
//! differently — LilyPond a name, MIDI a program, MusicXML both — so the shared
//! GM table (`adapters::gm`) cross-fills the missing half at each boundary.

use _core::adapters::ir_to_ly::IrToLyAdapter;
use _core::adapters::ir_to_midi::IrToMidiAdapter;
use _core::adapters::ir_to_mxml::IrToMxmlAdapter;
use _core::adapters::ly_to_ir::LyToIrAdapter;
use _core::adapters::midi_to_ir::MidiToIrAdapter;
use _core::adapters::mxml_to_ir::MxmlToIrAdapter;
use _core::adapters::{FromIrAdapter, ToIrAdapter};
use _core::ir::score::Score;

const LY: &str = r#"\version "2.24.0"
\score {
  \new Staff {
    \set Staff.midiInstrument = "violin"
    c'4 d'4 e'4 f'4
  }
  \layout { }
  \midi { }
}
"#;

fn parse_ly(src: &str) -> Score {
    LyToIrAdapter::new().convert_str(src).expect("LY → IR")
}

#[test]
fn ly_to_musicxml_keeps_name_and_program() {
    // The original bug: ly → musicxml dropped the MIDI instrument. The XML must
    // now carry both the GM name and a (1-indexed) midi-program.
    let score = parse_ly(LY);
    let xml = IrToMxmlAdapter::new().convert(&score).expect("IR → XML");
    assert!(
        xml.contains("<midi-name>violin</midi-name>"),
        "no midi-name:\n{xml}"
    );
    // Violin = MIDI program 40 (0-indexed) = MusicXML midi-program 41 (1-indexed).
    assert!(
        xml.contains("<midi-program>41</midi-program>"),
        "no/ wrong midi-program:\n{xml}"
    );
    // A real display name, not the generic "Instrument".
    assert!(
        xml.contains("<instrument-name>Violin</instrument-name>"),
        "no display name:\n{xml}"
    );
}

#[test]
fn ly_to_musicxml_to_ly_round_trips_instrument() {
    let score = parse_ly(LY);
    let xml = IrToMxmlAdapter::new().convert(&score).expect("IR → XML");
    let score2 = MxmlToIrAdapter::new().convert_str(&xml).expect("XML → IR");
    let part = &score2.parts()[0];
    assert_eq!(part.midi_instrument, "violin");
    assert_eq!(part.midi_program, 40); // 0-indexed
    let ly = IrToLyAdapter::new().convert(&score2).expect("IR → LY");
    assert!(
        ly.contains(r#"\set Staff.midiInstrument = "violin""#),
        "instrument lost on round-trip:\n{ly}"
    );
}

#[test]
fn ly_to_midi_to_ir_recovers_instrument_name() {
    // MIDI carries only the program number; the GM name must be recovered so the
    // instrument survives onward to LilyPond / MusicXML.
    let score = parse_ly(LY);
    let bytes = IrToMidiAdapter::new()
        .convert_bytes(&score)
        .expect("IR → MIDI");
    let score2 = MidiToIrAdapter::new()
        .convert_bytes(&bytes)
        .expect("MIDI → IR");
    let part = &score2.parts()[0];
    assert_eq!(part.midi_program, 40, "violin program lost");
    assert_eq!(
        part.midi_instrument, "violin",
        "name not recovered from program"
    );
}

#[test]
fn ly_to_midi_to_musicxml_keeps_program() {
    // Full cross-format chain: LY (name) → MIDI (program) → MusicXML (both).
    let score = parse_ly(LY);
    let bytes = IrToMidiAdapter::new()
        .convert_bytes(&score)
        .expect("IR → MIDI");
    let score2 = MidiToIrAdapter::new()
        .convert_bytes(&bytes)
        .expect("MIDI → IR");
    let xml = IrToMxmlAdapter::new().convert(&score2).expect("IR → XML");
    assert!(
        xml.contains("<midi-name>violin</midi-name>"),
        "name lost:\n{xml}"
    );
    assert!(
        xml.contains("<midi-program>41</midi-program>"),
        "program lost / off-by-one:\n{xml}"
    );
}

#[test]
fn musicxml_program_only_survives_to_lilypond() {
    // A MusicXML part with only a midi-program (no midi-name) → LilyPond name.
    let xml = r#"<?xml version="1.0"?>
<score-partwise>
  <part-list>
    <score-part id="P1">
      <part-name>P1</part-name>
      <midi-instrument id="P1-I1"><midi-program>57</midi-program></midi-instrument>
    </score-part>
  </part-list>
  <part id="P1">
<measure number="1"><attributes><divisions>4</divisions></attributes>
<note><pitch><step>C</step><octave>4</octave></pitch>
  <duration>4</duration><voice>1</voice><type>quarter</type></note></measure>
  </part>
</score-partwise>"#;
    let score = MxmlToIrAdapter::new().convert_str(xml).expect("XML → IR");
    // MusicXML 57 (1-indexed) = 56 (0-indexed) = trumpet.
    assert_eq!(score.parts()[0].midi_program, 56);
    let ly = IrToLyAdapter::new().convert(&score).expect("IR → LY");
    assert!(
        ly.contains(r#"\set Staff.midiInstrument = "trumpet""#),
        "trumpet not recovered:\n{ly}"
    );
}
