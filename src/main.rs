use clap::Parser;
use flate2::read::GzDecoder;
use flate2::write::GzEncoder;
use flate2::Compression;
use indicatif::{ProgressBar, ProgressStyle};
use rayon::prelude::*;
use std::error::Error;
use std::fs;
use std::fs::{copy, File};
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::path::PathBuf;
use tempfile::NamedTempFile;
use walkdir::WalkDir;

#[derive(Parser, Debug)]
struct Args {
    /// Root directory
    root_dir: String,

    /// Patterns
    patterns: Vec<String>,

    /// Number of threads
    #[arg(long, default_value = "10")]
    threads: usize,
}

fn main() -> Result<(), Box<dyn Error>> {
    let args = Args::parse();
    let root = PathBuf::from(&args.root_dir);

    // Gather all ".gz" files
    let gz_files: Vec<PathBuf> = WalkDir::new(&root)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|e| {
            e.file_type().is_file() && e.path().extension().and_then(|s| s.to_str()) == Some("gz")
        })
        .map(|e| e.path().to_path_buf())
        .collect();

    // Create a progress bar
    let total_files = gz_files.len() as u64;
    let pb = ProgressBar::new(total_files);
    pb.set_style(
        ProgressStyle::default_bar()
            .template("[{elapsed_precise}] {bar:40.cyan/blue} {pos}/{len} ({eta})")
            .unwrap()
            .progress_chars("##-"),
    );

    // Run in parallel with rayon
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(args.threads)
        .build()?;

    pool.install(|| {
        gz_files.par_iter().for_each(|file_path| {
            if let Err(e) = remove_lines_with_patterns(file_path, &args.patterns) {
                eprintln!("Error processing {}: {}", file_path.display(), e);
            }
            pb.inc(1);
        });
    });

    pb.finish_with_message("Done!");
    Ok(())
}

/// Removes lines containing any pattern from a single .gz file.
fn remove_lines_with_patterns(
    file_path: &PathBuf,
    patterns: &[String],
) -> Result<(u64, u64), Box<dyn Error>> {
    let temp_file = NamedTempFile::new()?;

    // Read from .gz
    let in_file = File::open(file_path)?;
    let gz_in = GzDecoder::new(in_file);
    let mut reader = BufReader::new(gz_in);

    // Write to temporary .gz
    let out_file = File::create(temp_file.path())?;
    let gz_out = GzEncoder::new(BufWriter::new(out_file), Compression::default());
    let mut writer = BufWriter::new(gz_out);

    let mut read_count = 0_u64;
    let mut removed_count = 0_u64;
    let mut line = String::new();

    while reader.read_line(&mut line)? > 0 {
        read_count += 1;
        if patterns.iter().any(|pat| line.contains(pat)) {
            removed_count += 1;
        } else {
            writer.write_all(line.as_bytes())?;
        }
        line.clear();
    }
    writer.flush()?;

    // Explicitly close GzEncoder before replacing original
    drop(writer);

    copy(temp_file.path(), file_path)?;
    Ok((read_count, removed_count))
}

fn gather_gz_files(root: &std::path::Path) -> (Vec<PathBuf>, u64) {
    let mut gz_files = Vec::new();
    let mut total_size = 0_u64;

    // Recursively walk root directory
    for entry in WalkDir::new(root) {
        if let Ok(e) = entry {
            if e.file_type().is_file() {
                if let Some("gz") = e.path().extension().and_then(|s| s.to_str()) {
                    let size = e.metadata().map(|m| m.len()).unwrap_or(0);
                    total_size += size;
                    gz_files.push(e.path().to_path_buf());
                }
            }
        }
    }

    (gz_files, total_size)
}

fn create_progress_bar(total_size: u64) -> Arc<ProgressBar> {
    let pb = ProgressBar::new(total_size);
    pb.set_style(
        ProgressStyle::with_template(
            "[{elapsed_precise}] {bar:40.cyan/blue} {bytes}/{total_bytes} ({eta})",
        )
        .unwrap()
        .progress_chars("=>-"),
    );
    Arc::new(pb)
}

use std::io::{self, Read};
use std::sync::Arc;
use indicatif::ProgressBar;

struct ProgressReader<R: Read> {
    inner: R,
    pb: Arc<ProgressBar>,
}

impl<R: Read> Read for ProgressReader<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let bytes_read = self.inner.read(buf)?;
        if bytes_read > 0 {
            self.pb.inc(bytes_read as u64);
        }
        Ok(bytes_read)
    }
}

impl<R: Read> ProgressReader<R> {
    fn new(inner: R, pb: Arc<ProgressBar>) -> Self {
        ProgressReader { inner, pb }
    }
}
