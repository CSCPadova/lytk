//! lytk CLI — batch music notation conversion and augmentation.
//!
//! Subcommands:
//!   convert   Convert files between LilyPond, MusicXML, and MXL formats
//!   transpose Transpose pitches by N semitones
//!   info      Print score metadata

use std::path::{Path, PathBuf};

use clap::{Parser, Subcommand, ValueEnum};
use rayon::prelude::*;

use _core::adapters::{
    ir_to_ly::IrToLyAdapter, ir_to_mxml::IrToMxmlAdapter, ly_to_ir::LyToIrAdapter,
    mxml_to_ir::MxmlToIrAdapter, FromIrAdapter, ToIrAdapter,
};
use _core::ir::Score;
use _core::transforms::transpose;

/// lytk — music notation conversion and augmentation toolkit.
#[derive(Parser)]
#[command(name = "lytk", version, about)]
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
        #[arg(short, long)]
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
}

#[derive(Clone, Copy, ValueEnum)]
enum OutputFormat {
    Ly,
    Xml,
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
        run_batch(input, output, format, jobs, None::<&fn(&Score) -> Score>)
    } else {
        let score = parse_input(input)?;
        write_output(&score, output, format)?;
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

fn run_batch<F>(
    input_dir: &Path,
    output_dir: &Path,
    format: Option<OutputFormat>,
    jobs: usize,
    transform: Option<&F>,
) -> anyhow::Result<()>
where
    F: Fn(&Score) -> Score + Sync + Send,
{
    let files = collect_input_files(input_dir)?;
    if files.is_empty() {
        eprintln!("No supported files found in {}", input_dir.display());
        return Ok(());
    }

    std::fs::create_dir_all(output_dir)?;

    if jobs == 1 {
        for file in &files {
            if let Err(e) =
                process_one_file(file, input_dir, output_dir, format, transform)
            {
                eprintln!("{}: {e}", file.display());
            }
        }
    } else {
        // Configure rayon thread pool
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(if jobs == 0 { 0 } else { jobs })
            .build()?;

        pool.install(|| {
            files.par_iter().for_each(|file| {
                if let Err(e) =
                    process_one_file(file, input_dir, output_dir, format, transform)
                {
                    eprintln!("{}: {e}", file.display());
                }
            });
        });
    }

    eprintln!("Processed {} files", files.len());
    Ok(())
}

fn process_one_file<F>(
    file: &Path,
    input_dir: &Path,
    output_dir: &Path,
    format: Option<OutputFormat>,
    transform: Option<&F>,
) -> anyhow::Result<()>
where
    F: Fn(&Score) -> Score,
{
    let relative = file
        .strip_prefix(input_dir)
        .unwrap_or(file.file_name().map(Path::new).unwrap_or(file));

    let out_ext = match format {
        Some(OutputFormat::Ly) => "ly",
        Some(OutputFormat::Xml) => "xml",
        None => invert_ext(file),
    };
    let out_path = output_dir.join(relative).with_extension(out_ext);

    if let Some(parent) = out_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let mut score = parse_input(file)?;
    if let Some(t) = transform {
        score = t(&score);
    }
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
    matches!(
        path.extension().and_then(|e| e.to_str()),
        Some("ly" | "ily" | "xml" | "musicxml" | "mxl")
    )
}

/// Infer the "opposite" output extension for convert.
fn invert_ext(path: &Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()) {
        Some("ly" | "ily") => "xml",
        Some("xml" | "musicxml" | "mxl") => "ly",
        _ => "ly",
    }
}

// ---------------------------------------------------------------------------
// Parse / write helpers
// ---------------------------------------------------------------------------

fn detect_input_format(path: &Path) -> anyhow::Result<InputFormat> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("");
    match ext {
        "ly" | "ily" => Ok(InputFormat::LilyPond),
        "xml" | "musicxml" | "mxl" => Ok(InputFormat::MusicXml),
        _ => Err(anyhow::anyhow!("unsupported input format: .{ext}")),
    }
}

enum InputFormat {
    LilyPond,
    MusicXml,
}

fn parse_input(path: &Path) -> anyhow::Result<Score> {
    let fmt = detect_input_format(path)?;
    let score = match fmt {
        InputFormat::LilyPond => LyToIrAdapter::new().convert_file(path)?,
        InputFormat::MusicXml => MxmlToIrAdapter::new().convert_file(path)?,
    };
    Ok(score)
}

fn detect_output_format(path: &Path, forced: Option<OutputFormat>) -> anyhow::Result<OutputFormat> {
    if let Some(f) = forced {
        return Ok(f);
    }
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("");
    match ext {
        "ly" | "ily" => Ok(OutputFormat::Ly),
        "xml" | "musicxml" => Ok(OutputFormat::Xml),
        _ => Err(anyhow::anyhow!("cannot infer output format from .{ext}; use --format")),
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
    }
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
