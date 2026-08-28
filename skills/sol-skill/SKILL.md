---
name: sol-skill
version: 1.0.3
description: Conventions and tooling for ASU's Sol supercomputer, built around the `solx` CLI. Use whenever a task is happening on Sol - the user mentions Sol or ASU Research Computing, or is clearly on their Sol account (a Sol /scratch path, an sbatch/interactive job, a login/compute node). It covers renewing /scratch files Sol has flagged for deletion (purge/inactivity warnings) via `solx keep` and where to store datasets and model caches; requesting and managing SLURM jobs (the `solx job` interactive-allocation lifecycle, sbatch for batch, GPU and partition/QOS choice, why a job is pending, fairshare-aware and time-aware job management); installing software without sudo (module load, uv for Python, tinytex for LaTeX); reaching a Sol compute-node service like Jupyter from a laptop browser; detecting login-vs-compute nodes and choosing where to run heavy I/O (the DTN, a compute node, or a batch job); and transferring data to and from Sol. Not for generic SLURM/HPC on other clusters (Phoenix, NERSC, ...), cloud GPUs, or purely local-laptop tasks (local virtualenvs, local LaTeX, local file/timestamp cleanup).
license: MIT
---

# 🌵 Sol skills

Official doc: <https://docs.rc.asu.edu/>.

That official doc is authoritative; these notes are just a cache.

## What this skill helps with

This skill teaches an AI coding assistant how to operate on ASU's Sol
supercomputer the way a careful human user would. The everyday job and
scratch operations are driven through **`solx`**, a small CLI this skill
installs and prefers; the rest is situational guidance for Sol's native
tools. Concretely, it covers:

- **Driving `solx`** - the CLI for daily Sol work: request and
  manage interactive allocations (`solx job start/list/jump/time/stop`)
  and renew flagged `/scratch` files (`solx keep`). The skill installs
  it on first use and prefers it.
- **Detecting the environment** - recognizing whether the agent is
  running on a Sol login/compute node or on a laptop, and branching
  behavior accordingly.
- **Storage decisions** - putting datasets, model weights, and caches
  under `/scratch/$USER` and keeping `/home` for config; never asking
  for `sudo`.
- **Scratch retention** - refreshing files Sol has flagged for deletion
  with `solx keep`, driven by Sol's own warning CSVs and a `[keep]`
  block in the `solx` config; refusing to bulk-`touch` `/scratch`.
- **Getting software onto Sol without sudo** - `module load` for what
  the cluster already provides, `uv` for Python interpreters and envs,
  R's `tinytex` for LaTeX, `~/.local`/`~/opt` for everything else.
- **SLURM jobs** - interactive allocations through `solx job`; batch
  jobs through `sbatch` with correct partition/QOS/time headers and
  Sol's prebuilt SBATCH templates.
