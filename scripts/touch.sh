#!/bin/bash
set -o errexit
set -o nounset
set -o pipefail

#
# This script updates the last modified timestamp of files that haven't been modified
# in more than a specified number of days (default 180), to prevent auto-deletion by
# systems that remove untouched files after that period.
#
# Usage examples:
#   ./touch.sh /path/to/file                # Touch a single file if older than 90 days
#   ./touch.sh -r /path/to/dir              # Touch all old files in directory recursively
#   ./touch.sh -t 30 -v /path/to/dir        # Touch files older than 30 days, verbose output
#   ./touch.sh -r -j 8 /big/dir             # Use 8 parallel jobs for fast batched touching
#   JOBS=12 ./touch.sh -r /big/dir          # Or set JOBS via environment before running

# --- Default Settings ---
RECURSIVE=false
DRY_RUN=false
VERBOSE=false
TARGET_PATH=""
# Files older than DAYS are considered for touching. Default lowered to 45 as requested.
DAYS=45

# --- Help/Usage Function ---
usage() {
    local exit_code="${1:-1}"
    cat << EOF
Usage: $(basename "$0") [options] <path>

Updates the 'last modified' timestamp of a file or directory of files.

Arguments:
  <path>                The target file or directory path.

Options:
  -r                    Recursively update timestamps for all files within the directory.
                        Required if <path> is a directory.
  -d                    Show which files would be updated without making changes.
  -v                    Print the name of each file as it is being updated.
    -t DAYS              Number of days to consider a file "old" (default: 45).
    -j JOBS              Number of parallel jobs to run when touching files (default: auto-detected).
  -h                    Display this help message and exit.
EOF
    exit "$exit_code"
}

# --- Performance Tuning ---
# Determine parallel jobs for batching (override with env JOBS)
JOBS=${JOBS:-}
if ! [[ "${JOBS:-}" =~ ^[0-9]+$ ]] || [ "${JOBS:-0}" -le 0 ]; then
    if command -v getconf >/dev/null 2>&1; then
        JOBS=$(getconf _NPROCESSORS_ONLN 2>/dev/null || echo 1)
    elif command -v nproc >/dev/null 2>&1; then
        JOBS=$(nproc 2>/dev/null || echo 1)
    else
        JOBS=1
    fi
fi

# --- Argument Parsing ---
while getopts "rdvt:hj:" opt; do
    case $opt in
        r) RECURSIVE=true ;;
        d) DRY_RUN=true ;;
        v) VERBOSE=true ;;
        t)
            if [[ ! "$OPTARG" =~ ^[0-9]+$ ]]; then
                echo "Error: -t requires a positive integer argument." >&2
                usage
            fi
            DAYS="$OPTARG"
            ;;
        j)
            if [[ ! "$OPTARG" =~ ^[0-9]+$ ]] || [ "$OPTARG" -le 0 ]; then
                echo "Error: -j requires a positive integer argument." >&2
                usage
            fi
            JOBS="$OPTARG"
            ;;
        h) usage 0 ;;
        *) usage ;;
    esac
done
shift $((OPTIND-1))

