#!/usr/bin/env bash
set -e

# Install tarpaulin if not already installed
if ! command -v cargo-tarpaulin &> /dev/null; then
    echo "Installing cargo-tarpaulin..."
    cargo install cargo-tarpaulin
fi

mkdir -p .coverage
cargo tarpaulin --config .tarpaulin.toml --out Html --out Xml --out Json --output-dir .coverage
