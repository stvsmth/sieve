use clap::Parser;
use flate2::read::GzDecoder;
use flate2::write::GzEncoder;
use flate2::Compression;
use indicatif::{ProgressBar, ProgressStyle};
use rayon::prelude::*;
use std::error::Error;
use std::fs::{copy, File};
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};
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

    // Gather gzipped files with sizes
    let (gz_files, total_size) = gather_gz_files(&root);

    // Create a progress bar
    let pb = ProgressBar::new(total_size);
    pb.set_style(
        ProgressStyle::default_bar()
            .template("[{elapsed_precise}] {bar:40.cyan/blue} {bytes}/{total_bytes} ({eta})")
            .unwrap()
            .progress_chars("##-"),
    );

    // Process each file in parallel, incrementing by its precomputed size
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(args.threads)
        .build()?;
    pool.install(|| {
        gz_files.par_iter().for_each(|(file_path, file_size)| {
            if let Err(e) = remove_lines_with_patterns(file_path, &args.patterns) {
                eprintln!("Error processing {}: {}", file_path.display(), e);
            }
            pb.inc(*file_size); // Increment progress by the file's size
        });
    });

    pb.finish_with_message("Done!");
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

    writer.flush()?; // Ensure compression is finalized
    drop(writer); // Close GzEncoder before replacing file

    // println!(
    //     "Processed {}: removed {} lines.",
    //     file_path.display(),
    //     removed_count
    // );

    // Replace original file
    copy(temp_file.path(), file_path)?;

    Ok((read_count, removed_count))
}
