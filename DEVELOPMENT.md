# Development

This doc is for contributors and the maintainer. End-user docs live in
`README.md` (CLI + skill install) and `skills/sol-skill/SKILL.md` (skill
content). Public-facing test methodology lives in
[`docs/coverage.md`](docs/coverage.md).

## Repository layout

```text
solx/                               # the repo
├── README.md                       # end-user entry point (CLI + skill)
├── DEVELOPMENT.md                  # you are here (skill + eval harness)
├── .github/workflows/              # ci.yml (lint+test+build) · release.yml (musl binary + GH release on tag)
├── docs/
│   ├── ROADMAP.md                  # roadmap
│   ├── solx.md                     # solx user manual
│   └── coverage.md                 # public methodology + coverage matrix
├── solx/                           # the solx CLI crate (Rust; see solx/DEVELOPMENT.md)
├── skills/sol-skill/               # the shipped skill (what users install)
│   ├── SKILL.md
│   └── references/                 # solx, module, scratch, slurm, sessions, sharing
└── evals/                          # eval harness (not shipped with the skill)
    ├── README.md
    ├── evals.example.json          # sanitized template
    ├── evals.json                  # gitignored - maintainer's real prompts
    ├── mocks/                      # userland Sol mock environment
    │   ├── activate.sh
    │   ├── bin/                    # PATH shims (hostname, module, srun, ...)
    │   └── home/                   # fake $HOME with solx config ([keep]) + CSV warnings
    ├── runner/                     # thin wrapper over skill-creator
    └── results/                    # gitignored - per-iteration benchmarks
```

The live skill-creator workspace (`sol-skill-workspace/`, sibling to the
skill folder) is also gitignored - it holds transcripts, raw outputs,
and per-run benchmark files that don't belong in version control.

## Skill design

These are load-bearing for the skill's quality. Apply them when
adding or revising any section.

### Situational guidance

This skill is not an "SSH skill", not a "Slurm skill", not a "Python
skill". It is a **situational guide**: the user is trying to get
something done on Sol, and the skill teaches *which Sol-specific path
is right for the situation*. The underlying techniques (SSH port
forwarding, sbatch headers, environment modules) aren't the
contribution - the situational mapping is.

Every section in `SKILL.md` should open with the situation it
addresses, not the technique it employs. Compare:

- ✗ Technique-first: *"Sol uses SSH port forwarding to expose
  compute-node services. Run `ssh -L ... -J ...` to forward a port..."*
- ✓ Situation-first: *"The user wants a Jupyter notebook running on
  a Sol GPU and wants to open it in their laptop browser. Three
  paths exist: Open OnDemand for casual use, `solx` if installed,
  manual SSH chain otherwise."*

If a section reads like a manual page for a generic technique,
rewrite it. The agent already knows generic techniques from training
data; what it doesn't know is which one Sol's setup makes
appropriate, and why.

### Content placement

**Load-bearing decision rules belong in `SKILL.md` itself.** Anything
the agent needs to make a *correct decision* - partition choice,
refusal patterns, branching logic, default substitutions - should be
visible without requiring a separate Read of a reference file.
Reserve `references/` for the detail that backs those decisions:
worked examples, full command tables, syntax minutiae, troubleshooting.

This isn't a stylistic preference - it's load-bearing for robustness.
A skill that buries critical guidance in `references/` is fragile to:

- `claude -p --print` mode (reference Reads need explicit permission
  and may be denied silently)
- Symlinked dev trees that fall outside Claude Code's per-session
  directory-access guardrails
- Subagent invocations with restricted tool sets
- Any other situation where the agent can't (or chooses not to) take
  a Read tool turn

Iteration 3 of this skill caught exactly this failure mode: a "use
`htc` for lightweight debug" rule lived only in
`references/sessions.md`, was invisible to `claude -p`, and the
agent defaulted to `general`. Promoting the same rule into
`SKILL.md` (iter-4) fixed it immediately and the rule even
generalized to adjacent prompts.

When in doubt, ask: "if the agent never reads this reference, would
its answer still be correct on this topic?" If no, the rule
belongs up in `SKILL.md`.

## Evaluation layers

