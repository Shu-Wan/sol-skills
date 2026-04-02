# Using Software Modules

## Overview

Supercomputing software environments are highly domain-dependent.
To prevent conflicts while balancing ease of use, the software your
research needs must be loaded into the environment as a
**software module**.

No modules are loaded when you start a new session — you must load
software in every session.

## Listing Available Modules

List all available modules:

```shell
module avail
```

Search with a keyword (e.g., `rust`):

```shell
module avail rust
```

Browse modules on the web:

- [Sol modules](https://links.asu.edu/sol-modules)
- [Phoenix modules](https://links.asu.edu/phx-modules)

### Naming Schemes

Research Computing uses two methods to build modules:

| Method | Naming scheme | Example |
|--------|--------------------------------------|-------------------------------|
| Manual | `software/version.number` | `aspect/2.3.0` |
| Spack  | `software-version-compiler-version` | `aspect-2.3.0-gcc-11.2.0` |

There is no functional difference between the two.

## Loading a Module

```shell
module load aspect/2.3.0
```

> **Tip:** use `ml` as a shorthand for `module load`.

## Listing Loaded Modules

```shell
module list
```

## Unloading Modules

Unload a single module:

```shell
module unload aspect/2.3.0
```

Unload **all** loaded modules (useful for starting fresh inside an
SBATCH script):

```shell
module purge
```

## Using Modules in SBATCH Job Scripts

Many SBATCH scripts include a `module load` command.
See [SLURM.md](SLURM.md) for full examples.
