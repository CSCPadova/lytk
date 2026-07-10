//! lytk CLI — batch music notation conversion and augmentation.
//!
//! Subcommands:
//!   convert         Convert between LilyPond, MusicXML, MXL, MIDI and ABC
//!   transpose       Transpose pitches by N semitones
//!   invert          Mirror pitches around an axis pitch
//!   retrograde      Reverse the music in time
//!   change-language Change the LilyPond pitch-name language
//!   abs2rel         Rewrite a LilyPond file's pitches in `\relative` form
//!   rel2abs         Rewrite a LilyPond file's pitches in absolute form
//!   info            Print score metadata
//!   flatten         Recursively expand \include directives into a single flat file
//!
//! Input/output paths of `-` mean stdin/stdout. When reading from stdin the
//! input format must be given with `--from`; when writing to stdout the output
//! format must be given with `--format` (there is no extension to infer from).

use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use clap::{Parser, Subcommand, ValueEnum};
use rayon::prelude::*;
use serde::{Deserialize, Serialize};

use _core::adapters::ir_to_midi::IrToMidiAdapter;
use _core::adapters::midi_to_ir::MidiToIrAdapter;
use _core::adapters::{
    abc_to_ir::AbcToIrAdapter,
    humdrum_to_ir::HumdrumToIrAdapter,
    ir_to_abc::IrToAbcAdapter,
    ir_to_humdrum::IrToHumdrumAdapter,
    ir_to_ly::IrToLyAdapter,
    ir_to_mxml::IrToMxmlAdapter,
    ly_flatten::{flatten, FlattenOpts},
    ly_to_ir::LyToIrAdapter,
    mxml_to_ir::MxmlToIrAdapter,
    FromIrAdapter, FromMusicAdapter, ToIrAdapter, ToMusicAdapter,
};
use _core::ir::duration::Frac;
use _core::ir::interval::Interval;
use _core::ir::language::{PitchLanguage, PitchMode};
use _core::ir::note::VoiceElement;
use _core::ir::pitch::{Alter, Pitch, PitchStep};
use _core::ir::{Score, ScoreChild};
use _core::transforms::{invert, language, retrograde, transpose};

