use chrono::Local;
use clap::{Parser, ValueEnum};
use flate2::read::GzDecoder;
use flate2::write::GzEncoder;
use flate2::Compression;
use indicatif::{ProgressBar, ProgressStyle};
use log::{debug, error, set_max_level, warn, LevelFilter};
use num_format::{Locale, ToFormattedString};
use rayon::prelude::*;
use std::fs::OpenOptions;
use std::fs::{copy, File};
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tempfile::NamedTempFile;
use thiserror::Error;
use walkdir::WalkDir;

#[cfg(test)]
mod tests;

#[derive(Error, Debug)]
enum SieveError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Failed to open file {path}: {source}")]
    FileOpen {
        path: String,
        source: std::io::Error,
    },

    #[error("Failed to read line in {path}: {source}")]
    LineRead {
        path: String,
        source: std::io::Error,
    },

    #[error("Failed to process file: {0}")]
    Processing(String),

    #[error("Thread pool error: {0}")]
    ThreadPool(#[from] rayon::ThreadPoolBuildError),
}

#[derive(Parser, Debug)]
struct Args {
    /// Root directory
    root_dir: String,

    /// Patterns
    patterns: Vec<String>,

    /// Number of threads (defaults to number of logical CPUs)
    #[arg(long)]
    threads: Option<usize>,

    /// Log output destination
    #[arg(long, value_enum, default_value = "file")]
    log_output: LogOutput,

    /// Locale for number formatting
    #[arg(long, default_value = "en")]
    locale: String,
}

#[derive(ValueEnum, Clone, Debug, PartialEq)]
enum LogOutput {
    File,
    Stdout,
}

fn main() -> Result<(), SieveError> {
    let args = Args::parse();

    let log_file_name = format!("{}-sieve.log", Local::now().format("%Y-%m-%d-%H-%M-%S"));

    match args.log_output {
        LogOutput::File => {
            let file = OpenOptions::new()
                .create(true)
                .append(true)
                .open(&log_file_name)?;
            let logger = env_logger::Builder::new()
                .target(env_logger::Target::Pipe(Box::new(file)))
                .build();
            set_max_level(LevelFilter::Info);
            log::set_boxed_logger(Box::new(logger)).unwrap();
        }
        LogOutput::Stdout => {
            env_logger::init();
        }
    }

    let root = Path::new(&args.root_dir).canonicalize()?;

    // Gather gzipped files with sizes
    let (gz_files, total_size) = gather_gz_files(&root);

    // Create a progress bar with adaptive width
    let progress = ProgressBar::new(total_size);
    let term_width = match term_size::dimensions() {
        Some((width, _)) => width.max(80),
        None => 80,
    };
    let bar_width = (term_width / 2).clamp(40, 100);

    progress.set_style(
        ProgressStyle::default_bar()
            .template(&format!(
                "[{{elapsed_precise}}] {{bar:{bar_width}.cyan/blue}} {{bytes}}/{{total_bytes}} ({{eta}})"
            ))
            .unwrap()
            .progress_chars("##-"),
    );

    // Atomic counters for total lines read and removed
    let total_lines_read = Arc::new(AtomicU64::new(0));
    let total_lines_removed = Arc::new(AtomicU64::new(0));

    // Use available CPU cores if threads not specified
    let thread_count = args.threads.unwrap_or_else(num_cpus::get);

    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(thread_count)
        .build()?;
    pool.install(|| {
        gz_files.par_iter().for_each(|(file_path, file_size)| {
            match remove_lines_with_patterns(file_path, &args.patterns) {
                Ok((read, removed)) => {
                    total_lines_read.fetch_add(read, Ordering::Relaxed);
                    total_lines_removed.fetch_add(removed, Ordering::Relaxed);
                }
                Err(e) => {
                    warn!("Error processing {}: {}", file_path.display(), e);
                }
            }
            progress.inc(*file_size);
        });
    });

    progress.finish_with_message("Done!");

    // Get locale for number formatting
    let locale = match args.locale.as_str() {
        "fr" => Locale::fr,
        "de" => Locale::de,
        "ja" => Locale::ja,
        _ => Locale::en, // Default to English
    };

    // Print final summary with locale-aware separators
    println!(
        "Removed {} lines from a total of {} lines read.",
        total_lines_removed
            .load(Ordering::Relaxed)
            .to_formatted_string(&locale),
        total_lines_read
            .load(Ordering::Relaxed)
            .to_formatted_string(&locale),
    );

    if args.log_output == LogOutput::File {
        let metadata = std::fs::metadata(&log_file_name)?;
        if metadata.len() == 0 {
            std::fs::remove_file(log_file_name)?;
        }
    }

    Ok(())
}

/// Gather all `.gz` files and compute their sizes.
fn gather_gz_files(root: &Path) -> (Vec<(PathBuf, u64)>, u64) {
    let mut gz_files = Vec::new();
    let mut total_size = 0_u64;

    for entry in WalkDir::new(root).into_iter().flatten() {
        if entry.file_type().is_file()
            && entry.path().extension().and_then(|s| s.to_str()) == Some("gz")
        {
            let size = entry.metadata().map(|m| m.len()).unwrap_or(0);
            total_size += size;
            gz_files.push((entry.path().to_path_buf(), size));
        }
    }

    (gz_files, total_size)
}

/// Removes lines containing any pattern from a single `.gz` file.
fn remove_lines_with_patterns(
    file_path: &PathBuf,
    patterns: &[String],
) -> Result<(u64, u64), SieveError> {
    let temp_file = NamedTempFile::new().map_err(SieveError::Io)?;

    // Read from .gz
    let in_file = File::open(file_path).map_err(|e| SieveError::FileOpen {
        path: file_path.display().to_string(),
        source: e,
    })?;

    let gz_in = GzDecoder::new(in_file);
    let reader = BufReader::new(gz_in);

    // Write to temporary .gz
    let out_file = File::create(temp_file.path()).map_err(SieveError::Io)?;
    let gz_out = GzEncoder::new(BufWriter::new(out_file), Compression::default());
    let mut writer = BufWriter::new(gz_out);

    let mut read_count = 0_u64;
    let mut removed_count = 0_u64;
    for content in reader.lines() {
        match content {
            Ok(mut line) => {
                read_count += 1;
                if patterns.iter().any(|pat| line.contains(pat)) {
                    removed_count += 1;
                } else {
                    writer.write_all(line.as_bytes()).map_err(SieveError::Io)?;
                    writer.write_all(b"\n").map_err(SieveError::Io)?;
                }
                line.clear();
            }
            Err(e) => {
                error!("Failed to read line: {} in {}", e, file_path.display());
                return Err(SieveError::LineRead {
                    path: file_path.display().to_string(),
                    source: e,
                });
            }
        }
    }
    writer.flush().map_err(SieveError::Io)?; // Ensure compression is finalized
    drop(writer); // Close GzEncoder before replacing file

    debug!(
        "Processed {}: removed {} lines of {} total lines.",
        file_path.display(),
        removed_count,
        read_count,
    );

    // Replace original file
    copy(temp_file.path(), file_path)
        .map_err(|e| SieveError::Processing(format!("Failed to replace original file: {e}")))?;

    Ok((read_count, removed_count))
}
