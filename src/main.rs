use clap::Parser;
use flate2::read::GzDecoder;
use flate2::write::GzEncoder;
use flate2::Compression;
use indicatif::{ProgressBar, ProgressStyle};
use log::{debug, warn};
use num_format::{Locale, ToFormattedString};
use rayon::prelude::*;
use std::error::Error;
use std::fs::{copy, File};
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
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
    env_logger::init();

    let args = Args::parse();

    let root = Path::new(&args.root_dir).canonicalize()?;

    // Gather gzipped files with sizes
    let (gz_files, total_size) = gather_gz_files(&root);

    // Create a progress bar
    let progress = ProgressBar::new(total_size);
    progress.set_style(
        ProgressStyle::default_bar()
            .template("[{elapsed_precise}] {bar:40.cyan/blue} {bytes}/{total_bytes} ({eta})")
            .unwrap()
            .progress_chars("##-"),
    );

    // Atomic counters for total lines read and removed
    let total_lines_read = Arc::new(AtomicU64::new(0));
    let total_lines_removed = Arc::new(AtomicU64::new(0));

    // Process files in parallel
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(args.threads)
        .build()?;
    pool.install(|| {
        gz_files.par_iter().for_each(|(file_path, file_size)| {
            match remove_lines_with_patterns(file_path, &args.patterns) {
                Ok((read, removed)) => {
                    total_lines_read.fetch_add(read, Ordering::Relaxed);
                    total_lines_removed.fetch_add(removed, Ordering::Relaxed);
                }
                Err(e) => warn!("Error processing {}: {}", file_path.display(), e),
            }
            progress.inc(*file_size);
        });
    });

    progress.finish_with_message("Done!");

    // Print final summary
    // ... add locale-aware separators
    println!(
        "Removed {} lines from a total of {} lines read.",
        total_lines_removed
            .load(Ordering::Relaxed)
            .to_formatted_string(&Locale::en),
        total_lines_read
            .load(Ordering::Relaxed)
            .to_formatted_string(&Locale::en),
    );

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

    debug!(
        "Processed {}: removed {} lines of {} total lines.",
        file_path.display(),
        removed_count,
        read_count,
    );

    // Replace original file
    copy(temp_file.path(), file_path)?;

    Ok((read_count, removed_count))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::tempdir;

    #[test]
    fn test_gather_gz_files() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("test.gz");
        File::create(&file_path).unwrap();

        let (files, total_size) = gather_gz_files(dir.path());
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].0, file_path);
        assert_eq!(total_size, 0);
    }

    #[test]
    fn test_remove_lines_with_patterns() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("test.gz");

        // Create a gzipped file with some content
        {
            let file = File::create(&file_path).unwrap();
            let gz = GzEncoder::new(file, Compression::default());
            let mut writer = BufWriter::new(gz);
            writeln!(writer, "line 1").unwrap();
            writeln!(writer, "line 2 pattern").unwrap();
            writeln!(writer, "line 3").unwrap();
        }

        let patterns = vec!["pattern".to_string()];
        let (read, removed) = remove_lines_with_patterns(&file_path, &patterns).unwrap();

        assert_eq!(read, 3);
        assert_eq!(removed, 1);

        // Verify the content of the file
        let file = File::open(&file_path).unwrap();
        let gz = GzDecoder::new(file);
        let reader = BufReader::new(gz);
        let lines: Vec<String> = reader.lines().map(|l| l.unwrap()).collect();

        assert_eq!(lines, vec!["line 1", "line 3"]);
    }

    #[test]
    fn test_empty_directory() {
        let dir = tempdir().unwrap();
        let (files, total_size) = gather_gz_files(dir.path());
        assert!(files.is_empty());
        assert_eq!(total_size, 0);
    }

    #[test]
    fn test_multiple_patterns() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("test.gz");

        // Create test content
        {
            let file = File::create(&file_path).unwrap();
            let gz = GzEncoder::new(file, Compression::default());
            let mut writer = BufWriter::new(gz);
            writeln!(writer, "keep this line").unwrap();
            writeln!(writer, "remove pattern1").unwrap();
            writeln!(writer, "also pattern2 here").unwrap();
            writeln!(writer, "keep this too").unwrap();
        }

        let patterns = vec!["pattern1".to_string(), "pattern2".to_string()];
        let (read, removed) = remove_lines_with_patterns(&file_path, &patterns).unwrap();

        assert_eq!(read, 4);
        assert_eq!(removed, 2);

        // Verify content
        let file = File::open(&file_path).unwrap();
        let gz = GzDecoder::new(file);
        let reader = BufReader::new(gz);
        let lines: Vec<String> = reader.lines().map(|l| l.unwrap()).collect();

        assert_eq!(lines, vec!["keep this line", "keep this too"]);
    }

    #[test]
    fn test_large_file() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("large.gz");

        // Create large test content
        {
            let file = File::create(&file_path).unwrap();
            let gz = GzEncoder::new(file, Compression::default());
            let mut writer = BufWriter::new(gz);
            for i in 0..1000 {
                writeln!(
                    writer,
                    "line {} {}",
                    i,
                    if i % 10 == 0 { "remove" } else { "keep" }
                )
                .unwrap();
            }
        }

        let patterns = vec!["remove".to_string()];
        let (read, removed) = remove_lines_with_patterns(&file_path, &patterns).unwrap();

        assert_eq!(read, 1000);
        assert_eq!(removed, 100); // Every 10th line should be removed
    }

    #[test]
    fn test_invalid_gz_file() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("invalid.gz");

        // Create an invalid gzip file
        {
            let mut file = File::create(&file_path).unwrap();
            file.write_all(b"not a valid gz file").unwrap();
        }

        let patterns = vec!["pattern".to_string()];
        let result = remove_lines_with_patterns(&file_path, &patterns);
        assert!(result.is_err());
    }
}