`sol-skill` is mostly **decision** and **refusal** logic that only
matters on Sol: "use `$(whoami)`, not `<asurite>`", "don't `find
/scratch -exec touch`", "branch on `command -v solx`", "load the
`scratch.md` reference before touching scratch". skill-creator's
default loop assumes test prompts produce *files* that you grade - but
we shouldn't actually call `srun` or open ssh tunnels from a laptop
during eval, and we don't have admin on Sol either way.

So evals are sliced into four layers, each runnable in a different
environment, each graded differently.

| Layer | What it checks | Where it runs | How it's graded |
|---|---|---|---|
| **L0 - Triggering** | Does the skill's frontmatter description make Claude invoke the skill on Sol-related prompts and *not* on near-misses (generic SLURM, generic Python venv)? | Anywhere with `claude -p` | `skill-creator/scripts/run_loop.py` |
| **L1 - Static / transcript-only** | Agent's *proposed* commands and reference-file reads. No execution. Catches: wrong placeholder, wrong storage location, missing reference load, suggesting `sudo`, suggesting a bulk-touch, snooping `~/.ssh/config`, forgetting the `command -v solx` branch. | Laptop, Sol login, anywhere | Subagent runs the prompt in a "describe what you'd do" mode; grader greps the transcript for required/forbidden patterns. |
| **L2 - Mocked Sol** | `solx` run against a fake Sol environment, plus its own unit suite. Catches: parsing the warning CSVs, keep-list matching (incl. carve-outs), side-detection logic, the destructive-confirm contract. | Laptop or Sol login (no privileges needed - pure userland mocks) | Run -> assert on exit code + stdout/stderr + filesystem mutations. The renewal mechanism is covered by the crate's keep tests (`solx/src/keep.rs` vectors + the end-to-end `solx/tests/cli.rs` real-touch test over a real tree with stale mtimes); the static `mocks/` CSVs (absolute `/scratch` paths) back L1 parsing checks. |
| **L3 - Real Sol smoke** | Things only meaningful on actual Sol: real `module avail`, real `srun`, real ssh tunnel through compute node, the `vscode` wrapper, `solx`'s startup latency vs raw SLURM, and whether a recommended partition/QOS/gres/time combo is actually schedulable. | Sol, manually, by maintainer | Short checklist the maintainer runs before release, `evals/runner/bench_solx_latency.sh` (read-only timing of `solx job` vs raw `squeue`), and `l3_sbatch_test_only` assertions that run an agent's recommended `#SBATCH` header through `sbatch --test-only`. |

The classification lives **in the eval file** - each assertion is
tagged `layer: L1 | L2 | L3` so the runner picks the right execution
mode and the public coverage doc can show pass-rate per layer
separately, not just an overall number.

## Mock environment

The thing that makes L2 work. Plain shell + tiny Python - no
framework. The mocks are small enough to read in a sitting; if you
need to extend them, treat the existing files as the contract.

```
evals/mocks/
├── activate.sh                    # source this; prepends bin/ to PATH, sets fake $HOME
├── bin/                           # PATH shims (executable)
│   ├── hostname                   # fake `sc001.sol.rc.asu.edu`, configurable
│   ├── module                     # canned module avail/load/list output
│   ├── srun, sbatch, scancel, squeue   # log args, return canned exit
│   └── ssh                        # log args, never connect
├── home/                          # fake $HOME during eval
│   ├── .config/solx/config.toml   # example config with a [keep] block
│   └── scratch-dirs-*.csv         # synthetic Sol warning files
└── scratch/swan16/                # fake scratch tree under fake $HOME
```

Every mock invocation is appended to `$MOCK_LOG`
(default: `/tmp/sol-skill-mock-$$.log`). Assertions can grep this log
to verify "agent called `srun --partition=lightwork`" without needing
a real scheduler.

To toggle whether the mock pretends to be Sol or a laptop, set
`MOCK_HOSTNAME` before sourcing `activate.sh`. The default is the
Sol-side value (`sc001.sol.rc.asu.edu`). The `solx` binary is
intentionally **absent** from `bin/` - that's how we exercise the
"command -v solx returns nothing" branch. Drop a `solx` shim into
`bin/` only when testing the `solx`-present branch.

