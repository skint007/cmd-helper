#!/usr/bin/env bash
set -euo pipefail

if [ $# -ne 1 ]; then
    echo "Usage: $0 <version>"
    echo "Example: $0 0.2.0"
    exit 1
fi

VERSION="$1"

# Strip leading 'v' if provided
VERSION="${VERSION#v}"

# Validate semver format
if ! [[ "$VERSION" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
    echo "Error: Version must be in semver format (e.g. 1.2.3)"
    exit 1
fi

TAG="v${VERSION}"
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

# Check for uncommitted changes
if ! git -C "$SCRIPT_DIR" diff --quiet || ! git -C "$SCRIPT_DIR" diff --cached --quiet; then
    echo "Error: You have uncommitted changes. Commit or stash them first."
    exit 1
fi

# Check tag doesn't already exist
if git -C "$SCRIPT_DIR" rev-parse "$TAG" &>/dev/null; then
    echo "Error: Tag $TAG already exists."
    exit 1
fi

echo "Bumping version to ${VERSION}..."

# Update Cargo.toml
sed -i "s/^version = \".*\"/version = \"${VERSION}\"/" "$SCRIPT_DIR/Cargo.toml"

# Update Cargo.lock
cargo generate-lockfile --manifest-path "$SCRIPT_DIR/Cargo.toml" 2>/dev/null

# Update AUR PKGBUILD
sed -i "s/^pkgver=.*/pkgver=${VERSION}/" "$SCRIPT_DIR/aur/PKGBUILD"

# Update AUR .SRCINFO
sed -i "s/pkgver = .*/pkgver = ${VERSION}/" "$SCRIPT_DIR/aur/.SRCINFO"
sed -i "s|cmd-helper-[0-9.]*\.tar\.gz|cmd-helper-${VERSION}.tar.gz|g" "$SCRIPT_DIR/aur/.SRCINFO"
sed -i "s|/v[0-9.]*\.tar\.gz|/v${VERSION}.tar.gz|g" "$SCRIPT_DIR/aur/.SRCINFO"

echo "Updated:"
echo "  Cargo.toml      -> ${VERSION}"
echo "  Cargo.lock      -> ${VERSION}"
echo "  aur/PKGBUILD    -> ${VERSION}"
echo "  aur/.SRCINFO    -> ${VERSION}"

# Commit and tag
git -C "$SCRIPT_DIR" add Cargo.toml Cargo.lock aur/PKGBUILD aur/.SRCINFO
git -C "$SCRIPT_DIR" commit -m "Bump version to ${VERSION}"
git -C "$SCRIPT_DIR" tag -a "$TAG" -m "Release ${TAG}"

echo ""
echo "Created commit and tag ${TAG}."
echo "Push with: git push && git push --tags"
