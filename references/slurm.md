# Slurm — SBATCH Job Scripts

## Overview

SBATCH job scripts are the standard way to run work on the
supercomputer. In these scripts you request a resource allocation and
define the work to be done. ASU uses the **Slurm** resource manager,
so all job scripts must be in the SBATCH format.

An SBATCH script is a Bash script with special `#SBATCH` headers that
Slurm interprets before executing the job on compute hardware.

## Submitting Jobs

```shell
sbatch myscript.sh
```

### Overriding Headers at Submission Time

```shell
sbatch -q debug -t 15 -c 16 bigjob.sh
```

This submits `bigjob.sh` to the `debug` QOS with a 15-minute time
limit and 16 CPU cores, overriding any `#SBATCH` headers inside the
script.

## Canceling a Job

Find your job ID with `myjobs`, then cancel it:

```shell
myjobs
scancel <jobID>
```

## Updating a Pending Job

Modify resources of a **pending** job with `scontrol`:

```shell
scontrol update job <jobID> ReqCores=4
```

More examples:

```shell
scontrol update job 11254871 QOS=private Partition=htc
scontrol update job 11254871 Gres=gpu:a100:2
```

## Example SBATCH Scripts

Additional templates live on the cluster at:

- `/packages/public/sol-sbatch-templates/templates/`
- `/packages/public/phx-sbatch-templates/templates/`

### Simple (Serial) Job

```shell
#!/bin/bash

#SBATCH -N 1            # number of nodes
#SBATCH -c 8            # number of cores
#SBATCH -t 0-01:00:00   # time in d-hh:mm:ss
#SBATCH -p general      # partition
#SBATCH -q public       # QOS
#SBATCH -o slurm.%j.out # STDOUT (%j = JobId)
#SBATCH -e slurm.%j.err # STDERR (%j = JobId)
#SBATCH --mail-type=ALL
#SBATCH --mail-user="%u@asu.edu"
#SBATCH --export=NONE   # purge the submitting shell environment

module load mamba/latest
source activate myEnv

cd ~/myResearchDir/test01
python myscript.py
```

### MPI (Parallel) Job

The main difference from a serial job is that parallel jobs allocate
tasks (`-n`) in addition to cores-per-task (`-c`). MPI processes bind
to tasks across one or more nodes.

```shell
#!/bin/bash

#SBATCH -N 3
#SBATCH -n 8            # number of tasks
#SBATCH -c 1            # cores per task (default 1)
#SBATCH -t 0-01:00:00
#SBATCH -p general
#SBATCH -q public
#SBATCH -o slurm.%j.out
#SBATCH -e slurm.%j.err
#SBATCH --mail-type=ALL
#SBATCH --mail-user="%u@asu.edu"
#SBATCH --export=NONE

module load openmpi/4.1.5
module load lammps
srun -n 8 --mpi=pmix software
```

### Job Arrays

Job arrays submit a collection of similar jobs as a single entity —
useful when running the same computation with different input
parameters. A **manifest file** lists the input parameters.

```shell
#!/bin/bash

#SBATCH -N 1
#SBATCH -c 1
#SBATCH --array=1-20
#SBATCH -t 0-01:00:00
#SBATCH -p general
#SBATCH -q public
#SBATCH -o slurm.%j.out
#SBATCH -e slurm.%j.err
#SBATCH --mail-type=ALL
#SBATCH --mail-user="%u@asu.edu"
#SBATCH --export=NONE

readonly manifest="/path/to/manifest_file"
readonly run_opts=($(getline $SLURM_ARRAY_TASK_ID "$manifest"))

module load mamba/latest
source activate myEnv

python myscript.py "${run_opts[@]}"
```

Manifest file (one input path per line):

```shell
/path/to/inputData0001.csv
/path/to/inputData0002.csv
/path/to/inputData0020.csv
```

## Troubleshooting

### DOS Line Breaks

If you created the script on Windows you may see:

```shell
sbatch: error: Batch script contains DOS line breaks (\r\n)
```

Fix with `dos2unix`:

```shell
dos2unix myjob.sh
```

### Missing Shell-Bang Line

```shell
sbatch: error: This does not look like a batch script.
```

Ensure the very first line of every SBATCH script is:

```shell
#!/bin/bash
```

### Invalid Feature Specification

```shell
sbatch: error: Batch job submission failed: Invalid feature specification
```

Check that your script uses the correct partition and QOS for the
target supercomputer.
See [Sol Partitions and QOS](https://asurc.atlassian.net/wiki/spaces/RC/pages/1640103937).

## Slurm Exit Codes

| Code | Meaning |
|-----------|------------------------------------------------|
| 0 | Success |
| 1 | General failure |
| 2 | Incorrect use of shell builtins |
| 3–124 | Application-specific error (check software docs) |
| 125 | Out of memory |
| 126 | Command cannot execute |
| 127 | Command not found |
| 128 | Invalid argument to `exit` |
| 129–192 | Terminated by Linux signal (subtract 128) |

Run `kill -l` to list signal codes, or `man signal` for details.

> **Note:** running `sleep` commands in SBATCH jobs violates
> ASU policy and the job will be purged.
