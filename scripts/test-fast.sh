#!/bin/bash

# Fast test runner script using cargo-nextest for improved performance
# Installs cargo-nextest if not available and runs tests with optimal configuration

set -e

# Colors for output
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
RED='\033[0;31m'
NC='\033[0m' # No Color

echo -e "${GREEN}🚀 Aperture Fast Test Runner${NC}"

# Check if cargo-nextest is installed
if ! command -v cargo-nextest &> /dev/null; then
    echo -e "${YELLOW}⚡ Installing cargo-nextest...${NC}"
    cargo install cargo-nextest --locked
fi

# Default to all tests unless arguments provided
ARGS="$@"
if [ -z "$ARGS" ]; then
    ARGS="--workspace"
fi

# Run with appropriate profile based on environment
if [ "$CI" = "true" ]; then
    echo -e "${GREEN}🔧 Running tests in CI mode...${NC}"
    cargo nextest run --profile ci $ARGS
else
    echo -e "${GREEN}🔧 Running tests in development mode...${NC}"
    cargo nextest run --profile default $ARGS
fi

echo -e "${GREEN}✅ Test run completed${NC}"