/// lytk — music notation conversion and augmentation toolkit.
#[derive(Parser)]
// `about` is set explicitly (rather than inherited from the crate
// `description`) so the CLI banner stays human-facing and consistent with the
// Python entry point; the Cargo manifest keeps its package-metadata wording.
#[command(
    name = "lytk",
    version,
    about = "lytk — music notation conversion and augmentation toolkit."
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Convert files between LilyPond, MusicXML, MXL, MIDI and ABC formats.
    Convert {
        /// Input file or directory (`-` for stdin).
        input: PathBuf,

        /// Output file or directory (`-` for stdout).
        #[arg(short, long)]
        output: PathBuf,

        /// Force output format (auto-detected from extension by default;
        /// required when output is `-`/stdout).
        #[arg(short, long)]
        format: Option<OutputFormat>,

        /// Force input format (auto-detected from extension by default;
        /// required when input is `-`/stdin).
        #[arg(long)]
        from: Option<OutputFormat>,

        /// Number of parallel threads for batch mode (0 = auto).
        #[arg(short, long, default_value_t = 0)]
        jobs: usize,
    },

    /// Transpose pitches. Choose exactly one of --semitones, --interval, --to-key.
    Transpose {
        /// Input file (`-` for stdin).
        input: PathBuf,

        /// Output file (`-` for stdout).
        #[arg(short, long)]
        output: PathBuf,

        /// Chromatic transposition by N semitones (positive = up, negative = down).
        #[arg(short, long, allow_negative_numbers = true)]
        semitones: Option<i32>,

        /// Diatonic transposition by a named interval: M3, m3, P5, A4, d5, -m2, P8.
        #[arg(short, long)]
        interval: Option<String>,

        /// Transpose so the tonic becomes this key (nearest direction): D, Bb, F#, ef.
        #[arg(long)]
        to_key: Option<String>,

        /// Force output format.
        #[arg(short, long)]
        format: Option<OutputFormat>,

        /// Force input format (required when input is `-`/stdin).
        #[arg(long)]
        from: Option<OutputFormat>,
    },

    /// Invert (mirror) all pitches around an axis pitch.
    Invert {
        /// Input file (`-` for stdin).
        input: PathBuf,

        /// Output file (`-` for stdout).
        #[arg(short, long)]
        output: PathBuf,

        /// Axis pitch, e.g. `c4`, `fs3`, `bf5` (default middle C).
        #[arg(short, long, default_value = "c4")]
        axis: String,

        /// Force output format.
        #[arg(short, long)]
        format: Option<OutputFormat>,

        /// Force input format (required when input is `-`/stdin).
        #[arg(long)]
        from: Option<OutputFormat>,
    },

    /// Reverse the music in time (retrograde).
    Retrograde {
        /// Input file (`-` for stdin).
        input: PathBuf,

        /// Output file (`-` for stdout).
        #[arg(short, long)]
        output: PathBuf,

        /// Force output format.
        #[arg(short, long)]
        format: Option<OutputFormat>,

        /// Force input format (required when input is `-`/stdin).
        #[arg(long)]
        from: Option<OutputFormat>,
    },

    /// Change the LilyPond pitch-name language (affects LilyPond output).
    ChangeLanguage {
        /// Input file (`-` for stdin).
        input: PathBuf,

        /// Output file (`-` for stdout).
        #[arg(short, long)]
        output: PathBuf,

        /// Target language: nederlands, english, deutsch, italiano, espanol, …
        #[arg(short, long)]
        language: String,

        /// Force output format.
        #[arg(short, long)]
        format: Option<OutputFormat>,

        /// Force input format (required when input is `-`/stdin).
        #[arg(long)]
        from: Option<OutputFormat>,
    },

    /// Rewrite a LilyPond file's pitch entry in `\relative` form.
    ///
    /// Parses the source (absolute or relative) and re-emits it using
    /// `\relative` octave marks. Output is LilyPond. Multi-staff / multi-voice
    /// parts fall back to absolute octaves where relative threading would be
    /// ambiguous.
    Abs2rel {
        /// Input LilyPond file (`-` for stdin).
        input: PathBuf,

        /// Output file (`-` for stdout).
        #[arg(short, long)]
        output: PathBuf,
    },

    /// Rewrite a LilyPond file's pitch entry in absolute form.
    ///
    /// Parses the source (absolute or relative) and re-emits it with absolute
    /// octave marks (no `\relative` wrapper). Output is LilyPond.
    Rel2abs {
        /// Input LilyPond file (`-` for stdin).
        input: PathBuf,

        /// Output file (`-` for stdout).
        #[arg(short, long)]
        output: PathBuf,
    },

    /// Print score metadata (title, composer, parts, measures).
    Info {
        /// Input file.
        input: PathBuf,

        /// Emit machine-readable JSON instead of human-readable text.
        #[arg(long)]
        json: bool,
    },

    /// Emit per-part measure time positions as JSON (offsets in quarter notes).
    ///
    /// These are *temporal/structural* positions derived from notated durations
    /// — not graphical coordinates (lytk has no layout engine).
    Positions {
        /// Input file (`-` for stdin requires --from).
        input: PathBuf,

        /// Force input format (required when input is `-`/stdin).
        #[arg(long)]
        from: Option<OutputFormat>,
    },

    /// Export each part to its own file in a directory (a "parts bundle").
    ///
    /// Output filenames are `<input-stem>_<part>.<ext>`.
    Bundle {
        /// Input file (`-` for stdin requires --from).
        input: PathBuf,

        /// Output directory (created if missing).
        #[arg(short, long)]
        output: PathBuf,

        /// Output format for each part.
        #[arg(short, long, default_value = "xml")]
        format: OutputFormat,

        /// Force input format (required when input is `-`/stdin).
        #[arg(long)]
        from: Option<OutputFormat>,
    },

    /// Run a JSON batch-job file: an array of per-job conversions/transforms.
    ///
    /// Each job is `{in, out, format?, from?, transpose?, interval?}`. Jobs run in
    /// parallel (isolated: one failing job doesn't abort the others) and the
    /// command exits non-zero if any job failed.
    Batch {
        /// JSON job file (array of jobs).
        file: PathBuf,

        /// Number of parallel threads (0 = auto).
        #[arg(short, long, default_value_t = 0)]
        jobs: usize,

        /// Write a JSON report of per-job results to this path (`-` for stdout).
        #[arg(long)]
        report: Option<PathBuf>,
    },

    /// Compare two scores semantically (parts, note count, pitch multiset).
    ///
    /// Exits non-zero when they differ, so it can gate CI. Comparison is on
    /// sounding content, not source text.
    Diff {
        /// First input file (`-` for stdin requires --from).
        a: PathBuf,

        /// Second input file.
        b: PathBuf,

        /// Emit a JSON report instead of human-readable text.
        #[arg(long)]
        json: bool,

        /// Force input format for both files (required when either is `-`/stdin).
        #[arg(long)]
        from: Option<OutputFormat>,
    },

    /// Flatten a LilyPond file by recursively expanding all \include directives.
    ///
    /// Produces a single self-contained output file. Duplicate \version and
    /// \language directives are deduplicated (last occurrence wins, with a
    /// warning). Multiple \header blocks are an error.
    Flatten {
        /// Input LilyPond file.
        input: PathBuf,

        /// Output file. Writes to stdout if omitted.
        #[arg(short, long)]
        output: Option<PathBuf>,

        /// Additional include search paths (like LilyPond's -I flag).
        /// May be specified multiple times.
        #[arg(short = 'I', long = "include-path", value_name = "DIR")]
        include_paths: Vec<PathBuf>,

        /// Suppress % === BEGIN/END INCLUDE === comment markers in output.
        #[arg(long)]
        no_markers: bool,
    },
}

#[derive(Clone, Copy, ValueEnum)]
enum OutputFormat {
    Ly,
    Xml,
    Mxl,
    Midi,
    Abc,
    Krn,
}

