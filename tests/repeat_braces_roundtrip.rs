//! Regression: the Score-path LilyPond emitter must keep `{`/`}` balanced for
//! repeats, even when MusicXML uses a bare backward repeat (repeat-to-top, no
//! forward `|:`) or a forward repeat that is never ended. An unbalanced brace
//! previously closed the part variable early and silently dropped every later
//! measure's notes.

use _core::adapters::ir_to_ly::IrToLyAdapter;
use _core::adapters::ly_to_ir::LyToIrAdapter;
use _core::adapters::mxml_to_ir::MxmlToIrAdapter;
use _core::adapters::{FromIrAdapter, ToIrAdapter};
use _core::ir::note::VoiceElement;
use _core::ir::Score;

fn element_count(score: &Score) -> usize {
    score
        .parts()
        .iter()
        .flat_map(|p| &p.measures)
        .flat_map(|m| &m.voices)
        .flat_map(|v| &v.elements)
        .filter(|e| {
            matches!(
                e,
                VoiceElement::Note(_) | VoiceElement::Rest(_) | VoiceElement::Chord(_)
            )
        })
        .count()
}

fn braces_balanced(ly: &str) -> bool {
    ly.matches('{').count() == ly.matches('}').count()
}

fn check(fixture: &str) {
    let xml = std::fs::read_to_string(fixture).unwrap_or_else(|_| panic!("read {fixture}"));
    let score1 = MxmlToIrAdapter::new()
        .convert_str(&xml)
        .unwrap_or_else(|e| panic!("{fixture}: parse xml: {e}"));
    let before = element_count(&score1);

    let ly = IrToLyAdapter::new()
        .convert(&score1)
        .unwrap_or_else(|e| panic!("{fixture}: emit ly: {e}"));
    assert!(
        braces_balanced(&ly),
        "{fixture}: emitted LilyPond has unbalanced braces ({} open / {} close):\n{ly}",
        ly.matches('{').count(),
        ly.matches('}').count(),
    );

    // Re-parse must not have truncated the content at a stray brace.
    let score2 = LyToIrAdapter::new()
        .convert_str(&ly)
        .unwrap_or_else(|e| panic!("{fixture}: reparse ly: {e}"));
    let after = element_count(&score2);
    assert_eq!(
        before, after,
        "{fixture}: Score-path round-trip dropped content ({before} → {after})"
    );
}

#[test]
fn backward_only_repeat_keeps_braces_balanced() {
    // 45a: a bare `<repeat direction="backward"/>` with no forward repeat.
    check("tests/fixtures/xml/45a-SimpleRepeat.xml");
}

#[test]
fn forward_repeat_not_ended_keeps_braces_balanced() {
    // 45g: a forward repeat that never gets a matching backward repeat.
    check("tests/fixtures/xml/45g-Repeats-NotEnded.xml");
}
