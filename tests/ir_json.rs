//! The IR's JSON: every fixture's score, the music document lifted from it
//! and (for LilyPond) the music tree read directly read back equal to what
//! was written; and the JSON's size per note, a committed baseline that may
//! only fall.

mod common;

use _core::adapters::abc_to_ir::AbcToIrAdapter;
use _core::adapters::humdrum_to_ir::HumdrumToIrAdapter;
use _core::adapters::ly_to_ir::LyToIrAdapter;
use _core::adapters::midi_to_ir::MidiToIrAdapter;
use _core::adapters::mxml_to_ir::MxmlToIrAdapter;
use _core::adapters::{ToIrAdapter, ToMusicAdapter};
use _core::ir::lift::lift_to_music;
use _core::ir::music::MusicDocument;
use _core::ir::score::Score;
use serde::de::DeserializeOwned;
use serde::Serialize;
use std::fmt::Debug;
use std::path::PathBuf;

/// Compact JSON bytes per note of every fixture's score (0.5.0: 810).
const BYTES_PER_NOTE: usize = 174;

fn fixture_paths() -> Vec<PathBuf> {
    let mut paths: Vec<PathBuf> = std::fs::read_dir("tests/fixtures")
        .expect("fixtures")
        .flatten()
        .flat_map(|d| std::fs::read_dir(d.path()).into_iter().flatten().flatten())
        .map(|e| e.path())
        .collect();
    paths.sort();
    paths
}

fn reader(path: &std::path::Path) -> Option<Box<dyn ToIrAdapter>> {
    Some(match path.extension()?.to_str()? {
        "xml" | "mxl" => Box::new(MxmlToIrAdapter::new()),
        "ly" => Box::new(LyToIrAdapter::new()),
        "abc" => Box::new(AbcToIrAdapter::new()),
        "mid" | "midi" => Box::new(MidiToIrAdapter::new()),
        "krn" => Box::new(HumdrumToIrAdapter::new()),
        _ => return None,
    })
}

/// `None` when `value` reads back equal from its JSON, else what went wrong.
fn round_trip<T: Serialize + DeserializeOwned + PartialEq + Debug>(value: &T) -> Option<String> {
    let json = serde_json::to_string(value).expect("serializes");
    match serde_json::from_str::<T>(&json) {
        Ok(back) if back == *value => None,
        Ok(back) => {
            let lines = |v: &T| serde_json::to_string_pretty(v).expect("serializes");
            let (a, b) = (lines(value), lines(&back));
            let first = a.lines().zip(b.lines()).find(|(x, y)| x != y);
            Some(format!("reads back different: {first:?}"))
        }
        Err(e) => Some(format!("does not read back: {e}")),
    }
}

fn note_count(score: &Score) -> usize {
    score
        .parts()
        .iter()
        .flat_map(|p| &p.measures)
        .flat_map(|m| &m.voices)
        .flat_map(|v| &v.elements)
        .map(|e| e.notes().len())
        .sum()
}

#[test]
fn every_fixture_reads_back_from_its_json() {
    let (mut bytes, mut notes, mut read) = (0, 0, 0);
    let mut failed = Vec::new();
    for path in fixture_paths() {
        let Some(reader) = reader(&path) else {
            continue;
        };
        let name = path.display().to_string();
        let Some(score) = common::safe(|| reader.convert_file(&path).ok()) else {
            continue;
        };
        read += 1;
        if let Some(e) = round_trip(&score) {
            failed.push(format!("{name} (score): {e}"));
        }
        if let Some(e) = round_trip::<MusicDocument>(&lift_to_music(&score)) {
            failed.push(format!("{name} (lifted music): {e}"));
        }
        if path.extension().is_some_and(|e| e == "ly") {
            let text = std::fs::read_to_string(&path).expect("text");
            if let Some(doc) =
                common::safe(|| LyToIrAdapter::new().convert_str_to_music(&text).ok())
            {
                if let Some(e) = round_trip(&doc) {
                    failed.push(format!("{name} (music): {e}"));
                }
            }
        }
        bytes += serde_json::to_string(&score).expect("serializes").len();
        notes += note_count(&score);
    }
    assert!(read >= 190, "only {read} fixtures read");
    assert!(failed.is_empty(), "{failed:#?}");
    let per_note = bytes / notes.max(1);
    println!(
        "IR JSON: {bytes} bytes for {notes} notes of {read} fixtures, {per_note} a note \
         (baseline {BYTES_PER_NOTE})"
    );
    assert!(
        per_note <= BYTES_PER_NOTE,
        "the IR's JSON grew: {per_note} bytes a note > {BYTES_PER_NOTE}"
    );
}