fn main() {
    let cli = Cli::parse();

    let result = match cli.command {
        Command::Convert {
            input,
            output,
            format,
            from,
            jobs,
        } => run_convert(&input, &output, format, from, jobs),
        Command::Transpose {
            input,
            output,
            semitones,
            interval,
            to_key,
            format,
            from,
        } => run_transpose(
            &input,
            &output,
            semitones,
            interval.as_deref(),
            to_key.as_deref(),
            format,
            from,
        ),
        Command::Invert {
            input,
            output,
            axis,
            format,
            from,
        } => run_invert(&input, &output, &axis, format, from),
        Command::Retrograde {
            input,
            output,
            format,
            from,
        } => run_retrograde(&input, &output, format, from),
        Command::ChangeLanguage {
            input,
            output,
            language,
            format,
            from,
        } => run_change_language(&input, &output, &language, format, from),
        Command::Abs2rel { input, output } => {
            run_relative_mode(&input, &output, PitchMode::Relative)
        }
        Command::Rel2abs { input, output } => {
            run_relative_mode(&input, &output, PitchMode::Absolute)
        }
        Command::Info { input, json } => run_info(&input, json),
        Command::Positions { input, from } => run_positions(&input, from),
        Command::Bundle {
            input,
            output,
            format,
            from,
        } => run_bundle(&input, &output, format, from),
        Command::Batch { file, jobs, report } => run_batch_jobs(&file, jobs, report.as_deref()),
        Command::Diff { a, b, json, from } => run_diff(&a, &b, json, from),
        Command::Flatten {
            input,
            output,
            include_paths,
            no_markers,
        } => run_flatten(&input, output.as_deref(), &include_paths, no_markers),
    };

    if let Err(e) = result {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}

// ---------------------------------------------------------------------------
// Subcommand implementations
// ---------------------------------------------------------------------------

fn run_convert(
    input: &Path,
    output: &Path,
    format: Option<OutputFormat>,
    from: Option<OutputFormat>,
    jobs: usize,
) -> anyhow::Result<()> {
    if !is_dash(input) && input.is_dir() {
        return run_batch(input, output, format, jobs);
    }

    // LY→LY single-file: use Music tree path for better structural fidelity.
    let in_fmt = resolve_input_format(input, from)?;
    let out_fmt = detect_output_format(output, format)?;
    if matches!(in_fmt, InputFormat::LilyPond) && matches!(out_fmt, OutputFormat::Ly) {
        return convert_ly_to_ly(input, output);
    }

    let scores = parse_source_multi(input, from)?;
    if scores.len() <= 1 {
        let score = scores.into_iter().next().unwrap_or_else(Score::new);
        write_output(&score, output, format)?;
    } else if is_dash(output) {
        // A stdout stream has no place to put multiple movements.
        anyhow::bail!(
            "{} movements parsed; cannot write multiple movements to stdout — use a file path",
            scores.len()
        );
    } else {
        // Multi-movement: the FIRST movement goes to the requested path (so
        // pipelines that use it keep working), the rest to _02, _03, …
        // suffixed siblings. Previously nothing landed at the requested path
        // (only _01/_02 files), silently breaking any downstream step.
        let stem = output
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("output");
        let ext = output.extension().and_then(|s| s.to_str()).unwrap_or("xml");
        let parent = output.parent().unwrap_or(Path::new("."));
        for (idx, score) in scores.iter().enumerate() {
            let path = if idx == 0 {
                output.to_path_buf()
            } else {
                parent.join(format!("{}_{:02}.{}", stem, idx + 1, ext))
            };
            write_output(score, &path, format)?;
            eprintln!("Wrote movement {} → {}", idx + 1, path.display());
        }
        eprintln!(
            "note: input contains {} movements; movements 2+ were written with _NN suffixes",
            scores.len()
        );
    }
    Ok(())
}

/// True when both ends of a transform are LilyPond — those runs go through the
/// Music tree (Layer 1), like `convert`, so contexts/relative structure survive
/// instead of being flattened through the measure-based Score.
fn is_ly_to_ly(
    input: &Path,
    output: &Path,
    format: Option<OutputFormat>,
    from: Option<OutputFormat>,
) -> bool {
    matches!(resolve_input_format(input, from), Ok(InputFormat::LilyPond))
        && matches!(detect_output_format(output, format), Ok(OutputFormat::Ly))
}

fn ly_music_transform(
    input: &Path,
    output: &Path,
    apply: impl Fn(&_core::ir::music::MusicDocument) -> _core::ir::music::MusicDocument,
) -> anyhow::Result<()> {
    let parser = LyToIrAdapter::new();
    let doc = if is_dash(input) {
        let bytes = read_input_bytes(input)?;
        parser.convert_str_to_music(std::str::from_utf8(&bytes)?)?
    } else {
        parser.convert_file_to_music(input)?
    };
    let ly = IrToLyAdapter::new().convert_music(&apply(&doc))?;
    write_bytes(output, ly.as_bytes())
}

fn run_transpose(
    input: &Path,
    output: &Path,
    semitones: Option<i32>,
    interval: Option<&str>,
    to_key: Option<&str>,
    format: Option<OutputFormat>,
    from: Option<OutputFormat>,
) -> anyhow::Result<()> {
    let n_modes = semitones.is_some() as u8 + interval.is_some() as u8 + to_key.is_some() as u8;
    if n_modes != 1 {
        anyhow::bail!("specify exactly one of --semitones, --interval, --to-key");
    }

    if is_ly_to_ly(input, output, format, from) {
        let iv = interval
            .map(|iv| Interval::from_name(iv).map_err(|e| anyhow::anyhow!(e)))
            .transpose()?;
        let tonic = to_key.map(parse_tonic).transpose()?;
        return ly_music_transform(input, output, |doc| {
            if let Some(s) = semitones {
                transpose::transpose_music(doc, s)
            } else if let Some(iv) = iv {
                transpose::transpose_interval_music(doc, iv)
            } else {
                transpose::transpose_to_key_music(doc, tonic.unwrap())
            }
        });
    }

    let score = parse_source(input, from)?;
    let transposed = if let Some(s) = semitones {
        transpose::transpose(&score, s)
    } else if let Some(iv) = interval {
        let interval = Interval::from_name(iv).map_err(|e| anyhow::anyhow!(e))?;
        transpose::transpose_interval(&score, interval)
    } else {
        let tonic = parse_tonic(to_key.unwrap())?;
        transpose::transpose_to_key(&score, tonic)
    };
    write_output(&transposed, output, format)?;
    Ok(())
}

fn run_invert(
    input: &Path,
    output: &Path,
    axis: &str,
    format: Option<OutputFormat>,
    from: Option<OutputFormat>,
) -> anyhow::Result<()> {
    let pitch = parse_axis(axis)?;
    if is_ly_to_ly(input, output, format, from) {
        return ly_music_transform(input, output, |doc| invert::invert_music(doc, pitch));
    }
    let score = parse_source(input, from)?;
    let inverted = invert::invert(&score, pitch);
    write_output(&inverted, output, format)?;
    Ok(())
}

fn run_retrograde(
    input: &Path,
    output: &Path,
    format: Option<OutputFormat>,
    from: Option<OutputFormat>,
) -> anyhow::Result<()> {
    if is_ly_to_ly(input, output, format, from) {
        return ly_music_transform(input, output, retrograde::retrograde_music);
    }
    let score = parse_source(input, from)?;
    let reversed = retrograde::retrograde(&score);
    write_output(&reversed, output, format)?;
    Ok(())
}

fn run_change_language(
    input: &Path,
    output: &Path,
    lang: &str,
    format: Option<OutputFormat>,
    from: Option<OutputFormat>,
) -> anyhow::Result<()> {
    let target = PitchLanguage::from_str_loose(lang)
        .ok_or_else(|| anyhow::anyhow!("unknown pitch language: {lang}"))?;
    if is_ly_to_ly(input, output, format, from) {
        return ly_music_transform(input, output, |doc| {
            language::change_language_music(doc, target)
        });
    }
    let score = parse_source(input, from)?;
    let changed = language::change_language(&score, target);
    write_output(&changed, output, format)?;
    Ok(())
}

/// Re-emit a LilyPond file with the given pitch-entry mode (relative/absolute).
///
/// Both `abs2rel` and `rel2abs` parse the source to the (always-absolute) IR
/// and re-emit; only the emission mode differs. LilyPond input/output only.
fn run_relative_mode(input: &Path, output: &Path, mode: PitchMode) -> anyhow::Result<()> {
    if !is_dash(input) && !matches!(detect_input_format(input)?, InputFormat::LilyPond) {
        anyhow::bail!("abs2rel/rel2abs require a LilyPond (.ly) input");
    }
    // Force LilyPond parsing (covers the stdin case, which has no extension).
    let mut score = parse_source(input, Some(OutputFormat::Ly))?;
    score.metadata.pitch_mode = mode;
    let ly = build_ly_adapter(&score).convert(&score)?;
    write_bytes(output, ly.as_bytes())?;
    Ok(())
}

fn run_flatten(
    input: &Path,
    output: Option<&Path>,
    include_paths: &[PathBuf],
    no_markers: bool,
) -> anyhow::Result<()> {
    let opts = FlattenOpts {
        include_paths: include_paths.to_vec(),
        add_markers: !no_markers,
    };
    let text = flatten(input, opts)?;
    match output {
        Some(path) => {
            std::fs::write(path, &text)?;
        }
        None => {
            std::io::stdout().write_all(text.as_bytes())?;
        }
    }
    Ok(())
}

fn run_info(input: &Path, json: bool) -> anyhow::Result<()> {
    let score = parse_source(input, None)?;
    if json {
        print_info_json(&score)
    } else {
        print_info_human(&score);
        Ok(())
    }
}

fn print_info_human(score: &Score) {
    let meta = &score.metadata;
    if let Some(title) = &meta.title {
        println!("Title:    {title}");
    }
    if let Some(composer) = &meta.composer {
        println!("Composer: {composer}");
    }
    if let Some(subtitle) = &meta.subtitle {
        println!("Subtitle: {subtitle}");
    }
    if let Some(arranger) = &meta.arranger {
        println!("Arranger: {arranger}");
    }
    if let Some(lang) = &meta.pitch_language {
        println!("Language: {lang}");
    }

    let parts = score.parts();
    println!("Parts:    {}", parts.len());
    for part in &parts {
        let measures = part.measures.len();
        let name = if part.name.is_empty() {
            &part.part_id
        } else {
            &part.name
        };
        println!("  - {name} ({measures} measures)");
    }
}

/// Emit a curated metadata summary as JSON (for automation). This is a stable,
/// human-meaningful subset — not the full serialized score (use the Python
/// `Score.to_json` for that).
fn print_info_json(score: &Score) -> anyhow::Result<()> {
    let meta = &score.metadata;
    let parts: Vec<_> = score
        .parts()
        .iter()
        .map(|p| {
            serde_json::json!({
                "id": p.part_id,
                "name": p.name,
                "abbreviation": p.abbreviation,
                "measures": p.measures.len(),
                "staves": p.staves,
                "midi_program": p.midi_program,
                "midi_instrument": p.midi_instrument,
            })
        })
        .collect();
    let report = serde_json::json!({
        "title": meta.title,
        "subtitle": meta.subtitle,
        "composer": meta.composer,
        "arranger": meta.arranger,
        "lyricist": meta.lyricist,
        "language": meta.pitch_language.map(|l| l.as_str()),
        "part_count": parts.len(),
        "note_count": collect_pitches(score).len(),
        "parts": parts,
    });
    println!("{}", serde_json::to_string_pretty(&report)?);
    Ok(())
}

/// One entry in a JSON batch-job file.
#[derive(Deserialize)]
struct BatchJob {
    #[serde(alias = "in")]
    input: String,
    #[serde(alias = "out")]
    output: String,
    /// Output format override (else inferred from `output`'s extension).
    format: Option<String>,
    /// Input format override (required if `input` is `-`).
    from: Option<String>,
    /// Optional chromatic transposition in semitones.
    transpose: Option<i32>,
    /// Optional diatonic transposition by interval name (e.g. `"M3"`); takes
    /// precedence over `transpose` if both are given.
    interval: Option<String>,
}

/// Per-job outcome, serialized into the `--report` JSON.
#[derive(Serialize)]
struct JobResult {
    input: String,
    output: String,
    ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

/// Parse a format string (`ly`/`xml`/`midi`/`abc` and common aliases).
fn parse_format_str(s: &str) -> anyhow::Result<OutputFormat> {
    match s.to_ascii_lowercase().as_str() {
        "ly" | "ily" | "lilypond" => Ok(OutputFormat::Ly),
        "xml" | "musicxml" => Ok(OutputFormat::Xml),
        "mxl" => Ok(OutputFormat::Mxl),
        "mid" | "midi" => Ok(OutputFormat::Midi),
        "abc" => Ok(OutputFormat::Abc),
        "krn" | "kern" | "humdrum" => Ok(OutputFormat::Krn),
        _ => anyhow::bail!("unknown format `{s}`"),
    }
}

/// Run a JSON batch-job file. Jobs are isolated (a panic or error fails only that
/// job); the command exits non-zero if any job failed.
fn run_batch_jobs(file: &Path, jobs: usize, report: Option<&Path>) -> anyhow::Result<()> {
    let text = std::fs::read_to_string(file)?;
    let spec: Vec<BatchJob> =
        serde_json::from_str(&text).map_err(|e| anyhow::anyhow!("invalid batch job file: {e}"))?;
    if spec.is_empty() {
        eprintln!("no jobs in {}", file.display());
        return Ok(());
    }

    let run_job = |job: &BatchJob| -> anyhow::Result<()> {
        let from = job.from.as_deref().map(parse_format_str).transpose()?;
        let mut score = parse_source(Path::new(&job.input), from)?;
        if let Some(iv) = &job.interval {
            let interval = Interval::from_name(iv).map_err(|e| anyhow::anyhow!(e))?;
            score = transpose::transpose_interval(&score, interval);
        } else if let Some(s) = job.transpose {
            score = transpose::transpose(&score, s);
        }
        let out_path = Path::new(&job.output);
        if let Some(parent) = out_path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)?;
            }
        }
        let fmt = job.format.as_deref().map(parse_format_str).transpose()?;
        write_output(&score, out_path, fmt)
    };

    let exec = |job: &BatchJob| -> JobResult {
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| run_job(job)));
        let (ok, error) = match outcome {
            Ok(Ok(())) => (true, None),
            Ok(Err(e)) => (false, Some(e.to_string())),
            Err(_) => (false, Some("panicked while processing job".to_string())),
        };
        JobResult {
            input: job.input.clone(),
            output: job.output.clone(),
            ok,
            error,
        }
    };

    let results: Vec<JobResult> = if jobs == 1 {
        spec.iter().map(exec).collect()
    } else {
        let pool = rayon::ThreadPoolBuilder::new().num_threads(jobs).build()?;
        pool.install(|| spec.par_iter().map(exec).collect())
    };

    for r in results.iter().filter(|r| !r.ok) {
        eprintln!("{}: {}", r.input, r.error.as_deref().unwrap_or("failed"));
    }
    if let Some(report_path) = report {
        let json = serde_json::to_string_pretty(&results)?;
        write_bytes(report_path, json.as_bytes())?;
    }

    let failed = results.iter().filter(|r| !r.ok).count();
    eprintln!("Processed {} jobs", results.len());
    if failed > 0 {
        anyhow::bail!("{failed} of {} job(s) failed", results.len());
    }
    Ok(())
}

