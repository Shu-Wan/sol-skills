//! `solx cheatsheet` - print the Sol quick-reference as text.
//!
//! Single source of truth: the skill's `references/cheatsheet.md`, embedded
//! at build time. The CLI text, the rendered PDF
//! (`scripts/build-cheatsheet.sh`), and the skill reference all read the same
//! file, so they cannot drift. `solx` is always built from the repo (users
//! get prebuilt binaries, never `cargo publish`), so the relative path
//! resolves.

/// The cheat sheet, embedded from the skill's markdown source.
pub const CHEATSHEET: &str = include_str!("../../skills/sol-skill/references/cheatsheet.md");

/// Print the cheat sheet to stdout. Works anywhere - no Sol required.
pub fn cmd_cheatsheet() -> i32 {
    print!("{CHEATSHEET}");
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cheatsheet_has_the_key_sections() {
        for needle in [
            "Know your access",
            "Partition",
            "QOS",
            "debug",
            "htc",
            "solx",
            "collaborator-owned entries",
            "dirs_touched",
            "failure exits 1",
        ] {
            assert!(CHEATSHEET.contains(needle), "cheatsheet missing {needle:?}");
        }
    }
}
