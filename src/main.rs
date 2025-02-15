use clap::Parser;
use flate2::read::GzDecoder;
use flate2::write::GzEncoder;
use flate2::Compression;
use rayon::prelude::*;
use std::error::Error;
use std::fs::{self, File};
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::result::Result;
use tempfile::NamedTempFile;
use walkdir::WalkDir;

#[derive(Parser, Debug)]
#[command(author, version, about)]
struct Args {
    /// Root directory containing subdirectories of .gz logs
    root_dir: String,

    /// The patterns to remove from lines
    patterns: Vec<String>,

    /// Number of threads to use
    #[arg(long, default_value = "8")]
    threads: usize,
}

fn main() -> Result<(), Box<dyn Error>> {
    let args = Args::parse();
    let root_dir = PathBuf::from(&args.root_dir);

    // Collect immediate subdirectories of root_dir
    let sub_dirs: Vec<PathBuf> = fs::read_dir(&root_dir)?
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().map(|f| f.is_dir()).unwrap_or(false))
        .map(|entry| entry.path())
        .collect();

    // Build a thread pool with the desired number of threads
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(args.threads)
        .build()
        .unwrap();

    // Process subdirectories in parallel
    pool.install(|| {
        sub_dirs.par_iter().for_each(|dir_path| {
            if let Err(e) = process_directory(dir_path, &args.patterns) {
                eprintln!("Error processing {}: {}", dir_path.display(), e);
            }
        });
    });

    Ok(())
}

/// Recursively traverse a directory, processing every .gz file found.
fn process_directory(dir_path: &Path, patterns: &[String]) -> Result<(), Box<dyn Error>> {
    for entry in WalkDir::new(dir_path) {
        let entry = entry?;
        if !entry.file_type().is_file() {
            continue;
        }
        if let Some("gz") = entry.path().extension().and_then(|ext| ext.to_str()) {
            remove_lines_with_patterns(entry.path(), patterns)?;
        }
    }
    Ok(())
}

/// Remove lines containing any of the given patterns from a .gz file.
fn remove_lines_with_patterns(file_path: &Path, patterns: &[String]) -> Result<(), Box<dyn Error>> {
    let temp_file = NamedTempFile::new()?;

    // Read from .gz
    let f = File::open(file_path)?;
    let gz = GzDecoder::new(f);
    let mut reader = BufReader::new(gz);

    // Write to a temporary .gz
    let out_file = File::create(temp_file.path())?;
    let mut gz_out = GzEncoder::new(BufWriter::new(out_file), Compression::default());

    let mut removed_count = 0u64;
    let mut line = String::new();

    while reader.read_line(&mut line)? > 0 {
        // If any pattern matches the line, skip it
        if patterns.iter().any(|pat| line.contains(pat)) {
            removed_count += 1;
        } else {
            gz_out.write_all(line.as_bytes())?;
        }
        line.clear();
    }

    println!(
        "Processed {}: removed {} lines.",
        file_path.display(),
        removed_count
    );

    // Replace original file with the temp file's contents
    fs::copy(temp_file.path(), file_path)?;

    Ok(())
}