/// Emit per-part measure positions (start + duration, in quarter notes) as JSON.
fn run_positions(input: &Path, from: Option<OutputFormat>) -> anyhow::Result<()> {
    let score = parse_source(input, from)?;
    let parts: Vec<_> = score
        .parts()
        .iter()
        .map(|part| {
            let mut start = Frac::from_integer(0);
            let measures: Vec<_> = part
                .measures
                .iter()
                .map(|m| {
                    // A measure's musical length = the longest voice in it (handles
                    // pickups and incomplete measures without a time-sig lookup).
                    let dur = m
                        .voices
                        .iter()
                        .map(|v| {
                            v.elements
                                .iter()
                                .map(elem_duration)
                                .fold(Frac::from_integer(0), |a, b| a + b)
                        })
                        .max()
                        .unwrap_or_else(|| Frac::from_integer(0));
                    let entry = serde_json::json!({
                        "number": m.number,
                        "start": frac_to_quarters(start),
                        "duration": frac_to_quarters(dur),
                    });
                    start += dur;
                    entry
                })
                .collect();
            serde_json::json!({ "id": part.part_id, "measures": measures })
        })
        .collect();
    let report = serde_json::json!({ "unit": "quarter", "parts": parts });
    println!("{}", serde_json::to_string_pretty(&report)?);
    Ok(())
}

