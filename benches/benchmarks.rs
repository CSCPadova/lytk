//! Criterion benchmarks for lytk hot paths.
//!
//! Run with:
//!   cargo bench
//!   cargo bench -- <filter>       # e.g. cargo bench -- parse
//!
//! HTML reports are written to `target/criterion/`.

use std::path::Path;

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion};

use _core::adapters::ir_to_ly::IrToLyAdapter;
use _core::adapters::ir_to_mxml::IrToMxmlAdapter;
use _core::adapters::ly_to_ir::LyToIrAdapter;
use _core::adapters::mxml_to_ir::MxmlToIrAdapter;
use _core::adapters::{FromIrAdapter, ToIrAdapter};
use _core::ir::language::PitchLanguage;
use _core::ir::pitch::{Pitch, PitchStep};
use _core::ir::Score;
use _core::transforms::invert::invert;
use _core::transforms::language::change_language;
use _core::transforms::retrograde::retrograde;
use _core::transforms::transpose::transpose;

// ---------------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------------

const MXML_PITCHES: &str = "tests/fixtures/xml/01a-Pitches-Pitches.xml";
const MXML_MULTI: &str = "tests/fixtures/xml/41a-MultiParts-Partorder.xml";
const MXML_STAFFGROUPS: &str = "tests/fixtures/xml/41c-StaffGroups.xml";
const LY_RELATIVE: &str = "tests/fixtures/ly/relative-repeat.ly";

/// Build a moderately large score by concatenating an XML file N times via
/// adapter re-parsing.  Used to stress-test transforms.
fn load_large_score() -> Score {
    let adapter = MxmlToIrAdapter::new();
    let base = adapter.convert_file(Path::new(MXML_PITCHES)).unwrap();
    // The pitches file already has ~100 notes, which is sufficient.
    base
}

// ---------------------------------------------------------------------------
// Parsing benchmarks
// ---------------------------------------------------------------------------

fn bench_parse_musicxml(c: &mut Criterion) {
    let mut group = c.benchmark_group("parse_musicxml");

    let files = [
        ("pitches_1263L", MXML_PITCHES),
        ("multipart_186L", MXML_MULTI),
        ("staffgroups_1619L", MXML_STAFFGROUPS),
    ];

    for (label, path) in &files {
        let xml = std::fs::read_to_string(path).unwrap();
        group.bench_with_input(BenchmarkId::new("from_string", label), &xml, |b, xml| {
            let adapter = MxmlToIrAdapter::new();
            b.iter(|| adapter.convert_str(black_box(xml)));
        });
    }

    group.bench_function("from_file/pitches", |b| {
        let adapter = MxmlToIrAdapter::new();
        b.iter(|| adapter.convert_file(black_box(Path::new(MXML_PITCHES))));
    });

    group.finish();
}

fn bench_parse_lilypond(c: &mut Criterion) {
    let mut group = c.benchmark_group("parse_lilypond");

    let ly_text = std::fs::read_to_string(LY_RELATIVE).unwrap();

    group.bench_function("relative_repeat/from_string", |b| {
        let adapter = LyToIrAdapter::new();
        b.iter(|| adapter.convert_str(black_box(&ly_text)));
    });

    group.bench_function("relative_repeat/from_file", |b| {
        let adapter = LyToIrAdapter::new();
        b.iter(|| adapter.convert_file(black_box(Path::new(LY_RELATIVE))));
    });

    group.finish();
}

fn bench_batch_parse(c: &mut Criterion) {
    let mut group = c.benchmark_group("batch_parse");

    // Collect all parseable XML files
    let xml_dir = Path::new("tests/fixtures/xml");
    let xml_files: Vec<_> = std::fs::read_dir(xml_dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|ext| ext == "xml"))
        .collect();

    let count = xml_files.len();

    group.bench_function(format!("musicxml_{count}_files"), |b| {
        let adapter = MxmlToIrAdapter::new();
        b.iter(|| {
            for path in &xml_files {
                let _ = adapter.convert_file(black_box(path));
            }
        });
    });

    group.finish();
}

// ---------------------------------------------------------------------------
// Emission benchmarks
// ---------------------------------------------------------------------------

fn bench_emit(c: &mut Criterion) {
    let mut group = c.benchmark_group("emit");

    let adapter = MxmlToIrAdapter::new();
    let score = adapter.convert_file(Path::new(MXML_PITCHES)).unwrap();

    group.bench_function("ir_to_lilypond", |b| {
        let emitter = IrToLyAdapter::new();
        b.iter(|| emitter.convert(black_box(&score)));
    });

    group.bench_function("ir_to_musicxml", |b| {
        let emitter = IrToMxmlAdapter::new();
        b.iter(|| emitter.convert(black_box(&score)));
    });

    // Multi-part score emission
    let multi = adapter
        .convert_file(Path::new(MXML_MULTI))
        .unwrap();

    group.bench_function("ir_to_lilypond/multipart", |b| {
        let emitter = IrToLyAdapter::new();
        b.iter(|| emitter.convert(black_box(&multi)));
    });

    group.bench_function("ir_to_musicxml/multipart", |b| {
        let emitter = IrToMxmlAdapter::new();
        b.iter(|| emitter.convert(black_box(&multi)));
    });

    group.finish();
}

