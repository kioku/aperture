#!/bin/bash
set -e

echo "Creating new release..."

# Prompt for version first
echo "Enter version (e.g., 0.1.6):"
read -r VERSION

# Install tools if needed
if ! command -v cargo-release &>/dev/null; then
  echo "Installing cargo-release..."
  cargo install cargo-release
fi

if ! command -v git-cliff &>/dev/null; then
  echo "Installing git-cliff..."
  cargo install git-cliff
fi

# Update CHANGELOG with git-cliff
echo "Updating CHANGELOG.md..."
git-cliff --unreleased --tag "v${VERSION}" --prepend CHANGELOG.md

# Show what was added
echo "New changelog entry:"
git-cliff --unreleased --tag "v${VERSION}"

# Review and confirm
echo "Review CHANGELOG.md. Continue? (y/n)"
read -r CONFIRM
if [[ $CONFIRM != "y" ]]; then
  git checkout CHANGELOG.md
  exit 1
fi

# Stage and commit the changelog
git add CHANGELOG.md
git commit -m "docs: update changelog for v${VERSION}"

# Run existing checks
cargo test
cargo clippy

# Release (bumps version, commits, tags, pushes)
cargo release "${VERSION}" --execute
