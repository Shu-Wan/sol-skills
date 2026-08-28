#!/usr/bin/env bash
# Build the Sol cheatsheet PDF from the skill's Markdown source.
# A small ReportLab renderer turns the single Markdown source into a compact,
# card-based landscape layout. uv provides the pinned Python dependencies.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SRC="$ROOT/skills/sol-skill/references/cheatsheet.md"
OUT="$ROOT/docs/cheatsheet.pdf"

command -v uv >/dev/null || { echo "error: uv not found"; exit 1; }

SOLX_VERSION="$(sed -n 's/^version = "\([^"]*\)"/\1/p' "$ROOT/solx/Cargo.toml" | head -n 1)"
[ -n "$SOLX_VERSION" ] || { echo "error: could not read solx version"; exit 1; }

# Keep GitHub/terminal affordances in the single Markdown source while giving
# the PDF a compact title banner and no source/build note. Map the remaining
# decorative Unicode to ASCII so the PDF renderer stays font-portable.
TMP="$(mktemp --suffix=.md)"
trap 'rm -f "$TMP"' EXIT
sed -e '1d' \
    -e '/^> A rendered PDF lives at/d' \
    -e '/^> (build it with /d' \
    -e '/^> to print this page/d' \
    -e '/^---$/d' \
    -e 's/🌵 *//g' \
    -e 's/≤/<=/g' -e 's/≥/>=/g' \
    -e 's/↔/<->/g' -e 's/→/->/g' \
    "$SRC" > "$TMP"

UV_CACHE_DIR="${UV_CACHE_DIR:-/scratch/$USER/.cache/uv}" \
  uv run --script "$ROOT/scripts/render-cheatsheet.py" \
  "$TMP" "$OUT" --version "$SOLX_VERSION"

echo "wrote $OUT ($(du -h "$OUT" | cut -f1), renderer: ReportLab)"
