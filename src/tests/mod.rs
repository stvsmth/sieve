use super::*;
use flate2::Compression;
use flate2::read::GzDecoder;
use flate2::write::GzEncoder;
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

    // Create an empty gzipped file - properly initialize it as a valid gzip file
    {
        let file = File::create(&file_path).unwrap();
        let gz = GzEncoder::new(file, Compression::default());
        let mut writer = BufWriter::new(gz);
        // Just create a valid gzip file with no content
        writer.flush().unwrap();
    }

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
    let result = remove_lines_with_patterns(&file_path, &patterns);

    // With our improved error handling, this should now return an error
    // instead of silently returning (0, 0)
    assert!(result.is_err());
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

#[test]
fn test_parse_args() {
    // Test with specific arguments
    let args = super::parse_args_from(vec![
        "sieve",    // program name
        "/tmp",     // root_dir
        "pattern1", // patterns
        "pattern2",
        "--threads",
        "4",
        "--log-output",
        "stdout",
        "--locale",
        "fr",
    ]);

    // Verify the arguments were parsed correctly
    assert_eq!(args.root_dir, "/tmp");
    assert_eq!(args.patterns, vec!["pattern1", "pattern2"]);
    assert_eq!(args.threads, Some(4));
    assert_eq!(args.log_output, super::LogOutput::Stdout);
    assert_eq!(args.locale, "fr");

    // Test with minimal arguments
    let args = super::parse_args_from(vec!["sieve", "/tmp", "pattern1"]);

    // Verify defaults are applied
    assert_eq!(args.root_dir, "/tmp");
    assert_eq!(args.patterns, vec!["pattern1"]);
    assert_eq!(args.threads, None);
    assert_eq!(args.log_output, super::LogOutput::File); // default
    assert_eq!(args.locale, "en"); // default
}

#[test]
fn test_get_locale() {
    assert_eq!(super::get_locale("en"), Locale::en);
    assert_eq!(super::get_locale("fr"), Locale::fr);
    assert_eq!(super::get_locale("de"), Locale::de);
    assert_eq!(super::get_locale("ja"), Locale::ja);
    assert_eq!(super::get_locale("invalid"), Locale::en); // default
}

#[test]
fn test_cleanup_empty_log_file() {
    let dir = tempdir().unwrap();
    let file_path = dir.path().join("empty.log");

    // Create an empty file
    File::create(&file_path).unwrap();

    // Run the cleanup function
    let result = super::cleanup_empty_log_file(&file_path.to_string_lossy());

    // Should succeed
    assert!(result.is_ok());

    // File should be gone
    assert!(!file_path.exists());

    // Now test with a non-empty file
    let file_path = dir.path().join("non_empty.log");
    let mut file = File::create(&file_path).unwrap();
    file.write_all(b"some content").unwrap();

    let result = super::cleanup_empty_log_file(&file_path.to_string_lossy());

    // Should succeed
    assert!(result.is_ok());

    // File should still exist
    assert!(file_path.exists());
}

#[test]
fn test_setup_logging() {
    // Test stdout logging
    let result = super::setup_logging(&super::LogOutput::Stdout);
    assert!(result.is_ok());
    let log_file_name = result.unwrap();
    assert!(log_file_name.is_none());

    // We can't easily test file logging without mocking filesystem
    // In a real test environment, consider using a mock or a testing-specific
    // implementation of the logger
}

#[test]
fn test_print_summary() {
    // This function only prints to stdout, so we just ensure it doesn't panic
    super::print_summary(100, 10, "en");
    super::print_summary(100, 10, "fr");
    super::print_summary(100, 10, "invalid");
}

#[test]
fn test_process_files() {
    // Create a test directory with some files
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

    // Get file size for progress tracking
    let size = std::fs::metadata(&file_path).unwrap().len();
    let files = vec![(file_path.clone(), size)];

    // Test process_files with patterns
    let patterns = vec!["pattern".to_string()];
    let result = super::process_files(&files, &patterns, size, Some(1));

    assert!(result.is_ok());
    let (read, removed) = result.unwrap();
    assert_eq!(read, 3);
    assert_eq!(removed, 1);

    // Verify file contents were modified
    let file = File::open(&file_path).unwrap();
    let gz = GzDecoder::new(file);
    let reader = BufReader::new(gz);
    let lines: Vec<String> = reader.lines().map(|l| l.unwrap()).collect();
    assert_eq!(lines, vec!["line 1", "line 3"]);
}

// An integration test for the main workflow
#[test]
fn test_main_workflow() {
    // Create a test directory with files
    let dir = tempdir().unwrap();
    let file_path = dir.path().join("test.gz");

    // Create a gzipped file with content
    {
        let file = File::create(&file_path).unwrap();
        let gz = GzEncoder::new(file, Compression::default());
        let mut writer = BufWriter::new(gz);
        for i in 0..10 {
            writeln!(
                writer,
                "line {} {}",
                i,
                if i % 2 == 0 { "REMOVE" } else { "keep" }
            )
            .unwrap();
        }
    }

    // Parse test arguments
    let args = super::parse_args_from(vec!["sieve", &dir.path().to_string_lossy(), "REMOVE"]);

    // Skip logging setup for test (would interfere with test harness logging)
    // let log_file = super::setup_logging(&args.log_output).unwrap();

    // Process the root directory to find gz files
    let root = Path::new(&args.root_dir);
    let (gz_files, total_size) = super::gather_gz_files(root);

    // Process files
    let (total_lines_read, total_lines_removed) =
        super::process_files(&gz_files, &args.patterns, total_size, args.threads).unwrap();

    // Check results
    assert_eq!(total_lines_read, 10);
    assert_eq!(total_lines_removed, 5); // Every other line should be removed

    // Verify file was modified correctly
    let file = File::open(&file_path).unwrap();
    let gz = GzDecoder::new(file);
    let reader = BufReader::new(gz);
    let lines: Vec<String> = reader.lines().map(|l| l.unwrap()).collect();

    assert_eq!(lines.len(), 5);
    for line in &lines {
        assert!(line.contains("keep"));
        assert!(!line.contains("REMOVE"));
    }
}
