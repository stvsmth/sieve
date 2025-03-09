# Sieve

[![codecov](https://codecov.io/gh/stvsmth/sieve/branch/main/graph/badge.svg)](https://codecov.io/gh/stvsmth/sieve)

A tool for filtering lines from gzipped files based on patterns.

## Features

- Process multiple gzipped files in parallel
- Filter out lines containing specified patterns
- Progress bar with ETA
- Configurable thread count
- Locale-aware number formatting

## Usage

```bash
sieve [OPTIONS] <ROOT_DIR> [PATTERNS]...

Arguments:
  <ROOT_DIR>    Root directory to search for .gz files
  [PATTERNS]... Patterns to filter out

Options:
  --threads <THREADS>        Number of threads (defaults to number of logical CPUs)
  --log-output <LOG_OUTPUT>  Log output destination [default: file] [possible values: file, stdout]
  --locale <LOCALE>          Locale for number formatting [default: en]
  -h, --help                 Print help
```

## Development

### Running Tests

```bash
cargo test
```

### Code Coverage

To generate a code coverage report:

1. Run the coverage script:
   ```bash
   ./scripts/coverage.sh
   ```

2. Open the HTML report:
   ```bash
   open coverage/tarpaulin-report.html
   ```
