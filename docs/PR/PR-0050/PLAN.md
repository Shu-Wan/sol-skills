# Modern Sol cheatsheet PDF

## Problem

Issue [#49](https://github.com/Shu-Wan/solx/issues/49) requested a more
polished `docs/cheatsheet.pdf`. The existing PDF read like a default typeset
document, duplicated its title, wrapped commands poorly, and did not cover the
complete `solx` v1.0.2 command and safety surface.

This PR keeps the Markdown/CLI reference as the content source while turning
the generated PDF into a compact operational cheatsheet.

## Plan

1. Refresh the Markdown reference against the current CLI and Sol routing.
2. Replace the generic Pandoc/LaTeX presentation with a dedicated card-based
   renderer.
3. Generate and visually inspect the two-page landscape PDF.
4. Run documentation, renderer, PDF-content, and Rust test checks.

## Status

- [x] Audit the CLI surface and current PDF.
- [x] Update commands, aliases, JSON behavior, safety rules, and job routing.
- [x] Add the `uv`/ReportLab renderer and build integration.
- [x] Render and inspect every final PDF page.
- [x] Remove the page-one overlay and align the visual system with ASU's
  official primary palette.
- [x] Pass Markdown lint, Ruff, PDF extraction checks, and all Rust tests.

## Decision Log

### 2026-08-28

- Kept `skills/sol-skill/references/cheatsheet.md` as the single content source
  shared by the terminal command and generated PDF.
- Chose a two-page landscape card layout with wider columns, dark command
  panels, and compact decision tables to make scanning faster.
- Derived the displayed version from `solx/Cargo.toml` and pinned renderer
  dependencies through a PEP 723 `uv` script for reproducible builds on Sol.
- Replaced the escaping decorative circle with a contained command-cursor mark
  after inspecting the generated PDF at 3x resolution.
- Adopted ASU Maroon (`#8C1D40`), ASU Gold (`#FFC627`), Rich Black, White, and
  ASU Gray as the core palette; maroon provides structure while gold is
  reserved for decisions, cautions, and navigation rules.
- Replaced numeric card labels with semantic wayfinding labels such as
  `ACCESS`, `ROUTING`, `SAFETY`, and `STORAGE` because the cards are reference
  categories, not ordered steps.