fn elem_duration(elem: &VoiceElement) -> Frac {
    match elem {
        VoiceElement::Note(n) => n.duration.actual_duration(),
        VoiceElement::Rest(r) => r.duration.actual_duration(),
        VoiceElement::Chord(c) => c.duration.actual_duration(),
    }
}

/// Convert a whole-note-unit fraction to quarter-note units.
fn frac_to_quarters(f: Frac) -> f64 {
    *f.numer() as f64 / *f.denom() as f64 * 4.0
}

/// Export each part of a score to its own single-part file in `output_dir`.
fn run_bundle(
    input: &Path,
    output_dir: &Path,
    format: OutputFormat,
    from: Option<OutputFormat>,
) -> anyhow::Result<()> {
    let score = parse_source(input, from)?;
    let parts = score.parts();
    if parts.is_empty() {
        anyhow::bail!("no parts to export");
    }

    std::fs::create_dir_all(output_dir)?;
    let stem = if is_dash(input) {
        "score"
    } else {
        input
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("score")
    };
    let ext = match format {
        OutputFormat::Ly => "ly",
        OutputFormat::Xml => "xml",
        OutputFormat::Mxl => "mxl",
        OutputFormat::Midi => "mid",
        OutputFormat::Abc => "abc",
        OutputFormat::Krn => "krn",
    };

    for (i, part) in parts.iter().enumerate() {
        // A standalone score holding just this part (metadata/layout carried over).
        let mut single = Score::new();
        single.metadata = score.metadata.clone();
        single.page_layout = score.page_layout.clone();
        single.children.push(ScoreChild::Part((*part).clone()));

        let label = part_label(part, i);
        let path = output_dir.join(format!("{stem}_{label}.{ext}"));
        write_output(&single, &path, Some(format))?;
        eprintln!("Wrote part {} → {}", i + 1, path.display());
    }
    Ok(())
}

