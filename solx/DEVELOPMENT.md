# Developing solx

solx is a single native binary (Rust): the CLI for interactive Slurm jobs
and scratch renewal on Sol.

## Behavior contract

These invariants are load-bearing - preserve them when you touch any
user-visible output. The crate's own suite ([Tests](#tests)) is what
locks them.

* JSON renders like `json.dumps(obj, indent=2)`: two-space indent,
  `\uXXXX` escapes for non-ASCII, insertion-ordered keys.
* Results go to stdout, diagnostics to stderr as single plain lines (no
  markup, no color).
* Exit codes: 0 success, 1 runtime failure, 2 usage error / missing
  config / refused action.
* Help/usage and completion scripts are clap-rendered - assert on content,
  not exact wording.

## Module map

| Module            | Contents                                              |
| ----------------- | ----------------------------------------------------- |
| `src/main.rs`     | clap command tree, dispatch, `config show`/`edit`     |
| `src/side.rs`     | Sol host detection (`hostname -a` + kernel hostname)  |
| `src/slurm.rs`    | squeue/scancel/salloc/srun wrappers, jobid resolution |
| `src/config.rs`   | TOML config parsing, `[keep]` rules                   |
| `src/jobs.rs`     | job list/start/stop/jump/time bodies                  |
| `src/keep.rs`     | CSV plan, enumeration, touch pipeline                 |
| `src/init.rs`     | `solx init` starter config + walkthrough              |
| `src/output.rs`   | TTY detection, JSON writer, plain-text diagnostics    |
| `src/completions.rs` | embedded static completion scripts                 |

Notable design decisions:

* **CLI parsing.** clap handles the tree; two paths are parsed by hand
  because their semantics predate clap conventions: the leading global
  flags (`--json`, eager `--version`) and the whole `job start` tail, where
  the first unconsumed bare token - even after `--` - is the template and
  every other leftover token passes through to salloc in order.
* **`[keep]` matching.** `ignore::gitignore::Gitignore` rooted at `/` with
  `matched_path_or_any_parents`, so a bare path pattern matches the
  directory and everything under it and `!` negations win last-match style.
  The keep-matching vectors live as unit tests in `config.rs` and
  `keep.rs`; run them before touching matcher code.
* **Enumeration.** `ignore::WalkBuilder` with every ignore facility off
  (`hidden(false)`, `ignore(false)`, `git_*(false)`, `parents(false)`,
  `follow_links(false)`) - semantics equal `find DIR -type f` plus `find DIR
  -type d`, hidden entries included, symlinks neither touched nor followed.
  Directories are enumerated because touching a file does not move its
  parent's mtime, so the flagged directory's own stamp would otherwise never
  be refreshed.
* **Touch.** `utimensat(AT_FDCWD, path, NULL, 0)` - the "set both stamps to
  now" form, which per utimensat(2) needs only **write permission** on the
  entry. Explicit stamps (what `filetime::set_file_times` passes, even for
  "now") require **ownership** and EPERM on a collaborator's world-writable
  file inside your own `/scratch` tree, which is most of a shared project
  directory; that was issue #47. A missing path is a silent skip and nothing
  is ever created (`touch -c` semantics).
* **Counting.** `files_touched` / `dirs_touched` count entries that actually
  got fresh stamps; `failures` counts failed *operations* - one per entry
  that could not be touched, plus one per directory that could not be
  walked. Per-entry, not per-shard: a `BATCH`-wide failure must not report
  as one.
* **Completion scripts.** `assets/` holds the static bash/zsh/fish scripts,
  embedded via `include_str!`. Edit them as a set so the three shells stay
  in sync with the command surface; `tests/cli.rs` smoke-checks that each
  emits without error.

## Tests

Toolchain setup on Sol (rustup user-install, `CARGO_TARGET_DIR`, crates.io
access, glibc vs musl) is covered in
[`README.md` -> Toolchain on Sol](README.md#toolchain-on-sol).

```console
$ export CARGO_TARGET_DIR=/tmp/solx-target
$ cargo fmt --all --check
$ cargo clippy --all-targets -- -D warnings
$ cargo test
```

* Unit tests live next to each module (slurm parsing, config, keep
  matching/planning, JSON formatting).
* `tests/cli.rs` drives the compiled binary end-to-end with the SLURM mocks
  in `tests/mocks/bin` and a tempdir HOME/XDG, asserting stdout, stderr,
  and exit codes for the core flows.

CI (`.github/workflows/ci.yml`) runs the same three commands (`check`
job) on every push to main and every PR, plus a `build` job that
compiles the portable binary and uploads it (see below).

## Building and installing

A native development build, for running on the same machine:

```console
$ export CARGO_TARGET_DIR=/tmp/solx-target   # keep artifacts off the NFS home
$ cargo build --release                          # -> $CARGO_TARGET_DIR/release/solx
$ cp "$CARGO_TARGET_DIR/release/solx" ~/.local/bin-test/solx
```

This links the host's glibc, so it runs on the box it was built on (Sol
included). For a binary that runs anywhere - the form CI uploads and a
release ships - build the statically linked musl target:

```console
$ rustup target add x86_64-unknown-linux-musl    # one-time; no musl-gcc needed
$ cargo build --release --target x86_64-unknown-linux-musl
```

The result is a self-contained executable (`ldd` reports "statically
linked") with no libc-version dependency.

**From a PR, without a toolchain.** The `build` job attaches the musl
binary as the `solx-x86_64-linux-musl` artifact on every push/PR.
Download it from the PR's *Checks -> Artifacts*, `chmod +x`, and run it
on Sol as-is - no install step, no toolchain:

```console
$ chmod +x solx && ./solx --version
```
