use chrono::Local;
use clap::{Parser, ValueEnum};
use flate2::read::GzDecoder;
use flate2::write::GzEncoder;
use flate2::Compression;
use indicatif::{ProgressBar, ProgressStyle};
use log::{debug, error, set_max_level, warn, LevelFilter};
use num_format::{Locale, ToFormattedString};
use rayon::prelude::*;
use std::error::Error;
use std::fs::OpenOptions;
use std::fs::{copy, File};
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tempfile::NamedTempFile;
use walkdir::WalkDir;

// TODO:
//  * Error Handling: The code uses Box<dyn Error> for error handling, which can be
//    less informative than using specific error types. Consider using a custom error
//    type or `thiserror` crate for more detailed error handling.
//  * Logging Levels:  Ensure that the logging configuration in env_logger::init() is
//    set to capture these levels as needed.
//  * Progress Bar Template: Consider making progress bar width configurable or
//    adaptive to terminal width.
//  * Ensure that the number of threads (args.threads) is appropriate for the system's
//    capabilities. Maybe set a sensible default based on the # of available cores.
//  * Locale Handling: Consider removing the hard-coded locale for number formatting

#[derive(Parser, Debug)]
struct Args {
    /// Root directory
    root_dir: String,

    /// Patterns
    patterns: Vec<String>,

    /// Number of threads
    #[arg(long, default_value = "10")]
    threads: usize,

    /// Log output destination
    #[arg(long, value_enum, default_value = "file")]
    log_output: LogOutput,
}

#[derive(ValueEnum, Clone, Debug, PartialEq)]
enum LogOutput {
    File,
    Stdout,
}

