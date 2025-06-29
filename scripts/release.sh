#!/bin/bash
set -e

# Simple release script
echo "🚀 Creating new release..."

# Install cargo-release if needed
if ! command -v cargo-release &> /dev/null; then
    echo "Installing cargo-release..."
    cargo install cargo-release
fi

# Check everything looks good
cargo test
cargo clippy

# Release (will prompt for version)
cargo release