### Mock setup

```shell
cd /path/to/sol-skill
source evals/mocks/activate.sh
hostname -a                                  # -> sc001.sol.rc.asu.edu
solx keep --dry-run -v                       # exercises CSV + keep-list parsing (needs solx)
cat "$MOCK_LOG"                              # see what was invoked
```

## Local evaluation

Prereqs: `uv`, `claude` CLI, the `skill-creator` skill installed (the
harness shells out to its `scripts/` and `eval-viewer/`).

### Baseline isolation

Skill-creator compares **with-skill** runs against **baseline** runs.
If `sol-skill` is installed at user scope (`~/.claude/skills/sol-skill/`),
every subagent - baseline included - sees it, and the comparison is
meaningless.

The fix is to relocate Claude Code's config dir for the eval session
*only*. Claude Code reads its config from `$CLAUDE_CONFIG_DIR` if set
(verified in the v2.1.117 binary), falling back to `~/.claude/`. The
`evals/runner/build_sandbox_home.sh` script builds a mirror config dir
that symlinks everything from your real `~/.claude/` *except* the
`sol-skill` skill - so auth, plugins, every other skill, and your
settings all carry over, but `sol-skill` is invisible to baselines.

```shell
SANDBOX=$(./evals/runner/build_sandbox_home.sh)
CLAUDE_CONFIG_DIR=$SANDBOX claude     # start the eval-orchestrator session here
```

Other terminals running `claude` continue to see your real config and
the user-scope `sol-skill` install - parallel work is unaffected.

Inside the sandboxed session:

- **with-skill subagent** is given the dev tree explicitly via
  `--plugin-dir skills/sol-skill` (or skill-creator's `--skill` arg in
  newer versions).
- **baseline subagent** gets no skill arg; the sandbox config has no
  `sol-skill`, so the comparison is fair.

To hide a different skill (e.g., when iterating on a sibling skill):

```shell
./evals/runner/build_sandbox_home.sh --hide-skill other-skill
```

To verify the sandbox is taking effect, start a `claude -p` against
it and ask "list available skills" - `sol-skill` should be missing.

### Eval suite

```shell
# 1. Build the sandbox (first time, or whenever ~/.claude changes)
SANDBOX=$(./evals/runner/build_sandbox_home.sh)

# 2. Lay out a fresh iteration
WORKSPACE=sol-skill-workspace/iteration-1
mkdir -p "$WORKSPACE"

# 3. Run all evals from inside the sandboxed session.
#    Spawns one with-skill + one baseline subagent per eval. Use
#    --permission-mode acceptEdits when the runner shells out to
#    `claude -p`, otherwise reference Reads will be denied silently
#    and the skill will be measured at less than its real capability.
CLAUDE_CONFIG_DIR=$SANDBOX \
  python -m evals.runner --evals evals/evals.json --workspace "$WORKSPACE"

# 4. Aggregate (uses skill-creator's aggregator)
python -m scripts.aggregate_benchmark "$WORKSPACE" --skill-name sol-skill

# 5. View
python <skill-creator-path>/eval-viewer/generate_review.py \
  "$WORKSPACE" \
  --skill-name sol-skill \
  --benchmark "$WORKSPACE/benchmark.json"
```

## New evals

1. Open `evals/evals.json` (or `evals/evals.example.json` if you don't
   have a private set yet).
2. Append an entry with these fields:
   - `id`, `prompt`, `expected_output` - standard skill-creator schema
   - `assertions[]` - each assertion is `{text, layer, check}` where
     `layer` is `"L1"|"L2"|"L3"` and `check` is one of:
     - `"transcript_contains": "..."` / `"transcript_lacks": "..."`
     - `"file_exists": "..."` / `"file_contains": {...}`
     - `"exit_code": 0` (L2 only - the runner captures the script's
       exit code)
     - `"mock_log_contains": "..."` (L2 only - greps `$MOCK_LOG`)
     - `"manual"` (L3 only - surfaces in the manual checklist)
3. If the eval needs a specific mock state (e.g., `solx` present, or a
   different `[keep]` config), add a `setup` block that the runner sources
   before spawning the subagent.