/// A filesystem-safe label for a part: its name or id, else `partNN`.
fn part_label(part: &_core::ir::Part, index: usize) -> String {
    let base = if !part.name.is_empty() {
        part.name.as_str()
    } else {
        part.part_id.as_str()
    };
    let cleaned: String = base
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect();
    let cleaned = cleaned.trim_matches('_').to_string();
    if cleaned.is_empty() {
        format!("part{:02}", index + 1)
    } else {
        cleaned
    }
}

/// Compare two scores on sounding content. Exits non-zero when they differ.
fn run_diff(a: &Path, b: &Path, json: bool, from: Option<OutputFormat>) -> anyhow::Result<()> {
    let sa = parse_source(a, from)?;
    let sb = parse_source(b, from)?;

    let pitches_a = collect_pitches(&sa);
    let pitches_b = collect_pitches(&sb);
    let parts_a = sa.parts().len();
    let parts_b = sb.parts().len();
    let pitches_equal = pitches_a == pitches_b;
    let equal = pitches_equal && parts_a == parts_b;

    if json {
        let report = serde_json::json!({
            "equal": equal,
            "parts": { "a": parts_a, "b": parts_b },
            "note_count": { "a": pitches_a.len(), "b": pitches_b.len() },
            "pitch_multiset_equal": pitches_equal,
        });
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        println!("parts:          {parts_a} vs {parts_b}");
        println!("notes:          {} vs {}", pitches_a.len(), pitches_b.len());
        println!("pitches equal:  {pitches_equal}");
        println!(
            "{}",
            if equal {
                "scores are semantically equal"
            } else {
                "scores differ"
            }
        );
    }

    // `diff`-style exit code so automation/CI can branch on the result.
    if !equal {
        std::process::exit(1);
    }
    Ok(())
}

/// All sounding pitches in a score as a sorted MIDI-number multiset.
fn collect_pitches(score: &Score) -> Vec<i32> {
    let mut pitches = Vec::new();
    for part in score.parts() {
        for measure in &part.measures {
            for voice in &measure.voices {
                for elem in &voice.elements {
                    match elem {
                        VoiceElement::Note(n) => pitches.push(n.pitch.midi_number()),
                        VoiceElement::Chord(c) => {
                            pitches.extend(c.notes.iter().map(|n| n.pitch.midi_number()));
                        }
                        VoiceElement::Rest(_) => {}
                    }
                }
            }
        }
    }
    pitches.sort_unstable();
    pitches
}

// ---------------------------------------------------------------------------
// Batch processing
// ---------------------------------------------------------------------------

fn run_batch(
    input_dir: &Path,
    output_dir: &Path,
    format: Option<OutputFormat>,
    jobs: usize,
) -> anyhow::Result<()> {
    let files = collect_input_files(input_dir)?;
    if files.is_empty() {
        eprintln!("No supported files found in {}", input_dir.display());
        return Ok(());
    }

    std::fs::create_dir_all(output_dir)?;

    use std::sync::atomic::{AtomicUsize, Ordering};
    let failures = AtomicUsize::new(0);

    if jobs == 1 {
        for file in &files {
            if let Err(e) = process_one_file_caught(file, input_dir, output_dir, format) {
                eprintln!("{}: {e}", file.display());
                failures.fetch_add(1, Ordering::Relaxed);
            }
        }
    } else {
        // Configure rayon thread pool (0 = rayon's default: one thread per CPU).
        let pool = rayon::ThreadPoolBuilder::new().num_threads(jobs).build()?;

        pool.install(|| {
            files.par_iter().for_each(|file| {
                if let Err(e) = process_one_file_caught(file, input_dir, output_dir, format) {
                    eprintln!("{}: {e}", file.display());
                    failures.fetch_add(1, Ordering::Relaxed);
                }
            });
        });
    }

    let failed = failures.load(Ordering::Relaxed);
    eprintln!("Processed {} files", files.len());
    // A batch where some files failed must not report success: automation
    // should be able to detect partial failures from the exit code.
    if failed > 0 {
        anyhow::bail!("{failed} of {} file(s) failed to convert", files.len());
    }
    Ok(())
}

/// Run [`process_one_file`], converting a panic into an error so that a single
/// pathological file fails only itself instead of unwinding out of the rayon
/// worker and aborting the entire batch. The underlying panics are also fixed at
/// the source (Phase 2 robustness work); this is defense-in-depth.
fn process_one_file_caught(
    file: &Path,
    input_dir: &Path,
    output_dir: &Path,
    format: Option<OutputFormat>,
) -> anyhow::Result<()> {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        process_one_file(file, input_dir, output_dir, format)
    })) {
        Ok(result) => result,
        Err(_) => anyhow::bail!("panicked while processing file"),
    }
}