- **Situation-aware job management** - checking fairshare before
  submitting (and backing off when it's low), and tracking the wall-time
  left on the current allocation so work is wrapped up or handed off
  before Slurm reclaims the node.
- **Status queries about you and your jobs** - translating "what's
  queued?", "what's my fairshare?", "what accounts can I use?",
  "why is my job pending?" into the right Sol wrapper or SLURM call.
- **Reaching a Sol-side service from a laptop** - Open OnDemand vs. the
  manual `ssh -L ... -J ...` chain for Jupyter / dev servers / OAuth
  callbacks. (`solx` is Sol-only, so this stays manual - compute nodes
  aren't internet-reachable, so the SSH chain has to ProxyJump (`-J`)
  through the login node.)
- **File sharing** - the `chmod`/`install`/`cp` dance to share files
  between Sol users via `/scratch`.
- **Data transfer** - `rsync`/`scp` between laptop and Sol.
- **VS Code** - auto-activating shell environments via
  `terminal.integrated.env.linux`.

Cross-cutting conventions enforced throughout:

- Always substitute `$(whoami)` for the username before showing
  examples - never emit `<asurite>` or other angle-bracket
  placeholders.
- Never read `~/.ssh/config` or `~/.ssh/known_hosts`. If the user has
  a custom Host alias, they will tell you.
- Always preview destructive or long-running operations with
  `--dry-run` (or the equivalent) before executing.
- `solx` is CLI-agent native in operational terms: use `--json` when
  parsing, respect stdout/stderr separation, use `--dry-run`, avoid
  hidden prompts; raw Slurm commands remain documented equivalents.

For how this skill is verified, see
[`docs/coverage.md`](../../docs/coverage.md) in the source repo.

## `solx` - install it first

**`solx`** is a small CLI you run *on Sol*; it reads one config file and
shells out to Slurm, and never touches your laptop or `~/.ssh/*`. It's
the user's day-to-day tool for templated interactive allocations
(`job start` / `job jump`) and scratch renewal (`keep`) - install it when
the user is doing that kind of work.

**`solx` is fast enough to be the default.** It's a single native binary
(Rust), so a warm `solx job` read costs ~0.12s on Sol vs ~0.08s for a raw
`squeue` (measured - `evals/runner/bench_solx_latency.sh`), and startup is
flat regardless of node load or NFS cache state. One-off reads carry no
meaningful `solx` penalty. Raw SLURM stays a full equivalent (see "`solx`
vs raw SLURM" below) - it's the fallback when `solx` isn't installed, not a
faster path to prefer.

**Detect, then install when the task needs it.** Once you've confirmed
you're on Sol (see [Detecting the Environment](#detecting-the-environment)):

```shell
command -v solx        # missing? install it when the user needs job start/jump or keep
```

If it's absent and the task calls for it, **prompt the user to install
it** (then run `solx init`). `solx` is one static binary - no Python, no
`uv`, no toolchain - so installing is a download and a `chmod`:

```shell
mkdir -p ~/.local/bin
curl -fLo ~/.local/bin/solx https://github.com/Shu-Wan/solx/releases/latest/download/solx-x86_64-unknown-linux-musl
chmod +x ~/.local/bin/solx

solx --version
solx init              # writes ~/.config/solx/config.toml (offers a quick walkthrough)
```

The binary is fully static (musl), so it runs on Sol's RHEL 8 as-is.
Installing reaches the network and writes to `~/.local/bin` - propose the
command and get the user's go-ahead (or run it with their OK) rather than
installing silently. Make sure `~/.local/bin` is on `$PATH`.

**If the user declines or can't install `solx`,** nothing is lost for the
common cases - raw Slurm covers them: `squeue`/`scancel` for status and
cancel, `salloc`/`interactive` for allocations, `sbatch` for batch, and
the emergency single-path `touch` for scratch. `solx` changes the
ergonomics of `job start`/`jump` and `keep`, not what's possible.

The full command surface, the config schema, and the agent-output
contract are in [references/solx.md](references/solx.md). The short
version:

| Command | What it does |
|---|---|
| `solx init` / `solx config edit` | Write / edit `~/.config/solx/config.toml`. |
| `solx job start [TEMPLATE]` | Request an interactive allocation from a config template. |
| `solx job jump` | Drop a shell onto the job's compute node (`srun --pty`). |
| `solx job list` · `time` · `stop` | List · time-left · cancel. Raw `squeue`/`scancel` are equivalent (see below). |
| `solx keep` | Renew `/scratch` files Sol flagged, filtered by `[keep]`. |
| `solx cheatsheet` | Print the Sol quick reference (`references/cheatsheet.md`) as text. |

`--json` forces JSON - before the subcommand (`solx --json job list`) or
after it (`solx job list --json`; exception: after `job start`, tokens
pass through to `salloc`). Data goes to stdout, diagnostics to stderr,
so output pipes cleanly. Destructive commands (`job stop`, `keep`)
prompt unless `-y`, refuse in a non-interactive session rather than
hang, and preview with `-n`.

**`solx` vs raw SLURM - equivalent for one-off reads; use either.** A
warm `solx job` read runs in ~0.12s, vs ~0.08s for raw `squeue` (measured -
see `evals/runner/bench_solx_latency.sh`); the residual over `squeue` is
just the `squeue` subprocess `solx` spawns, and the native binary's
startup doesn't degrade under node load or a cold NFS cache. The raw
equivalents, for when `solx` isn't installed or the user asks for them:

```shell
squeue --me                                # = solx job list
squeue -h -j "$SLURM_JOB_ID" -o %L         # = solx job time (inside an allocation)
scancel <jobid>                            # = solx job stop -y <jobid>
```

`solx` adds the most over raw SLURM on the multi-step operations:
`solx job start` (allocate from a template and wait), `solx job jump`
(drop a shell onto the compute node), and `solx keep` (the
CSV-∩-keep-list renewal). When parsing any of its output, pass `--json`.

## Detecting the Environment

Three SLURM-side signals, cheapest first. Ignore `hostname` for
this - aliases and custom `~/.ssh/config` entries make it noisier
than necessary, and SLURM gives you cleaner answers.

1. **No SLURM client -> not on Sol.** If `command -v sacctmgr`
   returns nothing, the machine doesn't have a SLURM client at all,
   so it can't be a Sol login or compute node. This is the cheapest
   and most decisive negative check - stop here on a laptop. (`solx`
   itself enforces this: every subcommand exits 2 off-Sol with a
   redirect message, so a stray `solx job list` on a laptop is safe.)
2. **Which SLURM cluster.** `sacctmgr -n show cluster format=cluster`
   prints `sol` on any Sol login or compute node. A different value
   means a different SLURM cluster (Phoenix, etc.); empty output
   means SLURM is installed but the client isn't talking to a
   controller. SLURM's own view of cluster identity is canonical.
3. **Login vs compute node within Sol.** `$SLURM_JOB_ID` is set
   inside any allocation (you're on a compute node - don't `srun` /
   `sbatch` from here, don't run heavy work outside what the
   allocation reserved); unset means you're on a login node (free to
   submit jobs, but don't run heavy compute on the login node
   itself). `solx job time`/`jump`/`stop` read `$SLURM_JOB_ID` too:
   inside an allocation they default to *that* job.

## General Rules

1. Save datasets and caches under `/scratch`.
2. You do not have `sudo` privileges, so maintain a local environment under `/home/$USER/.local` or `/home/$USER/opt`.
3. Use `git` to keep code in sync between local and cluster.

## Submitting Jobs

Sol uses **Slurm**. Interactive allocations go through `solx`; batch
work goes through `sbatch`. `solx` deliberately doesn't wrap `sbatch` -
for batch, drive Sol's tooling directly.

### Know your access first

Partition and QOS advice is only correct *for this user* - what they can
run on depends on their account and group. Before recommending where a
job goes, check what's actually available to them:

```shell
sacctmgr -n show assoc user=$USER format=Account,Partition,QOS
#   -> e.g.  grp_yourpi || debug,htc,private,public
myfairshare                            # dampened RealFairShare; low -> prefer a buy-in/preemptible QOS (raw `sshare -U` is undampened)
```

The QOS column is the menu. Most users have `public` (default,
non-preemptable) and `debug` (15-min, high-priority); a `private` or
`grp_*` entry means the user's group owns nodes they can run on *longer
than htc's 4 hours* (and preemptibly, for `private`). Tailor the
partition/QOS choice below to that list - don't suggest a QOS the user
can't use. The full partition × QOS table is in the cheat sheet,
[references/cheatsheet.md](references/cheatsheet.md).

### Interactive allocations - `solx job`

`solx job start` requests an allocation from a named template in your
config and **waits until the queue grants it**, then returns; the
allocation keeps running in the background until you attach.

```shell
solx job start debug         # request the [jobs.debug] template; prints the job id
solx job start debug -n      # dry run: print the salloc argv, submit nothing
solx job jump                # open a shell on the compute node (srun --pty)
# ... work ...
exit                         # back to the login node; the allocation stays alive
solx job list                # still RUNNING?            (= squeue --me)
solx job time                # wall-time left             (= squeue -h -j <id> -o %L)
solx job stop                # cancel when done; prompts  (= scancel <id>)
```

Templates live in `~/.config/solx/config.toml` (`solx config edit`);
each `[jobs.<name>]` sets `partition`, `time`, optional `qos`, `gres`,
`extra_args`. Anything after `--` on `solx job start` is appended to
`salloc` (last flag wins), so you can override a template for one run:
`solx job start gpu -- --mem=128G`. Without `solx`, the equivalent is
`interactive` / `salloc` directly (next paragraph).

**The `interactive` wrapper** (the no-`solx` fallback) already defaults
to `-p htc -q public -c 1 -t 0-4`. Bare `interactive` gets you a 4-hour
`htc` shell - the right shape for most debug or "just need to check
something on a compute node" sessions, GPU work included: `htc` carries
A100s, so `interactive -p htc -G a100:1` gets you a quick GPU shell.
Override to `public` only when the run needs more than htc's 4-hour wall.

**Match the partition to the job's wall-time and priority, not to
whether it uses a GPU.** GPUs live in `htc`, `public`, *and* `general`,
so "it needs an A100" says nothing about where the job goes - *how
long* and *how urgently* do:

- **≤ 4 h, including GPU work -> `htc`.** The default home for debug,
  ablations, smoke-tests, and short training. `htc` carries Sol's
  largest A100 pool (dozens of `a100:4` nodes, plus H100 / L40 / A30)
  and is far less contended than `public`. Sol nudges you toward it - a
  ≤4h job submitted to `public` prints `you may consider '-p htc'`, but
  it doesn't move the job for you, so pass `-p htc` yourself. A 30-minute
  A100 ablation is an `htc` job, not a `public` one.
- **≤ 15 min and you want to jump the queue -> `-p htc -q debug`.**
  The `debug` QOS has a 15-minute hard cap but very high priority and allows
  GPUs - ideal for "does this even launch?". It also works with `public` and
  `general`, but `htc` is the default route for a short test. One job may run
  at a time, with at most two submitted.
- **> 4 h, non-preemptable -> `-p public`** (7-day wall) - real runs
  that can't finish or checkpoint inside 4 hours.
- **> 4 h on borrowed private nodes, OK with preemption -> `-p general
  -q private`.** The `private` QOS has no wall of its own (gated by the
  partition) and trades preemptibility - owners can cancel your job -
  for running past htc's 4 hours; it often starts sooner.

The trap is the **"GPU -> public" reflex**: sending a short GPU job to
`public` parks it behind multi-day jobs while hundreds of htc A100s sit
one partition over. Only the wall-time clock - or a node shape `htc`
lacks (GH200, Gaudi) - should push GPU work past `htc`.

### Batch jobs - `sbatch`

For real batch work, submit an SBATCH script with `sbatch` - `solx`
doesn't try to replace it. **Don't write SBATCH scripts from scratch
when a template fits.** Sol ships ready-to-modify templates under
`/packages/public/sol-sbatch-templates/templates/` for serial, MPI
(`hpcx` / `intel` / `mpich` / `mvapich` / `openmpi` variants), Python,
Python multiprocessing, R, MATLAB, and rclone jobs. Start from the
closest match instead of inventing headers.

See [references/slurm.md](references/slurm.md) for submission commands,
example scripts (serial, MPI, job arrays), troubleshooting, exit codes,
and the helpful-commands table; and
[references/sessions.md](references/sessions.md) for the manual
`interactive` / ssh-tunnel path when `solx` isn't in play.

## Situation-Aware Job Management

Submitting jobs is cheap to *type* and expensive to *get wrong* on a
shared cluster. Check the cluster's state before and after you submit -
fairshare and remaining wall-time should shape what you do, and a
PENDING job is a question to *answer*, not a state to sit in. Don't fly
blind.

### When a job is PENDING - diagnose cause + ETA, don't park it

PENDING is a **question, not a resting state.** The moment a job pends,
find out *why* and *when it will start* before deciding anything - and
never report "it's pending, want me to wait or switch partitions?"
without that diagnosis. Get cause + ETA up front:

```shell
squeue --me -t PD -O "JobID,Reason:50,StartTime"   # full reason + estimated start, parseable
scontrol show job <jobid>                          # all fields for one job (Reason=..., StartTime=...)
```

(Widen `Reason` - a real reason can be multi-word, e.g.
`ReqNodeNotAvail, UnavailableNodes:sc013`; a `grep 'Reason=[^ ]+'`
truncates it at the first space.)

**The `Reason` dictates the move** - and most of the time the move is
*report, don't reroute*:

| `Reason` | What it means | Do this |
|---|---|---|
| `Priority` (with low `myfairshare`) | **Priority-bound** - you're behind others under a per-user cap. | No reroute beats a priority cap. Report the `StartTime` ETA; **don't cancel-and-resubmit** - the new job inherits the same priority and burns more fairshare. |
| `ReqNodeNotAvail, UnavailableNodes:<n>` | **A required node is unavailable** - drained/down (may need an admin, no predictable clear time) or held by a reservation. SLURM uses a separate `Reservation` reason for waiting on an advanced reservation. | Check the node's state. A reservation clears at its `StartTime`; a drained/down node may not - reroute to a partition whose nodes are healthy. |
| `Resources` | **Capacity-bound** - the QOS/partition is simply full right now (usually carries a backfill `StartTime` estimate). | *Now* a reroute can help: check `sinfo` for a partition with free nodes for a QOS you hold (your `sacctmgr` menu), and/or right-size. |

Then, before blaming the queue:

1. **Right-size the request.** An over-specific GRES is often the whole
   reason a job waits - don't pin `--gres=gpu:a100:1` when any GPU runs
   the job (`--gres=gpu:1`), and don't grab `public`/large nodes for
   work that fits on `htc`.
2. **Reroute in place, and only if it wins.** Modify the pending job
   with `scontrol update job <id> Partition=... QOS=...` rather than
   `scancel` + `sbatch` - resubmitting forfeits the priority the job has
   already accrued in the queue. Compare the candidate's `StartTime`
   against the current one before committing (`sbatch --test-only` /
   `scontrol show job` estimate the move without losing your slot).

**The punchline:** when the wait is fairshare- or reservation-bound,
spraying the job across partitions is wasted motion - every partition
converges on the same start time because the binding constraint is
*you* (priority) or *the reservation*, not the partition. The correct
proactive move is to **diagnose, report cause + ETA + whether routing
can help, then stop submitting.** Long queue waits are usually fairshare
(see **Fairshare**, next), not a stuck cluster.

### Fairshare - check it before you submit, and back off when it's low

Fairshare is Sol's scheduling-priority score (roughly 0–1): heavy recent
usage drives it down, which makes your future jobs queue *behind*
everyone else's. Read it with the wrapper, which does the live priority
math:

```shell
myfairshare
```

`myfairshare` prints a table; the number to read is the rightmost
**`RealFairShare`** column (the dampened 0–1 score). **Below ~0.05 is
bad** - the account is effectively throttled. When fairshare is that
low:

- **Don't spam the scheduler.** Submitting a pile of jobs, or
  auto-resubmitting on every failure, burns more fairshare and digs the
  hole deeper - the opposite of what's needed. Submit fewer, right-sized
  jobs and let them run.
- **Right-size requests.** Don't grab `public`/GPU/large nodes for work
  that fits on `htc`; over-asking costs more fairshare per job.
- **Tell the user.** Surface the low fairshare ("your fairshare is
  0.03, so jobs will queue for a while") instead of quietly firing more
  work. Long queue waits are usually fairshare, not a stuck cluster.

Don't reflexively cancel-and-resubmit a pending job to "get a better
spot" - the new job inherits the same (or worse) priority and you've
spent fairshare for nothing.

### Remaining time - wrap up or hand off before the node is reclaimed

When the agent is working *inside* an allocation, it's on borrowed
wall-time: when the clock runs out, Slurm kills the job and **anything
not written to durable storage is lost.** Know how much time is left:

```shell
solx job time                          # remaining wall-time (e.g. 2:14:09; D-HH:MM:SS once over a day)
squeue -h -j "$SLURM_JOB_ID" -o %L     # the no-solx equivalent (TimeLeft, no padding)
```

Then manage the window deliberately:

- **Don't start what can't finish.** Before kicking off a step, compare
  its expected runtime to the time left. If it won't fit, either
  checkpoint-and-resume in chunks, or request more time up front (a new
  `solx job start` with a longer `time`, or a batch job) rather than
  losing the work at the boundary.
- **Wrap up early.** As the remaining time gets short, stop starting new
  work and instead **flush results/outputs to `/scratch`, checkpoint
  model state, and write a one-paragraph summary** of where things
  stand. `/home` and `/scratch` survive the allocation; the compute
  node's local state does not.
- **Hand off.** If the task will outlive the allocation, leave a resume
  note or a small script (what ran, what's left, the command to
  continue) so the next session - or a follow-up batch job - picks up
  cleanly instead of redoing work.

### Use Sol's own wrappers directly

For status and introspection, Sol ships `my*` / `show*` wrappers (and
their SLURM-native equivalents). Call them **directly** - `solx` doesn't
wrap them (it owns the *interactive-allocation lifecycle*; these own
*status*), and don't reimplement them. Which form to prefer depends on
audience - see the caveat under the table:

| You want | Command |
|---|---|
| Your fairshare / scheduling priority | `myfairshare` |
| Your `/scratch` quota | `beegfs-ctl --getquota --uid $USER` |
| Your jobs right now | `myjobs` (or `squeue --me`) |
| Estimated start time of a pending job | `thisjob <jobid>` |
| Efficiency of a finished job | `seff <jobid>` |
| Free capacity / partitions | `sinfo`, `showparts` |
| Free GPUs per node | `showgpus` (color-coded; to *parse* free counts use `sinfo -h -O "Partition,StateLong,Gres,GresUsed"` - free = `Gres` − `GresUsed`) |

These wrappers are built for **human eyes** (color, tty layout), so
they're the right call when you're *showing a user* or when the wrapper
does a calculation you'd otherwise reimplement (`myfairshare`). **When
you're going to parse the output yourself, prefer the SLURM-native or
`--json` form** - see the audience-tagged table in [Asking the Cluster
About Yourself and Your
Jobs](#asking-the-cluster-about-yourself-and-your-jobs). Full command
list and output notes: [references/slurm.md](references/slurm.md).

## Asking the Cluster About Yourself and Your Jobs

Status questions ("what's queued?", "why is my job pending?", "what
accounts can I use?") almost always have a one-line answer. But the two
forms are **not interchangeable for an agent**: the SLURM-native and
`--json`/`-O`/`-o` forms are built to be parsed, while Sol's `my*` /
`show*` wrappers are built for **human eyes** - ANSI color, tty layout,
pagination - and fight `awk`/`grep`.

**Rule of thumb: for an agent, default to the agent-parseable form.
Reach for a `show*` / `my*` wrapper only to show a human, or when it
encapsulates a calculation you'd otherwise reimplement (`myfairshare`'s
dampened score).** Concretely, `showgpus` is color-coded and its column
layout fights parsers - `sinfo -h -o "%P %t %G %N"` parses cleanly and
shows which GPU *types* each partition carries and each node's state;
for the actual free count, subtract `GresUsed` from `Gres` (below).

| User question | Agent-parseable form (parse this) | Human wrapper (to show a user) |
|---|---|---|
| What jobs do I have right now? | `squeue --me -O JobID,Partition,State,Reason` (or `solx --json job list`) | `myjobs` (priority/QOS/GPU columns), `summary` (state counts). NB `sq` is the *whole-cluster* queue - filter with `sq -u $USER` |
| Tell me about job N | `scontrol show job N` | `thisjob N` adds a `squeue` row + est. start; `showjob N` also runs `seff` if finished |
| What's my historical job activity? | `sacct --user=$USER --starttime=YYYY-mm-dd -X -P -o JobID,State,Elapsed,ReqMem` (`-P` = parseable) | `mysacct` (preset format) |
| What accounts and QOS can I submit under? | `sacctmgr -n show assoc user=$USER format=Account,Partition,QOS` | `myaccounts` (color table) |
| What's my fairshare / scheduling priority? | `myfairshare` -> read the `RealFairShare` column - the **exception**: the wrapper *is* the parseable source (it does the dampening math; raw `sshare -U` gives only undampened shares) | - (same wrapper) |
| What's my scratch quota? | `beegfs-ctl --getquota --uid $USER` | - |
| Why is my job stuck pending? | `squeue --me -t PD -O "JobID,Reason:50,StartTime"` (widen `Reason` so a multi-word reason isn't truncated); `scontrol show job N` for one job's full detail | `showlimited` (cluster-wide capacity holds by group/QOS) |
| Which partitions have free capacity? | `sinfo -h -o "%P %a %l %D %t"` | `showparts` (color-coded availability) |
| Which GPU nodes have free GPUs? | `sinfo -h -O "Partition,StateLong,Gres,GresUsed,NodeList"` (free = `Gres` − `GresUsed`); `%G` alone / `scontrol show nodes` show *configured* GPUs, not free | `showgpus` (color-coded per-node) |
| How efficient was a finished job? | `seff <jobid>` | (no wrapper) |

## Filesystem and Storage

Sol provides two main storage areas:

| Location         | Purpose                      | Policy                          |
|------------------|------------------------------|---------------------------------|
| `/home/$USER`    | Config, small files          | Limited space, backed up        |
| `/scratch/$USER` | Large data, caches, outputs  | Layered deletion - see Sol docs |

Always place large data files, model caches, and outputs under
`/scratch/$USER`.

### Renewing the Scratch Timestamp - `solx keep`

Sol deletes inactive `/scratch` files on a layered schedule and writes
per-stage CSV warnings into `$HOME`. ASU Research Computing defines the
thresholds, CSV filenames, and warning cadence; their doc is
authoritative: <https://docs.rc.asu.edu/scratch>.

**Use `solx keep`.** It reads those CSVs, keeps only the directories
that match your **keep-list**, and refreshes their timestamps with
`touch`. It only ever touches directories that are **both** flagged by
Sol **and** in your keep-list - so there's nothing to do until Sol
actually flags something, and it never walks `/scratch` wholesale. That
bound is the whole point: it's a tool to extend the life of files you
still use, not to defeat Sol's retention policy.

**Where the keep-list lives:** the `[keep]` block in
`~/.config/solx/config.toml` - `include` / `exclude` gitignore-style
globs, set up once with `solx config edit`. Keep regenerable junk
(`.venv`, `__pycache__`, `node_modules`, ...) out of `include` so a
renewal isn't spent on files that rebuild for free. `solx` owns the
mechanics - the config schema and a worked example are in
[references/solx.md](references/solx.md).

**Preview before the real pass.** `solx keep` rewrites timestamps on
every kept file - potentially hundreds of thousands. Never fire it
blind: run `--dry-run` first and check the plan, *or* get the user's
go-ahead on the scope. It also prompts (`... ? [y/N]`) before touching
unless you pass `-y`; in a non-interactive session it refuses rather
than hang.

```shell
solx keep --dry-run -v       # preview which directories would be renewed
solx keep                    # renew them (prompts; -y to skip the prompt)
solx keep --stage pending    # only the most-urgent CSV
solx --json keep --dry-run   # machine-readable plan (counts + a capped sample)
```

#### Where to run it

A renewal is metadata-heavy I/O, not compute - but a touch pass over
tens of thousands of files is exactly the load Sol's **login nodes
throttle**. Check the environment first (see [Detecting the
Environment](#detecting-the-environment)), then branch:

- **On a compute node** (`$SLURM_JOB_ID` set) - run it directly; you
  already hold dedicated resources.
- **On a login node** (`$SLURM_JOB_ID` unset) - don't run the heavy
  pass here. Move it to one of, in rough order of convenience:
  - the **DTN**: `ssh soldtn '<cmd>'` (the `dtn` wrapper is literally
    `ssh soldtn`). It's tuned for I/O, isn't throttled, and has many
    cores - the best home for a large renewal.
  - a **compute node**: grab one with `solx job start` (or
    `interactive`) and run it there.
  - a **batch job**: submit a short `htc` job whose payload is the
    renewal, for an unattended pass.

Match `-j` (parallel workers) to where it actually runs: a 4-core
compute node can't feed more than a couple, while the DTN has many. See
[references/scratch.md](references/scratch.md) for the non-interactive
`PATH` gotcha when invoking over `ssh soldtn`.

### Sharing Files

See [references/sharing.md](references/sharing.md) for the
step-by-step procedure to share files with other users on the
cluster.

## Getting the Software You Need on Sol

Situation: you need a tool - a compiler, a Python interpreter, an R
package, a LaTeX distribution, a CLI - and the system `PATH` on Sol
either doesn't have it or has too old a version. You don't have
`sudo`. There are four non-sudo paths; pick the one that matches the
kind of software:

1. **Already on the cluster as a module.** Compilers, MPI stacks,
   Python distributions, R, CUDA, common applications - all live
   under the `module` system. No modules are loaded when a session
   starts, so `module load` them every session (or in every SBATCH
   script). See [references/module.md](references/module.md) for
   `avail` / `load` / `list` / `purge` and the naming schemes.

2. **Python - use `uv`.** The system `python3` on Sol is older than
   modern code expects. Don't fight it; use
   [`uv`](https://docs.astral.sh/uv/) to manage interpreters and
   environments instead.

   - Point `uv`'s cache at `/scratch` so it doesn't fill `/home`:

     ```shell
     export UV_CACHE_DIR=/scratch/$USER/.cache/uv
     ```
   - For one-file utility scripts, prefer the PEP 723 inline-metadata
     shebang `#!/usr/bin/env -S uv run --script` so the script
     self-bootstraps its interpreter and dependencies.

3. **LaTeX - use R's `tinytex`.** Builds a per-user TeX Live tree
   under `~/.local/bin/latex`, no sudo:

   1. `module avail r-4` to find a current R, then `module load` it.
   2. Use the R package `tinytex` to install TeX Live locally.
   3. Install TeX packages on demand: `tlmgr install <pkg>`.
   4. If `tlmgr` complains "is older than remote repository", refresh
      the local TeX Live: load R, then
      `Rscript -e "tinytex::reinstall_tinytex(repository='illinois')"`.

4. **Anything else - install to `~/.local` or `~/opt`.** No `sudo`
   on Sol, so anything you build or download from source goes under
   your home directory. `~/.local/bin` should be on `PATH` by
   default; add it in `~/.bashrc` / `~/.zshrc` if not.

Across all four: never propose `sudo`. If a tool genuinely requires
root, file a ticket with ASU Research Computing rather than working
around it.

## Using a Service That Runs on Sol, From Your Laptop

The canonical version of this situation: the user wants a Jupyter
notebook running on a Sol GPU and wants to open it in their laptop
browser. Same shape covers dev servers, MLflow UIs, TensorBoard,
OAuth callbacks - anything that listens on a localhost port on Sol
and that the user wants to reach from their own machine.

`solx` is Sol-only (no laptop side), so this stays a manual choice
between two paths - SSH port-forwarding is the mechanism, but the value
is *which path fits*:

1. **Open OnDemand - lowest friction for casual notebook use.** ASU
   Research Computing runs an OnDemand portal that launches Jupyter
   (and other interactive apps) on compute nodes through a web UI:
   no SSH, no tunnels, no terminal. For a one-off browser-based
   notebook on a GPU, this is the right answer. Find the portal URL
   on the official Sol docs (<https://docs.rc.asu.edu/>); confirm
   there or with the user rather than guessing.

2. **Manual SSH tunnel chain - always works, slightly fiddly.** Use
   this when the user needs a terminal-driven workflow, custom env
   vars in the compute-node shell, multiple ports forwarded,
   long-running scripts that wrap the allocation, or anything else
   OnDemand's UI doesn't expose. The Sol-specific catch: compute
   nodes are not internet-reachable, so the laptop can't ssh them
   directly - the chain has to ProxyJump (`-J`) through the login
   node. See [references/sessions.md](references/sessions.md) for
   the worked steps, multi-port stacking, OAuth reverse tunnels,
   and diagnostics.

**Personalize.** Substitute `whoami` into every command you show.
Never emit `<asurite>`. Don't read `~/.ssh/config` or
`~/.ssh/known_hosts` - if the user has a custom `Host` alias they
will tell you.

## Transferring Data

For routine transfers, `rsync -avz` between laptop and Sol's login
node is fine:

```shell
rsync -avz ./local_dir/ $USER@sol.asu.edu:/scratch/$USER/remote_dir/
```

**For large transfers, route through the Data Transfer Node.** Sol
has a dedicated DTN (`soldtn`) tuned for I/O-heavy transfers; pushing
multi-GB datasets through the login node is slower and risks tripping
login-node CPU/IO limits. From a Sol shell, `dtn` (literally
`ssh soldtn`) drops you onto it; from a laptop, target
`soldtn.asu.edu` directly:

```shell
rsync -avz --progress ./big_data/ $USER@soldtn.asu.edu:/scratch/$USER/data/
```

For very large transfers prefer `rsync --progress` (visibility) or
`scp -r` (when you don't need rsync's incremental update logic).

## Working with VS Code

To auto-activate custom commands in VS Code, you can modify `terminal.integrated.env.linux`
and `VSCODE_PYTHON_ZSH_ACTIVATE` in your `settings.json`.

For example, to activate a Python virtual environment, add the following to your `settings.json`:

```json
{
  "terminal.integrated.env.linux": {
    "VSCODE_PYTHON_ZSH_ACTIVATE": "source .venv/bin/activate"
  }
}
```

## Disclaimer

This is a **personal toolkit** (the `solx` CLI and this skill), not an
official ASU Research Computing product. It is published in the hope
that other Sol users find it useful, with the following caveats:

- **Not affiliated with ASU Research Computing.** This skill and `solx`
  are not endorsed, reviewed, or maintained by ASU. The official Sol
  documentation at <https://docs.rc.asu.edu/> is authoritative on
  every policy referenced here (storage retention, partitions, QOS,
  module names, scratch CSV schema). When this skill and the official
  docs disagree, the official docs win - please file an issue so this
  skill can be updated.
- **Conventions go stale.** Sol is a live system: modules get
  renamed, partitions retire, the scratch deletion pipeline can
  change its CSV format. The cached notes in `references/` are
  snapshots from a particular point in time. Verify against
  <https://docs.rc.asu.edu/> if a command does not behave as
  documented here.
- **Use cautiously.** The agent will sometimes propose commands that
  modify files, submit jobs, or open network connections. Always
  preview with `--dry-run` (or the equivalent for the tool in
  question) before executing, and review what the agent proposes
  before approving.
- **Limited test coverage.** This skill and `solx` are exercised
  against a layered eval harness and a unit-test suite, but not every
  behavior is automatically verified - see
  [`docs/coverage.md`](../../docs/coverage.md) for the current matrix
  and known gaps.
- **No warranty.** The skill and `solx` are provided as-is. Review the
  code before running it on data you care about.

If you are unsure whether the agent's proposed action is appropriate
for your account or project on Sol, contact ASU Research Computing
directly rather than acting on the suggestion.
