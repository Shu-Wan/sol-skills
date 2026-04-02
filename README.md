# SOL-skill

An opinionated and personal agent skill for working on ASU's Sol supercomputer.

## Structure

```text
SOL-skill/
├── SKILL.md              # Main skill instructions
├── README.md             # This file
├── references/
│   ├── module.md         # Environment Modules reference
│   ├── sharing.md        # File-sharing procedure
│   └── slurm.md          # Slurm / SBATCH reference
└── scripts/
    └── touch.sh          # Renew scratch file timestamps
```

## References

The `references/` directory contains cleaned-up excerpts from the
ASU Research Computing documentation:

- [module.md](references/module.md) — loading and managing software
  modules
- [sharing.md](references/sharing.md) — sharing files between users
  on the cluster
- [slurm.md](references/slurm.md) — submitting and managing Slurm
  jobs