fn process_one_file(
    file: &Path,
    input_dir: &Path,
    output_dir: &Path,
    format: Option<OutputFormat>,
) -> anyhow::Result<()> {
    let relative = file
        .strip_prefix(input_dir)
        .ok()
        .or_else(|| file.file_name().map(Path::new))
        .unwrap_or(file);

    let out_ext = match format {
        Some(OutputFormat::Ly) => "ly",
        Some(OutputFormat::Xml) => "xml",
        Some(OutputFormat::Mxl) => "mxl",
        Some(OutputFormat::Midi) => "mid",
        Some(OutputFormat::Abc) => "abc",
        Some(OutputFormat::Krn) => "krn",
        None => invert_ext(file),
    };
    let out_path = output_dir.join(relative).with_extension(out_ext);

    if let Some(parent) = out_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let score = parse_source(file, None)?;
    write_output(&score, &out_path, format)?;
    Ok(())
}

/// Collect all supported music files from a directory tree.
fn collect_input_files(dir: &Path) -> anyhow::Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    collect_recursive(dir, &mut files)?;
    files.sort();
    Ok(files)
}

fn collect_recursive(dir: &Path, out: &mut Vec<PathBuf>) -> anyhow::Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            collect_recursive(&path, out)?;
        } else if is_supported_ext(&path) {
            out.push(path);
        }
    }
    Ok(())
}

fn is_supported_ext(path: &Path) -> bool {
    let ext = path.extension().and_then(|e| e.to_str());
    matches!(
        ext,
        Some("ly" | "ily" | "xml" | "musicxml" | "mxl" | "mid" | "midi" | "abc" | "krn" | "kern")
    )
}

/// Infer the "opposite" output extension for convert.
fn invert_ext(path: &Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()) {
        Some("ly" | "ily") => "xml",
        Some("xml" | "musicxml" | "mxl") => "ly",
        Some("mid" | "midi") => "ly",
        Some("abc") => "ly",
        Some("krn" | "kern") => "ly",
        _ => "ly",
    }
}

// ---------------------------------------------------------------------------
// Parse / write helpers
// ---------------------------------------------------------------------------

/// Is this path the stdin/stdout sentinel `-`?
fn is_dash(path: &Path) -> bool {
    path.as_os_str() == "-"
}

fn detect_input_format(path: &Path) -> anyhow::Result<InputFormat> {
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
    match ext {
        "ly" | "ily" => Ok(InputFormat::LilyPond),
        "xml" | "musicxml" | "mxl" => Ok(InputFormat::MusicXml),
        "mid" | "midi" => Ok(InputFormat::Midi),
        "abc" => Ok(InputFormat::Abc),
        "krn" | "kern" => Ok(InputFormat::Humdrum),
        _ => Err(anyhow::anyhow!("unsupported input format: .{ext}")),
    }
}

#[derive(Clone, Copy)]
enum InputFormat {
    LilyPond,
    MusicXml,
    Midi,
    Abc,
    Humdrum,
}

fn to_input_format(f: OutputFormat) -> InputFormat {
    match f {
        OutputFormat::Ly => InputFormat::LilyPond,
        OutputFormat::Xml | OutputFormat::Mxl => InputFormat::MusicXml,
        OutputFormat::Midi => InputFormat::Midi,
        OutputFormat::Abc => InputFormat::Abc,
        OutputFormat::Krn => InputFormat::Humdrum,
    }
}

/// Resolve the input format from an explicit `--from` override, else the file
/// extension. Reading stdin (`-`) without `--from` is an error.
fn resolve_input_format(path: &Path, from: Option<OutputFormat>) -> anyhow::Result<InputFormat> {
    if let Some(f) = from {
        return Ok(to_input_format(f));
    }
    if is_dash(path) {
        anyhow::bail!("reading from stdin (`-`) requires --from <ly|xml|midi|abc>");
    }
    detect_input_format(path)
}

fn read_input_bytes(path: &Path) -> anyhow::Result<Vec<u8>> {
    if is_dash(path) {
        let mut buf = Vec::new();
        std::io::stdin().read_to_end(&mut buf)?;
        Ok(buf)
    } else {
        Ok(std::fs::read(path)?)
    }
}

/// Parse a single score from a file or stdin (`-`), honoring a `--from` override.
fn parse_source(path: &Path, from: Option<OutputFormat>) -> anyhow::Result<Score> {
    let fmt = resolve_input_format(path, from)?;
    if is_dash(path) {
        parse_bytes(&read_input_bytes(path)?, fmt)
    } else {
        parse_file(path, fmt)
    }
}

fn parse_file(path: &Path, fmt: InputFormat) -> anyhow::Result<Score> {
    let score = match fmt {
        InputFormat::LilyPond => LyToIrAdapter::new().convert_file(path)?,
        InputFormat::MusicXml => MxmlToIrAdapter::new().convert_file(path)?,
        InputFormat::Midi => MidiToIrAdapter::new().convert_bytes(&std::fs::read(path)?)?,
        InputFormat::Abc => AbcToIrAdapter::new().convert_file(path)?,
        InputFormat::Humdrum => HumdrumToIrAdapter::new().convert_file(path)?,
    };
    Ok(score)
}

fn parse_bytes(bytes: &[u8], fmt: InputFormat) -> anyhow::Result<Score> {
    let score = match fmt {
        InputFormat::LilyPond => LyToIrAdapter::new().convert_str(std::str::from_utf8(bytes)?)?,
        // `convert_bytes` handles both plain XML and zipped MXL.
        InputFormat::MusicXml => MxmlToIrAdapter::new().convert_bytes(bytes)?,
        InputFormat::Midi => MidiToIrAdapter::new().convert_bytes(bytes)?,
        InputFormat::Abc => AbcToIrAdapter::new().convert_str(std::str::from_utf8(bytes)?)?,
        InputFormat::Humdrum => {
            HumdrumToIrAdapter::new().convert_str(std::str::from_utf8(bytes)?)?
        }
    };
    Ok(score)
}

