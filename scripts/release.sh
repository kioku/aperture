#!/bin/bash
set -e

# Release script - updates version, commits, tags, and pushes
# CI handles: crates.io, binaries, GitHub release, Homebrew

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

# Get version from argument or prompt
VERSION="${1:-}"
if [ -z "$VERSION" ]; then
    echo -e "${YELLOW}Enter version (e.g., 0.1.8):${NC}"
    read -r VERSION
fi

# Validate version format
if ! [[ "$VERSION" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
    echo -e "${RED}Error: Invalid version format. Expected: X.Y.Z${NC}"
    exit 1
fi

# Check for uncommitted changes
if ! git diff --quiet || ! git diff --cached --quiet; then
    echo -e "${RED}Error: You have uncommitted changes. Please commit or stash them first.${NC}"
    exit 1
fi

# Check we're on main branch
BRANCH=$(git branch --show-current)
if [ "$BRANCH" != "main" ]; then
    echo -e "${YELLOW}Warning: You're on branch '$BRANCH', not 'main'.${NC}"
    echo "Continue anyway? (y/n)"
    read -r CONFIRM
    if [[ $CONFIRM != "y" ]]; then
        exit 1
    fi
fi

# Get current version
CURRENT_VERSION=$(grep '^version = ' Cargo.toml | head -1 | sed 's/version = "\(.*\)"/\1/')
echo -e "Current version: ${YELLOW}${CURRENT_VERSION}${NC}"
echo -e "New version:     ${GREEN}${VERSION}${NC}"

# Confirm
echo ""
echo "This will:"
echo "  1. Update Cargo.toml to version ${VERSION}"
echo "  2. Commit: 'chore: release v${VERSION}'"
echo "  3. Create tag: v${VERSION}"
echo "  4. Push commit and tag to origin"
echo ""
echo "CI will then handle: crates.io, binaries, GitHub release, Homebrew"
echo ""
echo -e "${YELLOW}Proceed? (y/n)${NC}"
read -r CONFIRM
if [[ $CONFIRM != "y" ]]; then
    echo "Aborted."
    exit 1
fi

# Update Cargo.toml version
echo -e "\n${GREEN}Updating Cargo.toml...${NC}"
sed -i.bak "s/^version = \"${CURRENT_VERSION}\"/version = \"${VERSION}\"/" Cargo.toml
rm -f Cargo.toml.bak

# Update Cargo.lock
echo -e "${GREEN}Updating Cargo.lock...${NC}"
cargo check --quiet 2>/dev/null || cargo generate-lockfile --quiet

# Commit
echo -e "${GREEN}Committing...${NC}"
git add Cargo.toml Cargo.lock
git commit -m "chore: release v${VERSION}"

# Tag
echo -e "${GREEN}Creating tag v${VERSION}...${NC}"
git tag "v${VERSION}"

# Push
echo -e "${GREEN}Pushing to origin...${NC}"
git push && git push --tags

echo ""
echo -e "${GREEN}Done!${NC}"
echo ""
echo "CI is now running. Monitor progress at:"
echo "  https://github.com/kioku/aperture/actions"
echo ""
echo "Release will be available at:"
echo "  https://github.com/kioku/aperture/releases/tag/v${VERSION}"
