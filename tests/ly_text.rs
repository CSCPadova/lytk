//! LilyPond strings and headers (Epic J4), and pitch languages (J5).

use _core::adapters::ir_to_ly::IrToLyAdapter;
use _core::adapters::ly_to_ir::{check, header_fields, LyToIrAdapter};
use _core::adapters::{FromIrAdapter, FromMusicAdapter, ToIrAdapter};
use _core::ir::lift::lift_to_music;
use _core::ir::note::VoiceElement;
use _core::ir::pitch::PitchStep;
use _core::ir::Score;

/// Strings to decode: a quote, a backslash, a line break, non-ASCII.
const TRICKY: [&str; 4] = ["say \"hi\"", r"a\b", "one\ntwo", "Émile — 漢字 ü"];

/// `s` as a LilyPond string literal.
fn ly(s: &str) -> String {
    let escaped = s
        .replace('\\', r"\\")
        .replace('"', "\\\"")
        .replace('\n', r"\n");
    format!("\"{escaped}\"")
}

fn read(text: &str) -> Score {
    LyToIrAdapter::new().convert_str(text).expect("reads")
}

/// Every text of `score` that a string of the source reaches.
fn texts(score: &Score) -> Vec<String> {
    let meta = &score.metadata;
    let mut out: Vec<String> = [&meta.title, &meta.composer, &meta.lyricist]
        .into_iter()
        .flatten()
        .cloned()
        .chain(meta.extra.values().cloned())
        .collect();
    for part in score.parts() {
        out.push(part.name.clone());
        for m in &part.measures {
            for d in &m.directions {
                out.extend(d.tempo.as_ref().and_then(|t| t.text.clone()));
                out.extend(d.da_capo.clone());
            }
            for v in &m.voices {
                for e in &v.elements {
                    if let VoiceElement::Note(n) = e {
                        out.extend(n.text_directions.iter().map(|t| t.text.clone()));
                        out.extend(n.lyrics.iter().map(|l| l.text.clone()));
                    }
                }
            }
        }
    }
    out.retain(|t| !t.is_empty());
    out.sort();
    out
}

#[test]
fn strings_decode_and_round_trip_in_every_context() {
    let [q, b, n, u] = TRICKY.map(ly);
    let dc = ly(&format!("D.C. {}", TRICKY[0]));
    let src = format!(
        "\\header {{ title = {q} composer = {b} opus = {n} copyright = {u} }}\n\
         \\new Staff \\with {{ instrumentName = {u} }} {{\n\
           \\tempo {q} 4 = 60\n\
           c'4^\\markup {{ {b} }} d'4 \\mark {dc} e'4 f'4\n\
         }}\n\
         \\addlyrics {{ {n} {q} x y }}\n"
    );
    let first = texts(&read(&src));
    for s in TRICKY {
        assert!(first.iter().any(|t| t == s), "{s:?} not read: {first:?}");
    }
    assert!(first.contains(&format!("D.C. {}", TRICKY[0])), "{first:?}");
    // Written back and read again, every text is the same.
    let written = IrToLyAdapter::new().convert(&read(&src)).unwrap();
    assert_eq!(texts(&read(&written)), first, "{written}");
}

#[test]
fn header_values_are_text() {
    let meta = read(
        "\\header { composer = \\markup { \\bold \"J. S.\" Bach } poet = #\"Anon \\\"X\\\"\" \
         tagline = ##f title = \\markup \\italic Sonata }",
    )
    .metadata;
    assert_eq!(meta.composer.as_deref(), Some("J. S. Bach"));
    assert_eq!(meta.lyricist.as_deref(), Some("Anon \"X\""));
    assert_eq!(meta.title.as_deref(), Some("Sonata"));
    assert!(!meta.extra.contains_key("tagline"), "{:?}", meta.extra);
}

#[test]
fn headers_are_scoped_to_their_movement() {
    let src = "\\header { title = \"Book\" composer = \"C\" }\n\
               \\score { \\header { piece = \"I\" title = \"First\" } { c'1 } }\n\
               \\score { { d'1 } }\n\
               \\header { opus = \"5\" title = \"Book 2\" }\n";
    let movements = LyToIrAdapter::new().convert_str_multi(src).unwrap();
    let (a, b) = (&movements[0].metadata, &movements[1].metadata);
    assert_eq!(a.title.as_deref(), Some("First"));
    assert_eq!(a.extra.get("piece").map(String::as_str), Some("I"));
    // The book's header applies to every movement, its last assignment
    // winning, wherever it stands.
    assert_eq!(b.title.as_deref(), Some("Book 2"));
    assert_eq!(a.composer.as_deref(), Some("C"));
    assert_eq!(b.composer.as_deref(), Some("C"));
    assert_eq!(a.extra.get("opus").map(String::as_str), Some("5"));
    assert_eq!(b.extra.get("opus").map(String::as_str), Some("5"));
    assert!(!b.extra.contains_key("piece"), "{:?}", b.extra);
}