fn main() -> Result<(), Box<dyn Error>> {
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
                Err(e) => {
                    warn!("Error processing {}: {}", file_path.display(), e);
                }
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
) -> Result<(u64, u64), Box<dyn Error>> {
    let temp_file = NamedTempFile::new()?;

    // Read from .gz
    let in_file = match File::open(file_path) {
        Ok(file) => file,
        Err(e) => {
            warn!("Failed to open file {}: {}", file_path.display(), e);
            return Err(Box::new(e));
        }
    };
    let gz_in = GzDecoder::new(in_file);
    let reader = BufReader::new(gz_in);

    // Write to temporary .gz
    let out_file = File::create(temp_file.path())?;
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
                    writer.write_all(line.as_bytes())?;
                    writer.write_all(b"\n")?;
                }
                line.clear();
            }
            Err(e) => {
                error!("Failed to read line: {} in {}", e, file_path.display());
                return Ok((0, 0));
            }
        }
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
    fn test_no_patterns() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("test.gz");

        // Create a gzipped file with some content
        {
            let file = File::create(&file_path).unwrap();
            let gz = GzEncoder::new(file, Compression::default());
            let mut writer = BufWriter::new(gz);
            writeln!(writer, "line 1").unwrap();
            writeln!(writer, "line 2").unwrap();
            writeln!(writer, "line 3").unwrap();
        }

        let patterns: Vec<String> = vec![];
        let (read, removed) = remove_lines_with_patterns(&file_path, &patterns).unwrap();

        assert_eq!(read, 3);
        assert_eq!(removed, 0);

        // Verify the content of the file
        let file = File::open(&file_path).unwrap();
        let gz = GzDecoder::new(file);
        let reader = BufReader::new(gz);
        let lines: Vec<String> = reader.lines().map(|l| l.unwrap()).collect();

        assert_eq!(lines, vec!["line 1", "line 2", "line 3"]);
    }

    #[test]
    fn test_non_existent_patterns() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("test.gz");

        // Create a gzipped file with some content
        {
            let file = File::create(&file_path).unwrap();
            let gz = GzEncoder::new(file, Compression::default());
            let mut writer = BufWriter::new(gz);
            writeln!(writer, "line 1").unwrap();
            writeln!(writer, "line 2").unwrap();
            writeln!(writer, "line 3").unwrap();
        }

        let patterns = vec!["nonexistent".to_string()];
        let (read, removed) = remove_lines_with_patterns(&file_path, &patterns).unwrap();

        assert_eq!(read, 3);
        assert_eq!(removed, 0);

        // Verify the content of the file
        let file = File::open(&file_path).unwrap();
        let gz = GzDecoder::new(file);
        let reader = BufReader::new(gz);
        let lines: Vec<String> = reader.lines().map(|l| l.unwrap()).collect();

        assert_eq!(lines, vec!["line 1", "line 2", "line 3"]);
    }

    #[test]
    fn test_special_characters_in_patterns() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("test.gz");

        // Create a gzipped file with some content
        {
            let file = File::create(&file_path).unwrap();
            let gz = GzEncoder::new(file, Compression::default());
            let mut writer = BufWriter::new(gz);
            writeln!(writer, "line 1").unwrap();
            writeln!(writer, "line 2 special*chars").unwrap();
            writeln!(writer, "line 3").unwrap();
        }

        let patterns = vec!["special*chars".to_string()];
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
    fn test_empty_files() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("empty.gz");

        // Create an empty gzipped file
        File::create(&file_path).unwrap();

        let patterns = vec!["pattern".to_string()];
        let (read, removed) = remove_lines_with_patterns(&file_path, &patterns).unwrap();

        assert_eq!(read, 0);
        assert_eq!(removed, 0);
    }

    #[test]
    fn test_large_patterns_list() {
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

        let patterns: Vec<String> = (0..1000).map(|i| format!("pattern{}", i)).collect();
        let (read, removed) = remove_lines_with_patterns(&file_path, &patterns).unwrap();

        assert_eq!(read, 3);
        assert_eq!(removed, 0);

        // Verify the content of the file
        let file = File::open(&file_path).unwrap();
        let gz = GzDecoder::new(file);
        let reader = BufReader::new(gz);
        let lines: Vec<String> = reader.lines().map(|l| l.unwrap()).collect();

        assert_eq!(lines, vec!["line 1", "line 2 pattern", "line 3"]);
    }

    #[test]
    fn test_nested_directories() {
        let dir = tempdir().unwrap();
        let nested_dir = dir.path().join("nested");
        std::fs::create_dir(&nested_dir).unwrap();
        let file_path = nested_dir.join("test.gz");

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
    fn test_read_only_files() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("readonly.gz");

        // Create a gzipped file with some content
        {
            let file = File::create(&file_path).unwrap();
            let gz = GzEncoder::new(file, Compression::default());
            let mut writer = BufWriter::new(gz);
            writeln!(writer, "line 1").unwrap();
            writeln!(writer, "line 2 pattern").unwrap();
            writeln!(writer, "line 3").unwrap();
        }

        // Set the file to read-only
        let mut perms = std::fs::metadata(&file_path).unwrap().permissions();
        perms.set_readonly(true);
        std::fs::set_permissions(&file_path, perms).unwrap();

        let patterns = vec!["pattern".to_string()];
        let result = remove_lines_with_patterns(&file_path, &patterns);

        assert!(result.is_err());
    }

    #[test]
    fn test_files_of_different_compression_levels() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("test.gz");

        // Create a gzipped file with some content at different compression levels
        for level in 0..=9 {
            {
                let file = File::create(&file_path).unwrap();
                let gz = GzEncoder::new(file, Compression::new(level));
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
    }

    #[test]
    fn test_files_containing_only_patterns() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("test.gz");

        // Create a gzipped file with content that matches the pattern
        {
            let file = File::create(&file_path).unwrap();
            let gz = GzEncoder::new(file, Compression::default());
            let mut writer = BufWriter::new(gz);
            writeln!(writer, "pattern").unwrap();
            writeln!(writer, "pattern").unwrap();
        }

        let patterns = vec!["pattern".to_string()];
        let (read, removed) = remove_lines_with_patterns(&file_path, &patterns).unwrap();

        assert_eq!(read, 2);
        assert_eq!(removed, 2);

        // Verify the content of the file
        let file = File::open(&file_path).unwrap();
        let gz = GzDecoder::new(file);
        let reader = BufReader::new(gz);
        let lines: Vec<String> = reader.lines().map(|l| l.unwrap()).collect();

        assert!(lines.is_empty());
    }

    #[test]
    fn test_files_containing_binary_data() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("binary.gz");

        // Create a gzipped file with binary data
        {
            let file = File::create(&file_path).unwrap();
            let gz = GzEncoder::new(file, Compression::default());
            let mut writer = BufWriter::new(gz);
            writer.write_all(&[0, 159, 146, 150]).unwrap(); // Some binary data
        }

        let patterns = vec!["pattern".to_string()];
        let (read, removed) = remove_lines_with_patterns(&file_path, &patterns).unwrap();

        assert_eq!(read, 0);
        assert_eq!(removed, 0);
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
        let result = remove_lines_with_patterns(&file_path, &patterns).unwrap();
        assert!(result.0 == 0 && result.1 == 0);
    }
}