Keep prompts concrete and realistic - see the skill-creator
description-optimization guide for what makes a good prompt.

## Releases

The CLI and the skill share one version line; a pushed `vX.Y.Z` tag
triggers `.github/workflows/release.yml` (build the static musl binary,
publish the GitHub Release with it attached). Before tagging:

1. Bump the version in `solx/Cargo.toml` and `skills/sol-skill/SKILL.md`
   (`version:`); refresh `solx/Cargo.lock` (`cargo update -p solx`). The
   release workflow refuses to publish if the tag, `Cargo.toml`, and
   `SKILL.md` disagree.
2. Run the full eval suite locally (L1 + L2) and `solx`'s test suite
   (`cd solx && cargo test`).
3. Walk the L3 manual checklist on real Sol (login + compute node).
4. Hand-edit `docs/coverage.md`: bump the "Last verified" date, flip
   any cells in the matrix, refresh "Known gaps", and bump its
   `**Version:**` line. Move the `[Unreleased]` notes under a
   `## [X.Y.Z]` heading in `CHANGELOG.md`, **and update the
   link-reference footer at the bottom** (repoint `[Unreleased]` to
   `compare/vX.Y.Z...HEAD` and add a `[X.Y.Z]` release-tag target) - a
   missed footer leaves the new heading as a dead link. The README's
   version is shown by the dynamic Release badge, so it needs no edit.
5. If the release added a user-visible capability, touch the "What this
   skill helps with" bullets in `skills/sol-skill/SKILL.md`.
6. Commit the docs on the release commit, then tag `vX.Y.Z` and push -
   CI builds and publishes the release.

**CLI-only releases skip the skill eval re-run.** When a release changes
only the `solx` crate and leaves the skill's guidance content unchanged
(everything under `skills/sol-skill/` identical apart from the shared
`version:` line), the L1/L2/L3 *skill* evals in steps 2-3 don't need
re-running - they still hold, because the skill itself is unchanged. The
gate for such a release is the crate's own `cargo test` suite plus an L3
*CLI* smoke on real Sol (the shipped binary still needs exercising). A
release that touches skill prose, references, or decision rules must run
the skill evals. (1.0.2 - the nested `job jump` fix - was CLI-only.)

## Repository contents

| Thing | Location | In git? | Why |
|---|---|---|---|
| Skill contents (SKILL.md, references) | `skills/sol-skill/` | yes | shipped to users |
| solx CLI crate | `solx/` | yes | the Rust CLI; built to a static binary by CI on tag |
| CI workflows | `.github/workflows/` | yes | lint + test, and tag-driven release |
| Mocks + runner code | `evals/mocks/`, `evals/runner/` | yes | no PII, useful for contributors |
| Sanitized eval template | `evals/evals.example.json` | yes | shows the schema |
| Real eval prompts + assertions | `evals/evals.json` | **no** | may reference real ASURITEs, project paths, partitions |
| Per-iteration `benchmark.{json,md}` | `evals/results/` | **no** | may include real paths in transcripts; summarize in `docs/coverage.md` instead |
| Live workspace (transcripts, raw outputs) | `sol-skill-workspace/` | **no** | regenerable, large, non-deterministic |
| L3 manual checklist results | maintainer's notes | **no** | personal Sol session details |

The public verification surface is `docs/coverage.md` - methodology
plus a coverage matrix at the *category* level. Everything more
specific than that stays out of git on purpose.

## Dependencies

- [`uv`](https://docs.astral.sh/uv/) - script runner and Python env
  manager for the eval harness (the runner and its helpers). `solx`
  itself no longer needs it - the shipped CLI is a static binary.
- [Rust](https://rustup.rs/) (stable) - to build and test the `solx`
  crate; `cargo test` runs the unit + end-to-end suites.
- [`claude` CLI](https://docs.claude.com/en/docs/claude-code) - the
  runner shells out to spawn subagents.
- The
  [`skill-creator`](https://github.com/anthropics/claude-code-plugins/tree/main/plugins/skill-creator)
  skill - provides `aggregate_benchmark.py`, `eval-viewer/`,
  `run_loop.py`. The runner doesn't reimplement them; it composes.
