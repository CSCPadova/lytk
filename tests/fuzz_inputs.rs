//! Robustness fuzzing: every parser entry point must DEGRADE GRACEFULLY on
//! arbitrary / malformed input — return `Err` (or `Ok`), never panic, hang, or
//! OOM. lytk ingests untrusted files (`.ly`, `.xml`/`.mxl`, `.mid`, `.abc`), so
//! this is a hard requirement for a public release. proptest catches any panic
//! as a test failure; the Phase-2 robustness fixes (recursion cap, input
//! clamping, bounded unzip, panic firewalls) are what keep these green.

use proptest::prelude::*;

use _core::adapters::abc_to_ir::AbcToIrAdapter;
use _core::adapters::humdrum_to_ir::HumdrumToIrAdapter;
use _core::adapters::ly_to_ir::LyToIrAdapter;
use _core::adapters::midi_to_ir::MidiToIrAdapter;
use _core::adapters::mxml_to_ir::MxmlToIrAdapter;
use _core::adapters::ToIrAdapter;

proptest! {
    #![proptest_config(ProptestConfig::with_cases(128))]

    /// Arbitrary text must not crash the LilyPond parser.
    #[test]
    fn ly_parser_survives_arbitrary_text(s in "\\PC{0,2000}") {
        let _ = LyToIrAdapter::new().convert_str(&s);
    }

    /// Arbitrary text must not crash the ABC parser.
    #[test]
    fn abc_parser_survives_arbitrary_text(s in "\\PC{0,2000}") {
        let _ = AbcToIrAdapter::new().convert_str(&s);
    }

    /// Arbitrary text must not crash the Humdrum parser.
    #[test]
    fn humdrum_parser_survives_arbitrary_text(s in "\\PC{0,2000}") {
        let _ = HumdrumToIrAdapter::new().convert_str(&s);
    }

    /// kern-shaped fuzz: a **kern header with arbitrary token soup.
    #[test]
    fn humdrum_parser_survives_kern_shaped_text(s in "\\PC{0,1000}") {
        let _ = HumdrumToIrAdapter::new().convert_str(&format!("**kern\n{s}\n*-\n"));
    }

    /// Arbitrary bytes must not crash the MusicXML/MXL reader (covers the zip
    /// path too — random bytes starting with the PK magic hit the unzip code).
    #[test]
    fn xml_parser_survives_arbitrary_bytes(bytes in proptest::collection::vec(any::<u8>(), 0..4000)) {
        let s = String::from_utf8_lossy(&bytes);
        let _ = MxmlToIrAdapter::new().convert_str(&s);
    }

    /// Arbitrary bytes must not crash the MIDI reader.
    #[test]
    fn midi_parser_survives_arbitrary_bytes(bytes in proptest::collection::vec(any::<u8>(), 0..4000)) {
        let _ = MidiToIrAdapter::new().convert_bytes(&bytes);
    }

    /// Bytes carrying the ZIP magic exercise the bounded unzip path specifically.
    #[test]
    fn xml_parser_survives_ziplike_bytes(mut bytes in proptest::collection::vec(any::<u8>(), 0..4000)) {
        bytes.splice(0..0, *b"PK\x03\x04");
        let s = String::from_utf8_lossy(&bytes);
        let _ = MxmlToIrAdapter::new().convert_str(&s);
    }
}

// --- Targeted regression inputs the audit reproduced as crashes/hangs --------

#[test]
fn deeply_nested_lilypond_is_rejected_not_overflowed() {
    let deep = format!("{}{}", "{ ".repeat(9000), " }".repeat(9000));
    // Must return (Err) without overflowing the stack / aborting the process.
    let _ = LyToIrAdapter::new().convert_str(&deep);
}

#[test]
fn abc_zero_denominator_meter_does_not_panic() {
    let _ = AbcToIrAdapter::new().convert_str("X:1\nM:4/0\nK:C\nCDEF\n");
}

#[test]
fn midi_zero_division_header_does_not_hang() {
    // Format-0 SMF declaring 0 ticks-per-quarter + one empty track.
    let bytes: &[u8] = &[
        b'M', b'T', b'h', b'd', 0, 0, 0, 6, 0, 0, 0, 1, 0, 0, // division = 0
        b'M', b'T', b'r', b'k', 0, 0, 0, 4, 0, 0xFF, 0x2F, 0x00,
    ];
    let _ = MidiToIrAdapter::new().convert_bytes(bytes);
}

#[test]
fn empty_and_garbage_inputs_are_clean_errors() {
    let ly = LyToIrAdapter::new();
    let abc = AbcToIrAdapter::new();
    let xml = MxmlToIrAdapter::new();
    let midi = MidiToIrAdapter::new();
    for input in ["", "\0\0\0\0", "not music at all", "<<<<<<", "}}}}"] {
        let _ = ly.convert_str(input);
        let _ = abc.convert_str(input);
        let _ = xml.convert_str(input);
        let _ = midi.convert_bytes(input.as_bytes());
    }
}