// ---------------------------------------------------------------------------
// Transform benchmarks
// ---------------------------------------------------------------------------

fn bench_transpose(c: &mut Criterion) {
    let mut group = c.benchmark_group("transform_transpose");

    let score = load_large_score();

    for semitones in [1, 5, 12, -3] {
        group.bench_with_input(
            BenchmarkId::new("semitones", semitones),
            &semitones,
            |b, &s| {
                b.iter(|| transpose(black_box(&score), s));
            },
        );
    }

    // Identity (zero) fast path
    group.bench_function("zero_noop", |b| {
        b.iter(|| transpose(black_box(&score), 0));
    });

    group.finish();
}

fn bench_change_language(c: &mut Criterion) {
    let mut group = c.benchmark_group("transform_language");

    let score = load_large_score();

    let languages = [
        ("english", PitchLanguage::English),
        ("deutsch", PitchLanguage::Deutsch),
        ("italiano", PitchLanguage::Italiano),
    ];

    for (name, lang) in &languages {
        group.bench_with_input(BenchmarkId::new("to", name), lang, |b, &lang| {
            b.iter(|| change_language(black_box(&score), lang));
        });
    }

    group.finish();
}

fn bench_invert(c: &mut Criterion) {
    let mut group = c.benchmark_group("transform_invert");

    let score = load_large_score();
    let axis = Pitch::new(PitchStep::C, 4);

    group.bench_function("around_c4", |b| {
        b.iter(|| invert(black_box(&score), axis));
    });

    group.finish();
}

fn bench_retrograde(c: &mut Criterion) {
    let mut group = c.benchmark_group("transform_retrograde");

    let score = load_large_score();

    group.bench_function("reverse", |b| {
        b.iter(|| retrograde(black_box(&score)));
    });

    group.finish();
}

// ---------------------------------------------------------------------------
// Round-trip benchmarks
// ---------------------------------------------------------------------------

fn bench_roundtrip(c: &mut Criterion) {
    let mut group = c.benchmark_group("roundtrip");

    let xml_text = std::fs::read_to_string(MXML_PITCHES).unwrap();

    group.bench_function("mxml_to_ir_to_ly", |b| {
        let parser = MxmlToIrAdapter::new();
        let emitter = IrToLyAdapter::new();
        b.iter(|| {
            let score = parser.convert_str(black_box(&xml_text)).unwrap();
            emitter.convert(black_box(&score)).unwrap()
        });
    });

    group.bench_function("mxml_to_ir_to_mxml", |b| {
        let parser = MxmlToIrAdapter::new();
        let emitter = IrToMxmlAdapter::new();
        b.iter(|| {
            let score = parser.convert_str(black_box(&xml_text)).unwrap();
            emitter.convert(black_box(&score)).unwrap()
        });
    });

    let ly_text = std::fs::read_to_string(LY_RELATIVE).unwrap();

    group.bench_function("ly_to_ir_to_ly", |b| {
        let parser = LyToIrAdapter::new();
        let emitter = IrToLyAdapter::new();
        b.iter(|| {
            let score = parser.convert_str(black_box(&ly_text)).unwrap();
            emitter.convert(black_box(&score)).unwrap()
        });
    });

    // Full pipeline: parse MusicXML → transpose → emit LilyPond
    group.bench_function("mxml_transpose_ly", |b| {
        let parser = MxmlToIrAdapter::new();
        let emitter = IrToLyAdapter::new();
        b.iter(|| {
            let score = parser.convert_str(black_box(&xml_text)).unwrap();
            let transposed = transpose(&score, 3);
            emitter.convert(black_box(&transposed)).unwrap()
        });
    });

    group.finish();
}

// ---------------------------------------------------------------------------
// Harness
// ---------------------------------------------------------------------------

criterion_group!(
    parsing,
    bench_parse_musicxml,
    bench_parse_lilypond,
    bench_batch_parse,
);

criterion_group!(emission, bench_emit);

criterion_group!(
    transforms,
    bench_transpose,
    bench_change_language,
    bench_invert,
    bench_retrograde,
);

criterion_group!(roundtrips, bench_roundtrip);

criterion_main!(parsing, emission, transforms, roundtrips);
