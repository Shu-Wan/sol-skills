---
name: sol-skill
description: Tips and conventions for working on ASU's Sol supercomputer. Use this skill when the agent is operating on Sol, submitting SLURM jobs, managing modules, or transferring data on the cluster.
---

# Sol skills

Official doc: <https://docs.rc.asu.edu/>.

This official doc is the source of truth.

## Detecting the Environment

Run `hostname -a` to determine whether you are on Sol or a local
machine. If the output resembles `sc001.sol.rc.asu.edu`, you are on
Sol.

## General Rules

1. Save datasets and caches under `/scratch`.
2. You do not have `sudo` privileges, so maintain a local environment under `/home/$USER/.local` or `/home/$USER/opt`.
3. Use `git` to keep code in sync between local and cluster.

## Modules

Sol uses the **Environment Modules** system to manage software.
Load, list, and unload modules before running any workload.

See [references/module.md](references/module.md) for commands and
naming conventions.

## Filesystem and Storage

Sol provides two main storage areas:

| Location   | Purpose                      | Policy                        |
|------------|------------------------------|-------------------------------|
| `/home/$USER`    | Config, small files          | Limited space, backed up      |
| `/scratch/$USER` | Large data, caches, outputs  | **180-day deletion policy**   |

Always place large data files, model caches, and outputs under
`/scratch/$USER`.

### Renewing the Scratch Timestamp

Files untouched for 180 days are automatically deleted. Use the
bundled helper script to refresh timestamps:

```shell
$SKILL_DIR/scripts/touch.sh -r /scratch/$USER/my_project
```

Run `scripts/touch.sh -h` for all options (dry-run, verbose,
parallel jobs).

### Sharing Files

See [references/sharing.md](references/sharing.md) for the
step-by-step procedure to share files with other users on the
cluster.

## Submitting Jobs

Sol uses **Slurm** to manage jobs. Submit work via SBATCH scripts.

See [references/slurm.md](references/slurm.md) for submission
commands, example scripts (serial, MPI, job arrays),
troubleshooting, and exit codes.

## Transferring Data

Use `rsync` for efficient transfers between local and Sol:

```shell
rsync -avz ./local_dir/ $USER@sol.asu.edu:/scratch/$USER/remote_dir/
```

For large transfers, prefer `rsync --progress` or `scp -r`.

## Python

- Use `uv` to manage Python environments on the cluster.
- Point the `uv` cache to `/scratch` to avoid filling `/home`:

  ```shell
  export UV_CACHE_DIR=/scratch/$USER/.cache/uv
  ```

## LaTeX

Use R package `tinytex` to manage a local TeX Live installation

1. Find the latest R distribution with `module avail r-4`.
2. Use the R package `tinytex` to download a local TeX Live
   distribution under `~/.local/bin/latex`.
3. Install TeX packages on demand:

   ```shell
   tlmgr install <package>
   ```
4. If got `tlmgr is older than remote repository`, it means `tlmgr` needs to be updated.
   This is done through `tinytex::reinstall_tinytex()`.
   Load R module first, then run `Rscript -e "tinytex::reinstall_tinytex(repository = "illinois")"` to update the local
  TeX Live distribution.

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
