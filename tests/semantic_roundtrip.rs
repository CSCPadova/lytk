//! Semantic round-trip tests (Epic C).
//!
//! Unlike `fixture_regression.rs` (asserts "non-empty") and `fixture_audit.rs`
//! (asserts "no panic"), these assert that *meaningful musical content* survives
//! a conversion: pitches, durations, dynamics, articulations, ties/slurs,
//! lyrics, and repeat structure.
//!
//! Helpers here are shared building blocks for the per-fixture round-trip suite.

use _core::adapters::ir_to_ly::IrToLyAdapter;
use _core::adapters::ly_to_ir::LyToIrAdapter;
use _core::adapters::mxml_to_ir::MxmlToIrAdapter;
use _core::adapters::{FromIrAdapter, FromMusicAdapter, ToIrAdapter, ToMusicAdapter};

// ---------------------------------------------------------------------------
// Conversion shortcuts
// ---------------------------------------------------------------------------

/// LilyPond → LilyPond via the Music-tree path (the path the CLI uses for LY→LY).
fn ly_to_ly_music(src: &str) -> String {
    let doc = LyToIrAdapter::new()
        .convert_str_to_music(src)
        .expect("LY → Music");
    IrToLyAdapter::new()
        .convert_music(&doc)
        .expect("Music → LY")
}

/// LilyPond → LilyPond via the Score path.
fn ly_to_ly_score(src: &str) -> String {
    let score = LyToIrAdapter::new().convert_str(src).expect("LY → Score");
    IrToLyAdapter::new().convert(&score).expect("Score → LY")
}

/// MusicXML → LilyPond via the Score path.
fn xml_to_ly(src: &str) -> String {
    let score = MxmlToIrAdapter::new()
        .convert_str(src)
        .expect("XML → Score");
    IrToLyAdapter::new().convert(&score).expect("Score → LY")
}

fn read_ly(name: &str) -> String {
    std::fs::read_to_string(format!("tests/fixtures/ly/{name}")).expect("read ly fixture")
}

fn read_xml(name: &str) -> String {
    std::fs::read_to_string(format!("tests/fixtures/xml/{name}")).expect("read xml fixture")
}

/// True if the emitted LilyPond contains a lyrics block with actual syllables.
fn has_lyric_content(ly: &str) -> bool {
    ly.contains("\\lyricmode") || ly.contains("\\lyricsto") || ly.contains("\\addlyrics")
}

// ---------------------------------------------------------------------------
// Lyrics (B1)
// ---------------------------------------------------------------------------

#[test]
fn xml_to_ly_preserves_lyrics() {
    let ly = xml_to_ly(&read_xml("61a-Lyrics.xml"));
    assert!(
        has_lyric_content(&ly),
        "XML→LY dropped lyrics; output had no lyric block:\n{ly}"
    );
}

/// Lyrics defined via `\lyricmode` / `\lyricsto` must survive LY→LY on both paths.
/// `example.ly` uses this form (parsed today); the bug was the emitter dropping them.
#[test]
fn ly_to_ly_music_preserves_lyrics_lyricmode() {
    let ly = ly_to_ly_music(&read_ly("example.ly"));
    assert!(
        has_lyric_content(&ly),
        "LY→LY (Music path) dropped \\lyricmode lyrics"
    );
}

#[test]
fn ly_to_ly_score_preserves_lyrics_lyricmode() {
    let ly = ly_to_ly_score(&read_ly("example.ly"));
    assert!(
        has_lyric_content(&ly),
        "LY→LY (Score path) dropped \\lyricmode lyrics"
    );
}

/// `\addlyrics` (postfix form) must be parsed and re-emitted. Use a `\layout`
/// block so the score is not treated as MIDI-only (which would be skipped).
const ADDLYRICS_SRC: &str = r#"\version "2.24.0"
\score {
  \new Staff { c'4 d'4 e'4 f'4 }
  \addlyrics { these are the words }
  \layout { }
}
"#;

