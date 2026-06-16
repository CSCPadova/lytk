//! lytk CLI — batch music notation conversion and augmentation.
//!
//! Subcommands:
//!   convert   Convert files between LilyPond, MusicXML, and MXL formats
//!   transpose Transpose pitches by N semitones
//!   info      Print score metadata
//!   flatten   Recursively expand \include directives into a single flat file

use std::path::{Path, PathBuf};

use clap::{Parser, Subcommand, ValueEnum};
use rayon::prelude::*;

use _core::adapters::ir_to_midi::IrToMidiAdapter;
use _core::adapters::midi_to_ir::MidiToIrAdapter;
use _core::adapters::{
    abc_to_ir::AbcToIrAdapter,
    ir_to_abc::IrToAbcAdapter,
    ir_to_ly::IrToLyAdapter,
    ir_to_mxml::IrToMxmlAdapter,
    ly_flatten::{flatten, FlattenOpts},
    ly_to_ir::LyToIrAdapter,
    mxml_to_ir::MxmlToIrAdapter,
    FromIrAdapter, FromMusicAdapter, ToIrAdapter, ToMusicAdapter,
};
use _core::ir::Score;
use _core::transforms::transpose;

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
    /// Convert files between LilyPond, MusicXML, and MXL formats.
    Convert {
        /// Input file or directory.
        input: PathBuf,

        /// Output file or directory.
        #[arg(short, long)]
        output: PathBuf,

        /// Force output format (auto-detected from extension by default).
        #[arg(short, long)]
        format: Option<OutputFormat>,

        /// Number of parallel threads for batch mode (0 = auto).
        #[arg(short, long, default_value_t = 0)]
        jobs: usize,
    },

    /// Transpose all pitches by a number of semitones.
    Transpose {
        /// Input file.
        input: PathBuf,

        /// Output file.
        #[arg(short, long)]
        output: PathBuf,

        /// Semitones to transpose (positive = up, negative = down).
        #[arg(short, long, allow_negative_numbers = true)]
        semitones: i32,

        /// Force output format.
        #[arg(short, long)]
        format: Option<OutputFormat>,
    },

    /// Print score metadata (title, composer, parts, measures).
    Info {
        /// Input file.
        input: PathBuf,
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
    Midi,
    Abc,
}

fn main() {
    let cli = Cli::parse();

    let result = match cli.command {
        Command::Convert {
            input,
            output,
            format,
            jobs,
        } => run_convert(&input, &output, format, jobs),
        Command::Transpose {
            input,
            output,
            semitones,
            format,
        } => run_transpose(&input, &output, semitones, format),
        Command::Info { input } => run_info(&input),
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
    jobs: usize,
) -> anyhow::Result<()> {
    if input.is_dir() {
        run_batch(input, output, format, jobs)
    } else {
        // LY→LY single-file: use Music tree path for better structural fidelity
        let in_fmt = detect_input_format(input)?;
        let out_fmt = detect_output_format(output, format)?;
        if matches!(in_fmt, InputFormat::LilyPond) && matches!(out_fmt, OutputFormat::Ly) {
            return convert_ly_to_ly(input, output);
        }

        let scores = parse_input_multi(input)?;
        if scores.len() <= 1 {
            let score = scores.into_iter().next().unwrap_or_else(Score::new);
            write_output(&score, output, format)?;
        } else {
            // Multi-movement: write separate files with _01, _02, etc. suffixes
            let stem = output
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("output");
            let ext = output.extension().and_then(|s| s.to_str()).unwrap_or("xml");
            let parent = output.parent().unwrap_or(Path::new("."));
            for (idx, score) in scores.iter().enumerate() {
                let filename = format!("{}_{:02}.{}", stem, idx + 1, ext);
                let path = parent.join(&filename);
                write_output(score, &path, format)?;
                eprintln!("Wrote movement {} → {}", idx + 1, path.display());
            }
        }
        Ok(())
    }
}

fn run_transpose(
    input: &Path,
    output: &Path,
    semitones: i32,
    format: Option<OutputFormat>,
) -> anyhow::Result<()> {
    let score = parse_input(input)?;
    let transposed = transpose::transpose(&score, semitones);
    write_output(&transposed, output, format)?;
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
            use std::io::Write;
            std::io::stdout().write_all(text.as_bytes())?;
        }
    }
    Ok(())
}

fn run_info(input: &Path) -> anyhow::Result<()> {
    let score = parse_input(input)?;
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

    Ok(())
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
        Some(OutputFormat::Midi) => "mid",
        Some(OutputFormat::Abc) => "abc",
        None => invert_ext(file),
    };
    let out_path = output_dir.join(relative).with_extension(out_ext);

    if let Some(parent) = out_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let score = parse_input(file)?;
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
        Some("ly" | "ily" | "xml" | "musicxml" | "mxl" | "mid" | "midi" | "abc")
    )
}