/// Parse one or more scores. Multi-movement only applies to LilyPond files
/// (stdin and the other formats yield a single score).
fn parse_source_multi(path: &Path, from: Option<OutputFormat>) -> anyhow::Result<Vec<Score>> {
    let fmt = resolve_input_format(path, from)?;
    if matches!(fmt, InputFormat::LilyPond) && !is_dash(path) {
        Ok(LyToIrAdapter::new().convert_file_multi(path)?)
    } else {
        Ok(vec![parse_source(path, from)?])
    }
}

fn detect_output_format(path: &Path, forced: Option<OutputFormat>) -> anyhow::Result<OutputFormat> {
    if let Some(f) = forced {
        return Ok(f);
    }
    if is_dash(path) {
        anyhow::bail!("writing to stdout (`-`) requires --format <ly|xml|midi|abc>");
    }
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
    match ext {
        "ly" | "ily" => Ok(OutputFormat::Ly),
        "xml" | "musicxml" => Ok(OutputFormat::Xml),
        "mxl" => Ok(OutputFormat::Mxl),
        "mid" | "midi" => Ok(OutputFormat::Midi),
        "abc" => Ok(OutputFormat::Abc),
        "krn" | "kern" => Ok(OutputFormat::Krn),
        _ => Err(anyhow::anyhow!(
            "cannot infer output format from .{ext}; use --format"
        )),
    }
}

/// Render a score to bytes in the given format.
fn render_output(score: &Score, fmt: OutputFormat) -> anyhow::Result<Vec<u8>> {
    let bytes = match fmt {
        OutputFormat::Ly => build_ly_adapter(score).convert(score)?.into_bytes(),
        OutputFormat::Xml => IrToMxmlAdapter::new().convert(score)?.into_bytes(),
        OutputFormat::Mxl => IrToMxmlAdapter::new().convert_mxl_bytes(score)?,
        OutputFormat::Midi => IrToMidiAdapter::new().convert_bytes(score)?,
        OutputFormat::Abc => {
            let doc = _core::ir::lift::lift_to_music(score);
            IrToAbcAdapter::new().convert_music(&doc)?.into_bytes()
        }
        OutputFormat::Krn => IrToHumdrumAdapter::new().convert(score)?.into_bytes(),
    };
    Ok(bytes)
}

fn write_bytes(path: &Path, bytes: &[u8]) -> anyhow::Result<()> {
    if is_dash(path) {
        std::io::stdout().write_all(bytes)?;
    } else {
        std::fs::write(path, bytes)?;
    }
    Ok(())
}

fn write_output(score: &Score, path: &Path, format: Option<OutputFormat>) -> anyhow::Result<()> {
    let fmt = detect_output_format(path, format)?;
    let bytes = render_output(score, fmt)?;
    write_bytes(path, &bytes)
}

/// Convert LilyPond → LilyPond through the Music tree (Layer 1) for better
/// structural preservation. Supports stdin/stdout via `-`.
fn convert_ly_to_ly(input: &Path, output: &Path) -> anyhow::Result<()> {
    let parser = LyToIrAdapter::new();
    let doc = if is_dash(input) {
        let bytes = read_input_bytes(input)?;
        parser.convert_str_to_music(std::str::from_utf8(&bytes)?)?
    } else {
        parser.convert_file_to_music(input)?
    };
    let emitter = IrToLyAdapter::new();
    let ly = emitter.convert_music(&doc)?;
    write_bytes(output, ly.as_bytes())
}

/// Build an `IrToLyAdapter` respecting the score's language metadata.
fn build_ly_adapter(score: &Score) -> IrToLyAdapter {
    let mut adapter = IrToLyAdapter::new();
    if let Some(lang) = score.metadata.pitch_language {
        adapter = adapter.with_language(lang);
    }
    adapter
}

/// Parse an axis pitch like `c4`, `fs3`, `bf5` (step + optional accidentals +
/// octave). `s`/`#` raise, `f` lowers; accidentals may repeat (e.g. `css4`).
fn parse_axis(s: &str) -> anyhow::Result<Pitch> {
    let trimmed = s.trim();
    let mut chars = trimmed.chars().peekable();
    let step = match chars.next().map(|c| c.to_ascii_lowercase()) {
        Some('c') => PitchStep::C,
        Some('d') => PitchStep::D,
        Some('e') => PitchStep::E,
        Some('f') => PitchStep::F,
        Some('g') => PitchStep::G,
        Some('a') => PitchStep::A,
        Some('b') => PitchStep::B,
        _ => anyhow::bail!("invalid axis pitch `{s}`: expected a note letter a–g (e.g. c4)"),
    };
    let mut alter = 0i32;
    while let Some(&c) = chars.peek() {
        match c.to_ascii_lowercase() {
            's' | '#' => alter += 1,
            'f' => alter -= 1,
            _ => break,
        }
        chars.next();
    }
    let octave_str: String = chars.collect();
    let octave: i32 = octave_str
        .trim()
        .parse()
        .map_err(|_| anyhow::anyhow!("invalid axis octave in `{s}`: expected e.g. c4"))?;
    Ok(if alter == 0 {
        Pitch::new(step, octave)
    } else {
        Pitch::with_alter(step, Alter::from_integer(alter), octave)
    })
}

/// Parse a key tonic like `D`, `Bb`, `F#`, `ef`, `bf` into a pitch (octave 4).
/// Accidentals after the letter: `#`/`s`/`+` sharpen, `b`/`f`/`-` flatten.
fn parse_tonic(s: &str) -> anyhow::Result<Pitch> {
    _core::ir::pitch::parse_tonic(s).map_err(|e| anyhow::anyhow!(e))
}