# The remaining argument should be the target path
if [ $# -eq 0 ]; then
    echo "Error: Missing target path." >&2
    usage
elif [ $# -gt 1 ]; then
    echo "Error: Only one path argument is allowed." >&2
    usage
fi
TARGET_PATH="$1"

# If verbose, show resolved JOBS value for tuning
if [ "$VERBOSE" = true ]; then
    printf 'Using JOBS=%s\n' "$JOBS" >&2
fi

# --- Input Validation ---
if [ -z "$TARGET_PATH" ]; then
    echo "Error: Missing target path." >&2
    usage
fi

if [ ! -e "$TARGET_PATH" ]; then
    echo "Error: Path does not exist: '$TARGET_PATH'" >&2
    exit 1
fi

# --- Core Logic ---

# Function to check if a file is older than DAYS days
is_old() {
    local file_path="$1"
    local file_mtime
    file_mtime=$(stat -c %Y "$file_path" 2>/dev/null) || return 1
    local cutoff
    cutoff=$(date -d "$DAYS days ago" +%s 2>/dev/null) || return 1
    [ "$file_mtime" -lt "$cutoff" ]
}

# Function to process a single file
process_file() {
    local file_path="$1"
    if [ "$DRY_RUN" = true ]; then
        printf '[DRY RUN] Would touch: %s\n' "$file_path"
        return
    fi

    # The actual touch command
    if ! touch -- "$file_path"; then
        printf 'Error: Failed to touch: %s\n' "$file_path" >&2
        return 1
    fi

    if [ "$VERBOSE" = true ]; then
        printf 'Updated: %s\n' "$file_path"
    fi
}

# --- Main Execution ---

if [ -f "$TARGET_PATH" ]; then
    # The target is a single file
    if is_old "$TARGET_PATH"; then
        process_file "$TARGET_PATH"
    fi

elif [ -d "$TARGET_PATH" ]; then
    # The target is a directory
    if [ "$RECURSIVE" = false ]; then
        printf 'Error: Target is a directory. Use the -r flag to proceed.\n' >&2
        exit 1
    fi

    # Prepare list of files and count total
    FILELIST_CMD=(find "$TARGET_PATH" -type f -mtime +$((DAYS-1)) -print0)
    total_files=$(${FILELIST_CMD[@]} | tr -cd '\0' | wc -c)

    if [ "$total_files" -eq 0 ]; then
        printf 'No files older than %s days found under %s\n' "$DAYS" "$TARGET_PATH"
    else
        # Create status file for collecting per-file results
        STATUS_FILE=$(mktemp)
        DONE_FILE=$(mktemp)
        export STATUS_FILE DONE_FILE

        start_time=$(date +%s)

        # Determine adaptive xargs chunk size using ARG_MAX
        ARG_MAX_VAL=131072
        if command -v getconf >/dev/null 2>&1; then
            ARG_MAX_VAL=$(getconf ARG_MAX 2>/dev/null || echo 131072)
        fi
        # Reserve some bytes for command overhead
        RESERVED=8192
        # Use up to 80% of ARG_MAX minus reserved
        MAX_BYTES=$(( (ARG_MAX_VAL * 80 / 100) - RESERVED ))

        # Ensure MAX_BYTES is at least 1024 (minimum safe value)
        if [ "$MAX_BYTES" -lt 1024 ]; then
            MAX_BYTES=1024
        fi

        # Cap to 1MB for sanity
        if [ "$MAX_BYTES" -gt $((1024*1024)) ]; then
            MAX_BYTES=$((1024*1024))
        fi

        # Check if xargs supports -s (max-chars) and test it with a safe value
        XARGS_S_ARG=""
        if xargs --help 2>&1 | grep -q -- '-s' && echo | xargs -s 1024 true 2>/dev/null; then
            XARGS_S_ARG="-s $MAX_BYTES"
        else
            # Fallback to -n (max-args) if -s is not supported or fails
            XARGS_S_ARG="-n 100"
        fi

        # Background progress reporter (prints processed/total and files/sec every 1s)
        reporter_pid=""
        if [ "$VERBOSE" = true ]; then
            reporter() {
                last_count=0
                last_time=$(date +%s)
                while true; do
                    # Check if processing is done
                    if [ -f "$DONE_FILE" ]; then
                        break
                    fi

                    processed=$(wc -l <"$STATUS_FILE" 2>/dev/null || echo 0)
                    now=$(date +%s)
                    elapsed=$((now - last_time))
                    if [ $elapsed -le 0 ]; then
                        rate=0
                    else
                        delta=$((processed - last_count))
                        rate=$((delta / elapsed))
                    fi
                    printf '\rProcessed: %d/%d (%.0f/s) ' "$processed" "$total_files" "$rate" >&2

                    last_count=$processed
                    last_time=$now
                    sleep 1
                done
            }

            reporter &
            reporter_pid=$!
        fi

        # Processing function: touch files in parallel and append status lines to STATUS_FILE
        if [ "$DRY_RUN" = true ]; then
            # Dry run: list files to STATUS_FILE using batched workers
            ${FILELIST_CMD[@]} | xargs -0 -r $XARGS_S_ARG -P "$JOBS" bash -c '
                for f in "$@"; do
                    printf "DRY:%s\n" "$f" >>"$STATUS_FILE"
                done
            ' _
        else
            # Real run: process batches of files per worker to reduce process churn
            ${FILELIST_CMD[@]} | xargs -0 -r $XARGS_S_ARG -P "$JOBS" bash -c '
                for f in "$@"; do
                    err=$(touch -- "$f" 2>&1 >/dev/null) || true
                    if [ -z "$err" ]; then
                        printf "OK:%s\n" "$f" >>"$STATUS_FILE"
                    else
                        err_singleline=$(echo "$err" | tr "\n" " ")
                        printf "ERR:%s:%s\n" "$f" "$err_singleline" >>"$STATUS_FILE"
                    fi
                done
            ' _
    fi

        # Wait for all workers to finish
        wait

        # Signal reporter to stop
        touch "$DONE_FILE"

        # Stop reporter and clean up terminal
        if [ -n "$reporter_pid" ]; then
            wait "$reporter_pid" 2>/dev/null || true
            # Clear the progress line and move to a new line
            if [ "$VERBOSE" = true ]; then
                printf '\r%*s\r' 80 '' >&2  # Clear the line
                printf '\n' >&2  # Move to new line
            fi
        fi

        end_time=$(date +%s)
        runtime=$((end_time - start_time))

        # Summarize results
        total_processed=$(wc -l <"$STATUS_FILE" || echo 0)
        ok_count=$(grep -c '^OK:' "$STATUS_FILE" || true)
        err_count=$(grep -c '^ERR:' "$STATUS_FILE" || true)
        dry_count=$(grep -c '^DRY:' "$STATUS_FILE" || true)

        printf '\nSummary:\n'
        printf '  Total files considered: %d\n' "$total_files"
        if [ "$DRY_RUN" = true ]; then
            printf '  Dry-run listed: %d\n' "$dry_count"
        else
            printf '  Succeeded: %d\n' "$ok_count"
            printf '  Failed: %d\n' "$err_count"
        fi
        printf '  Total time (s): %d\n' "$runtime"

        if [ "$err_count" -gt 0 ]; then
            printf '\nFailed files (first 50 shown):\n'
            grep '^ERR:' "$STATUS_FILE" | sed 's/^ERR://g' | head -n 50 | awk -F ':' '{file=$1; $1=""; sub(/^:/,""); print "  "file" ->"$0}'
        fi

        rm -f "$STATUS_FILE" "$DONE_FILE"
    fi
fi

if [ "$DRY_RUN" = true ]; then
    printf '\nDry run complete. No files were changed.\n'
else
    printf '\nOperation complete.\n'
fi

exit 0