/// Infer the "opposite" output extension for convert.
fn invert_ext(path: &Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()) {
        Some("ly" | "ily") => "xml",
        Some("xml" | "musicxml" | "mxl") => "ly",
        Some("mid" | "midi") => "ly",
        Some("abc") => "ly",
        _ => "ly",
    }
}

// ---------------------------------------------------------------------------
// Parse / write helpers
// ---------------------------------------------------------------------------

fn detect_input_format(path: &Path) -> anyhow::Result<InputFormat> {
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
    match ext {
        "ly" | "ily" => Ok(InputFormat::LilyPond),
        "xml" | "musicxml" | "mxl" => Ok(InputFormat::MusicXml),
        "mid" | "midi" => Ok(InputFormat::Midi),
        "abc" => Ok(InputFormat::Abc),
        _ => Err(anyhow::anyhow!("unsupported input format: .{ext}")),
    }
}

enum InputFormat {
    LilyPond,
    MusicXml,
    Midi,
    Abc,
}

fn parse_input(path: &Path) -> anyhow::Result<Score> {
    let fmt = detect_input_format(path)?;
    let score = match fmt {
        InputFormat::LilyPond => LyToIrAdapter::new().convert_file(path)?,
        InputFormat::MusicXml => MxmlToIrAdapter::new().convert_file(path)?,
        InputFormat::Midi => {
            let bytes = std::fs::read(path)?;
            MidiToIrAdapter::new().convert_bytes(&bytes)?
        }
        InputFormat::Abc => AbcToIrAdapter::new().convert_file(path)?,
    };
    Ok(score)
}

/// Parse input file, returning multiple scores for multi-movement LilyPond files.
fn parse_input_multi(path: &Path) -> anyhow::Result<Vec<Score>> {
    let fmt = detect_input_format(path)?;
    match fmt {
        InputFormat::LilyPond => Ok(LyToIrAdapter::new().convert_file_multi(path)?),
        InputFormat::MusicXml => Ok(vec![MxmlToIrAdapter::new().convert_file(path)?]),
        InputFormat::Midi => {
            let bytes = std::fs::read(path)?;
            Ok(vec![MidiToIrAdapter::new().convert_bytes(&bytes)?])
        }
        InputFormat::Abc => Ok(vec![AbcToIrAdapter::new().convert_file(path)?]),
    }
}

fn detect_output_format(path: &Path, forced: Option<OutputFormat>) -> anyhow::Result<OutputFormat> {
    if let Some(f) = forced {
        return Ok(f);
    }
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
    match ext {
        "ly" | "ily" => Ok(OutputFormat::Ly),
        "xml" | "musicxml" => Ok(OutputFormat::Xml),
        "mid" | "midi" => Ok(OutputFormat::Midi),
        "abc" => Ok(OutputFormat::Abc),
        _ => Err(anyhow::anyhow!(
            "cannot infer output format from .{ext}; use --format"
        )),
    }
}

fn write_output(score: &Score, path: &Path, format: Option<OutputFormat>) -> anyhow::Result<()> {
    let fmt = detect_output_format(path, format)?;
    match fmt {
        OutputFormat::Ly => {
            let adapter = build_ly_adapter(score);
            adapter.write(score, path)?;
        }
        OutputFormat::Xml => {
            IrToMxmlAdapter::new().write(score, path)?;
        }
        OutputFormat::Midi => {
            IrToMidiAdapter::new().write(score, path)?;
        }
        OutputFormat::Abc => {
            let doc = _core::ir::lift::lift_to_music(score);
            IrToAbcAdapter::new().write_music(&doc, path)?;
        }
    }
    Ok(())
}

/// Convert LilyPond → LilyPond through the Music tree (Layer 1) for better
/// structural preservation.
fn convert_ly_to_ly(input: &Path, output: &Path) -> anyhow::Result<()> {
    let parser = LyToIrAdapter::new();
    let doc = parser.convert_file_to_music(input)?;
    let emitter = IrToLyAdapter::new();
    let ly = emitter.convert_music(&doc)?;
    std::fs::write(output, ly)?;
    Ok(())
}

/// Build an `IrToLyAdapter` respecting the score's language metadata.
fn build_ly_adapter(score: &Score) -> IrToLyAdapter {
    let mut adapter = IrToLyAdapter::new();
    if let Some(lang) = score.metadata.pitch_language {
        adapter = adapter.with_language(lang);
    }
    adapter
}