#[test]
fn ly_to_ly_music_preserves_addlyrics() {
    let ly = ly_to_ly_music(ADDLYRICS_SRC);
    assert!(
        has_lyric_content(&ly) && ly.contains("words"),
        "LY→LY (Music path) dropped \\addlyrics; output:\n{ly}"
    );
}

#[test]
fn ly_to_ly_score_preserves_addlyrics() {
    let ly = ly_to_ly_score(ADDLYRICS_SRC);
    assert!(
        has_lyric_content(&ly) && ly.contains("words"),
        "LY→LY (Score path) dropped \\addlyrics; output:\n{ly}"
    );
}

// ---------------------------------------------------------------------------
// Repeats / voltas (B2)
// ---------------------------------------------------------------------------

const VOLTA_SRC: &str = r#"\version "2.24.0"
\score {
  \new Staff {
    \repeat volta 2 { c'4 d'4 e'4 f'4 }
    \alternative { { g'1 } { a'1 } }
  }
  \layout { }
}
"#;

// KNOWN GAP (roadmap EBT2): the LY parser records `\repeat volta` as Score
// repeat-barlines + volta endings and never builds `Music::Repeat`; `lift.rs`
// does not reconstruct it. So LY→LY flattens repeats to linear notes (no data
// loss, but the `\repeat volta`/`\alternative` structure is lost). Faithful
// round-trip needs lift→Music::Repeat reconstruction across parser/lift/emit.
#[test]
#[ignore = "EBT2: repeat/volta structure not yet reconstructed (Music path flattens)"]
fn ly_to_ly_music_preserves_volta() {
    let ly = ly_to_ly_music(VOLTA_SRC);
    assert!(
        ly.contains("\\repeat"),
        "Music path dropped \\repeat:\n{ly}"
    );
}

#[test]
#[ignore = "EBT2: repeat/volta structure not yet reconstructed (Score path emits malformed alternative)"]
fn ly_to_ly_score_preserves_volta() {
    let ly = ly_to_ly_score(VOLTA_SRC);
    assert!(
        ly.contains("\\repeat"),
        "Score path dropped \\repeat:\n{ly}"
    );
}

// ---------------------------------------------------------------------------
// MIDI velocity <-> dynamics (B5)
// ---------------------------------------------------------------------------

fn note_dynamics(score: &_core::ir::score::Score) -> Vec<String> {
    use _core::ir::note::VoiceElement;
    let mut out = Vec::new();
    for part in score.parts() {
        for m in &part.measures {
            for v in &m.voices {
                for e in &v.elements {
                    if let VoiceElement::Note(n) = e {
                        for d in &n.dynamics {
                            out.push(d.sign.clone());
                        }
                    }
                }
            }
        }
    }
    out
}

#[test]
fn midi_roundtrip_maps_dynamics_to_velocity_and_back() {
    use _core::adapters::ir_to_midi::IrToMidiAdapter;
    use _core::adapters::midi_to_ir::MidiToIrAdapter;

    let src = r#"\version "2.24.0"
\score { \new Staff { c'1\ppp d'1\fff } \layout { } }
"#;
    let score = LyToIrAdapter::new().convert_str(src).expect("LY → Score");
    // The source dynamics must be parsed onto the notes.
    let src_dyns = note_dynamics(&score);
    assert!(
        src_dyns.iter().any(|d| d == "ppp") && src_dyns.iter().any(|d| d == "fff"),
        "parser did not attach note dynamics: {src_dyns:?}"
    );

    let bytes = IrToMidiAdapter::new()
        .convert_bytes(&score)
        .expect("IR → MIDI");
    let score2 = MidiToIrAdapter::new()
        .convert_bytes(&bytes)
        .expect("MIDI → IR");

    let dyns = note_dynamics(&score2);
    assert!(
        !dyns.is_empty(),
        "MIDI→IR produced no dynamics (velocity not mapped back)"
    );
    // The soft note must map back softer than the loud note.
    assert_eq!(
        dyns.first().map(String::as_str),
        Some("ppp"),
        "soft note band"
    );
    assert!(
        dyns.iter().any(|d| d == "fff"),
        "loud note should map to fff band: {dyns:?}"
    );
}