#[test]
fn header_fields_are_located_and_can_be_cut() {
    let src = "\\header { title = \"A\\\"B\" }\n\
               \\score { \\header { piece = \\markup { \\italic Largo } } { c'1 } }\n\
               \\score { \\header { \"opus\" = #\"II\" } { d'1 } }\n";
    let fields: Vec<_> = header_fields(src)
        .into_iter()
        .map(|f| {
            (
                f.key.clone(),
                f.value.clone(),
                f.score,
                &src[f.start..f.end],
            )
        })
        .collect();
    assert_eq!(
        fields,
        [
            ("title".into(), "A\"B".into(), None, "title = \"A\\\"B\""),
            (
                "piece".into(),
                "Largo".into(),
                Some(0),
                "piece = \\markup { \\italic Largo }"
            ),
            ("opus".into(), "II".into(), Some(1), "\"opus\" = #\"II\""),
        ]
    );
}

#[test]
fn every_header_field_is_written_in_order() {
    let mut score = read("{ c'1 }");
    let meta = &mut score.metadata;
    meta.subtitle = Some("Sub".into());
    meta.lyricist = Some("Poet".into());
    for (k, v) in [
        ("opus", "5"),
        ("copyright", "©"),
        ("work-number", "3"),
        ("2nd key", "x"),
    ] {
        meta.extra.insert(k.into(), v.into());
    }
    let out = IrToLyAdapter::new().convert(&score).unwrap();
    let header: Vec<&str> = out
        .lines()
        .skip_while(|l| *l != "\\header {")
        .take_while(|l| *l != "}")
        .collect();
    assert_eq!(
        header,
        [
            "\\header {",
            "  subtitle = \"Sub\"",
            "  poet = \"Poet\"",
            "  \"2nd key\" = \"x\"",
            "  copyright = \"©\"",
            "  opus = \"5\"",
            "  work-number = \"3\"",
        ]
    );
    let again = read(&out).metadata;
    assert_eq!(again.extra, score.metadata.extra);
    // The Music path writes the same block.
    let music = IrToLyAdapter::new()
        .convert_music(&lift_to_music(&score))
        .unwrap();
    assert!(music.contains(&header.join("\n")), "{music}");
}

/// (step, alter, octave) of the first part's notes.
fn pitches(score: &Score) -> Vec<(PitchStep, i32, i32)> {
    score.parts()[0]
        .measures
        .iter()
        .flat_map(|m| &m.voices)
        .flat_map(|v| &v.elements)
        .filter_map(|e| match e {
            VoiceElement::Note(n) => {
                Some((n.pitch.step, n.pitch.alter.to_integer(), n.pitch.octave))
            }
            _ => None,
        })
        .collect()
}

#[test]
fn language_files_set_the_language() {
    use PitchStep::{C, D};
    assert_eq!(
        pitches(&read("\\include \"english.ly\" { cs'4 d'4 }")),
        [(C, 1, 4), (D, 0, 4)]
    );
    assert_eq!(
        pitches(&read("\\include \"arabic.ly\" { do'4 re'4 }")),
        [(C, 0, 4), (D, 0, 4)]
    );
    let codes = |t: &str| -> Vec<&str> { check(t, true).iter().map(|d| d.code).collect() };
    assert_eq!(
        codes("\\include \"english.ly\" { cs'4 }"),
        Vec::<&str>::new()
    );
    assert_eq!(
        codes("\\include \"persian.ly\" { c'4 }"),
        ["unknown-language"]
    );
    assert_eq!(
        codes("\\include \"notes.ily\" { c'4 }"),
        ["ignored-include"]
    );
}

#[test]
fn language_changes_inside_scores_and_music() {
    use PitchStep::C;
    assert_eq!(
        pitches(&read("\\score { \\language \"english\" { cs'4 } }")),
        [(C, 1, 4)]
    );
    assert_eq!(
        pitches(&read("{ c'4 \\language \"english\" cs'4 }")),
        [(C, 0, 4), (C, 1, 4)]
    );
    // An unknown name keeps the language in force.
    let src = "\\language \"english\" \\language \"klingon\" { cs'4 }";
    assert_eq!(pitches(&read(src)), [(C, 1, 4)]);
    let codes: Vec<&str> = check(src, true).iter().map(|d| d.code).collect();
    assert_eq!(codes, ["unknown-language"]);
}

#[test]
fn a_variable_read_again_keeps_the_language_of_its_definition() {
    // Inside `\relative` the variable is read again, in English.
    let src =
        "\\language \"english\"\nm = { cs'4 }\n\\language \"deutsch\"\n\\relative c' { \\m cis }";
    let p = pitches(&read(src));
    assert_eq!(
        p.iter().map(|&(s, a, _)| (s, a)).collect::<Vec<_>>(),
        [(PitchStep::C, 1); 2]
    );
}
