//! `solx keep` - renew scratch files Sol has flagged, filtered by `[keep]`.
//!
//! Read Sol's warning CSVs from `--csv-dir`, intersect the flagged
//! directories with the `[keep]` include/exclude globs from config, and
//! refresh timestamps (`touch -a -m -c` semantics) on only the intersection.
//! Only what Sol has explicitly flagged is renewed - never a wholesale
//! `/scratch` walk.
//!
//! Execution is entry-level-sharded: a streaming pipeline over one worker
//! pool - enumerate a kept directory, split its files and subdirectories
//! into evenly-sized batches, and touch the batches across the pool. A
//! single huge directory fans out into many batches, so `-j` scales the
//! parallelism of the whole run including its largest directory, not just
//! the count of directories.
//!
//! This is metadata-heavy NFS I/O. On Sol run it on a compute node or the
//! DTN (`ssh soldtn`), not a throttled login node.

use std::collections::HashSet;
use std::collections::VecDeque;
use std::ffi::CString;
use std::io::Write;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::sync::{Condvar, Mutex};

use serde_json::{json, Value};

use crate::config::KeepRules;
use crate::output::{confirm, to_python_json, Out};

pub const STAGE_ORDER: [&str; 3] = ["pending", "over90", "inactive"];
pub const STAGES_ALL: &str = "all";

pub fn stage_file(stage: &str) -> &'static str {
    match stage {
        "pending" => "scratch-dirs-pending-removal.csv",
        "over90" => "scratch-dirs-over-90days.csv",
        "inactive" => "scratch-dirs-inactive.csv",
        _ => unreachable!("stage validated by the caller"),
    }
}

/// Files per touch shard. Big enough that per-batch overhead is negligible,
/// small enough that one huge directory fans out into many batches and
/// keeps every worker busy.
pub const BATCH: usize = 2000;

/// Cap on how many dirs are inlined into a JSON payload. Sol's warning CSVs
/// can list thousands of flagged dirs; emitting them all makes a
/// multi-megabyte document that blows an agent's context. The inlined
/// sample is capped and the true totals + a `*_truncated` flag are always
/// reported. Counts are always exact; the lists are a sample.
pub const JSON_LIST_CAP: usize = 100;

/// The default `-j` worker count: `max(1, min(8, ncpus / 4))`.
///
/// `ncpus` is the count of ONLINE system CPUs (`sysconf(_SC_NPROCESSORS_ONLN)`,
/// i.e. Python `os.cpu_count()` semantics), NOT the cgroup/affinity-limited
/// parallelism of the current process - inside a 4-core Slurm allocation on a
/// 128-CPU node the default is still 8.
pub fn default_jobs() -> u64 {
    let n = unsafe { libc::sysconf(libc::_SC_NPROCESSORS_ONLN) };
    let cpus = if n > 0 { n as u64 } else { 2 };
    (cpus / 4).clamp(1, 8)
}

/// The directories `solx keep` would touch (`kept`) vs filter out (`skipped`),
/// each tagged with the warning stage that flagged it.
#[derive(Debug, Default, Clone)]
pub struct Plan {
    pub kept: Vec<(String, String)>,
    pub skipped: Vec<(String, String)>,
}

// --- planning ----------------------------------------------------------------

/// Return the `Directory` column from one of Sol's warning CSVs.
///
/// A missing file is fine - Sol only drops the CSV when there's something
/// to flag. An empty result means nothing to do for that stage. An existing
/// file that can't be read or decoded is a hard error (the command must
/// fail loudly rather than treat the stage as "nothing flagged").
///
/// A UTF-8 BOM is treated as part of the first header cell's name (so a
/// BOM'd `Directory` header is not the `Directory` column and the file
/// yields no directories).
pub fn load_csv_dirs(csv_path: &Path) -> Result<Vec<String>, String> {
    if !csv_path.exists() {
        return Ok(Vec::new());
    }
    let read_err =
        |e: &dyn std::fmt::Display| format!("unable to read {}: {e}", csv_path.display());
    let has_bom = std::fs::File::open(csv_path)
        .and_then(|mut f| {
            use std::io::Read;
            let mut head = [0u8; 3];
            let n = f.read(&mut head)?;
            Ok(n == 3 && head == [0xEF, 0xBB, 0xBF])
        })
        .map_err(|e| read_err(&e))?;
    let mut reader = csv::ReaderBuilder::new()
        .flexible(true)
        .from_path(csv_path)
        .map_err(|e| read_err(&e))?;
    let headers = reader.headers().map_err(|e| read_err(&e))?;
    let dir_idx = match headers
        .iter()
        .enumerate()
        .position(|(i, name)| name == "Directory" && !(i == 0 && has_bom))
    {
        Some(i) => i,
        None => return Ok(Vec::new()),
    };
    let mut dirs = Vec::new();
    for record in reader.records() {
        let record = record.map_err(|e| read_err(&e))?;
        if let Some(d) = record.get(dir_idx) {
            let d = d.trim();
            if !d.is_empty() {
                dirs.push(d.to_string());
            }
        }
    }
    Ok(dirs)
}

/// Walk the chosen stages' CSVs and split flagged dirs into kept/skipped.
pub fn build_plan(csv_dir: &Path, stages: &[String], keep: &KeepRules) -> Result<Plan, String> {
    let mut plan = Plan::default();
    let mut seen: HashSet<String> = HashSet::new();
    for stage in stages {
        for d in load_csv_dirs(&csv_dir.join(stage_file(stage)))? {
            if !seen.insert(d.clone()) {
                continue;
            }
            let entry = (stage.clone(), d.clone());
            if keep.matches(&d) {
                plan.kept.push(entry);
            } else {
                plan.skipped.push(entry);
            }
        }
    }
    Ok(plan)
}

// --- enumeration + touching ---------------------------------------------------
//
// Two task kinds run on one worker pool:
//   enumerate_dir  -- walk a kept directory, return its entries
//   touch_entries  -- refresh timestamps on a batch of those entries
// touch is the expensive half (one metadata write per entry), so it is
// sharded into batches and spread across the pool.

/// One kept directory's entries, as `enumerate_dir` found them.
#[derive(Debug, Default)]
pub struct Walk {
    /// Regular files under the directory (`find DIR -type f`).
    pub files: Vec<PathBuf>,
    /// The directory itself plus every subdirectory (`find DIR -type d`).
    pub dirs: Vec<PathBuf>,
    /// `ok`, a `skipped: ...` note, or the walk error.
    pub msg: String,
}

/// List every regular file and directory under `directory` in one walk.
///
/// Matches `find DIR -type f` plus `find DIR -type d`: hidden entries
/// included, no ignore files honored, symlinks not followed (so a symlink -
/// to a file or a directory - is never touched, and never walked into).
/// `dirs` holds the directory itself along with every subdirectory, so the
/// flagged directory's own timestamp is renewed too: touching a file does
/// not move its parent's mtime, so a directory's stamp otherwise only ever
/// moves when an entry is added or removed. Nothing here depends on the
/// order the walker yields entries in (it yields the root first, which
/// `enumerate_dir_lists_all_including_hidden_and_ignored` pins) - every
/// entry is touched exactly once either way. A path that isn't a directory
/// (e.g. flagged then removed) is reported as a benign skip, not an error.
pub fn enumerate_dir(directory: &str) -> Walk {
    if !Path::new(directory).is_dir() {
        return Walk {
            msg: "skipped: not a directory".to_string(),
            ..Walk::default()
        };
    }
    let walker = ignore::WalkBuilder::new(directory)
        .hidden(false)
        .ignore(false)
        .git_ignore(false)
        .git_global(false)
        .git_exclude(false)
        .parents(false)
        .follow_links(false)
        .build();
    let mut files = Vec::new();
    let mut dirs = Vec::new();
    let mut walk_error: Option<String> = None;
    for entry in walker {
        match entry {
            Ok(e) => match e.file_type() {
                Some(t) if t.is_file() => files.push(e.into_path()),
                Some(t) if t.is_dir() => dirs.push(e.into_path()),
                _ => {}
            },
            Err(e) => walk_error = Some(e.to_string()),
        }
    }
    if let Some(msg) = walk_error {
        return Walk {
            msg,
            ..Walk::default()
        };
    }
    Walk {
        files,
        dirs,
        msg: "ok".to_string(),
    }
}

/// Set one path's atime+mtime to now - `touch -a -m` on a single entry.
///
/// `utimensat` with a NULL `times` is the "both stamps to now" form, and
/// per utimensat(2) it needs only **write permission** on the entry.
/// Passing explicit stamps - what `filetime::set_file_times` does, even
/// for "now" - lands in the other clause of the same rule and requires
/// **ownership**, which EPERMs on a collaborator's `0666`/`0777` file
/// sitting in your own `/scratch` tree. Shared project directories are
/// full of exactly those files, so keep renews with the form plain
/// coreutils `touch` uses.
fn touch_now(path: &Path) -> std::io::Result<()> {
    let c_path = CString::new(path.as_os_str().as_bytes())
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidInput, e))?;
    // SAFETY: `c_path` is a valid NUL-terminated string that outlives the
    // call, and a null `times` is the documented set-both-to-now form.
    let rc = unsafe { libc::utimensat(libc::AT_FDCWD, c_path.as_ptr(), std::ptr::null(), 0) };
    if rc == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}

/// Refresh atime+mtime on a batch of entries (`touch -a -m -c` semantics).
///
/// Returns `(renewed, errors, message)`: how many entries got fresh stamps,
/// how many failed, and the first failure suffixed with the count of the
/// rest (`... (and N more in this batch)`) - a whole shard can fail, and
/// the summary has to show that without emitting `BATCH` lines. An entry
/// deleted between enumeration and touch is neither renewed nor an error,
/// and nothing is ever created.
pub fn touch_entries(paths: &[PathBuf]) -> (usize, usize, String) {
    let mut renewed = 0;
    let mut errors = 0;
    let mut msg = "ok".to_string();
    for p in paths {
        match touch_now(p) {
            Ok(()) => renewed += 1,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => {
                errors += 1;
                if errors == 1 {
                    msg = format!("touch {}: {e}", p.display());
                }
            }
        }
    }
    if errors > 1 {
        msg = format!("{msg} (and {} more in this batch)", errors - 1);
    }
    (renewed, errors, msg)
}

/// Split a flat entry list into evenly-sized batches for the touch pool.
pub fn shard(files: Vec<PathBuf>, batch_size: usize) -> Vec<Vec<PathBuf>> {
    if files.is_empty() {
        return Vec::new();
    }
    let mut batches = Vec::with_capacity(files.len().div_ceil(batch_size));
    let mut current = Vec::with_capacity(batch_size.min(files.len()));
    for f in files {
        current.push(f);
        if current.len() == batch_size {
            batches.push(std::mem::take(&mut current));
        }
    }
    if !current.is_empty() {
        batches.push(current);
    }
    batches
}

// --- command -------------------------------------------------------------------

pub struct KeepOptions<'a> {
    pub csv_dir: Option<PathBuf>,
    pub stage: String,
    pub jobs_n: u64,
    pub yes: bool,
    pub dry_run: bool,
    pub verbose: bool,
    pub config_keep: Option<&'a KeepRules>,
}

pub fn cmd_keep(opts: &KeepOptions, out: &Out) -> i32 {
    if opts.yes && opts.dry_run {
        out.error("error: --yes and --dry-run are mutually exclusive");
        return 2;
    }

    // The keep-list comes from the config `[keep]` block - the single source
    // of truth.
    let keep_rules: &KeepRules = match opts.config_keep {
        Some(rules) => rules,
        None => {
            out.error("error: no [keep] block in config. add one with `solx config edit`.");
            return 2;
        }
    };

    let csv_dir = opts.csv_dir.clone().unwrap_or_else(crate::config::home_dir);
    if !csv_dir.is_dir() {
        out.error(&format!(
            "error: --csv-dir {} is not a directory \
             (Sol drops the warning CSVs in $HOME).",
            csv_dir.display()
        ));
        return 2;
    }
    let stages: Vec<String> = if opts.stage == STAGES_ALL {
        STAGE_ORDER.iter().map(|s| s.to_string()).collect()
    } else {
        vec![opts.stage.clone()]
    };

    let plan = match build_plan(&csv_dir, &stages, keep_rules) {
        Ok(p) => p,
        Err(e) => {
            out.error(&format!("error: {e}"));
            return 1;
        }
    };
    if let Err(e) = report_plan(out, &plan, &csv_dir, &stages, opts.verbose) {
        out.error(&format!("error: {e}"));
        return 1;
    }

    if plan.kept.is_empty() {
        if out.json_mode {
            // Still emit a document so an agent gets structured output, not
            // empty stdout, when nothing is flagged.
            match plan_json(&plan, &csv_dir, &stages, opts.dry_run) {
                Ok(doc) => out.json(&doc),
                Err(e) => {
                    out.error(&format!("error: {e}"));
                    return 1;
                }
            }
        } else {
            out.status("no flagged directories matched - nothing to do.");
        }
        return 0;
    }

    if opts.dry_run {
        if out.json_mode {
            match plan_json(&plan, &csv_dir, &stages, true) {
                Ok(doc) => out.json(&doc),
                Err(e) => {
                    out.error(&format!("error: {e}"));
                    return 1;
                }
            }
        }
        return 0;
    }

    if !opts.yes {
        // Destructive: never block on a prompt in a non-interactive session.
        if !out.interactive {
            out.error(&format!(
                "error: non-interactive session - pass -y to renew {} \
                 directories, or -n to preview.",
                plan.kept.len()
            ));
            return 2;
        }
        if !confirm(
            &format!("Touch mtimes on {} directories?", plan.kept.len()),
            false,
        ) {
            out.status("aborted");
            return 1;
        }
    }

    let renewal = execute(&plan, opts.jobs_n, out);

    if out.json_mode {
        let kept_truncated = plan.kept.len() > JSON_LIST_CAP;
        let mut summary = json!({
            "renewed": true,
            "dirs": plan.kept.len(),
            "files_touched": renewal.files,
            "dirs_touched": renewal.dirs,
            "failures": renewal.failures,
            "kept_truncated": kept_truncated,
            "kept": plan.kept.iter().take(JSON_LIST_CAP).map(|(_, d)| d.clone()).collect::<Vec<_>>(),
        });
        if kept_truncated {
            match dump_full_plan(&plan, &csv_dir, &stages) {
                Ok(path) => summary["full_plan_path"] = json!(path),
                Err(e) => {
                    out.error(&format!("error: {e}"));
                    return 1;
                }
            }
        }
        out.json(&summary);
    } else {
        let failed = if renewal.failures > 0 {
            format!(" · {} failed", renewal.failures)
        } else {
            String::new()
        };
        out.status(&format!(
            "done {} flagged dirs · touched {} files + {} dirs{failed}",
            plan.kept.len(),
            renewal.files,
            renewal.dirs
        ));
    }
    if renewal.failures > 0 {
        1
    } else {
        0
    }
}

/// Print the plan summary to stderr (human) - stdout stays the data channel.
fn report_plan(
    out: &Out,
    plan: &Plan,
    csv_dir: &Path,
    stages: &[String],
    verbose: bool,
) -> Result<(), String> {
    if out.json_mode {
        return Ok(());
    }
    out.status(&format!(
        "csv-dir: {}  stages: {}",
        csv_dir.display(),
        stages.join(", ")
    ));
    out.status(&format!(
        "plan: {} kept, {} skipped",
        plan.kept.len(),
        plan.skipped.len()
    ));
    if plan.kept.len() > JSON_LIST_CAP || plan.skipped.len() > JSON_LIST_CAP {
        let path = dump_full_plan(plan, csv_dir, stages)?;
        out.status(&format!(
            "full plan ({} dirs): {path}",
            plan.kept.len() + plan.skipped.len()
        ));
    }
    if verbose {
        if !plan.kept.is_empty() {
            out.status("kept:");
            for (stage, d) in plan.kept.iter().take(20) {
                out.status(&format!("  {stage:>9} {d}"));
            }
            if plan.kept.len() > 20 {
                out.status(&format!("  ... and {} more", plan.kept.len() - 20));
            }
        }
        if !plan.skipped.is_empty() {
            out.status("skipped (flagged by Sol but not in [keep]):");
            for (stage, d) in plan.skipped.iter().take(20) {
                out.status(&format!("  {stage:>9} {d}"));
            }
        }
    }
    Ok(())
}

/// Bounded plan document: exact counts, a capped sample of each list.
///
/// When either list is truncated, the COMPLETE plan is spilled to a temp
/// file and its path returned under `full_plan_path` - so the response
/// stays small enough for an agent's context while the full detail is one
/// `cat` away.
fn plan_json(
    plan: &Plan,
    csv_dir: &Path,
    stages: &[String],
    dry_run: bool,
) -> Result<Value, String> {
    let entry = |(stage, dir): &(String, String)| json!({"stage": stage, "dir": dir});
    let kept_truncated = plan.kept.len() > JSON_LIST_CAP;
    let skipped_truncated = plan.skipped.len() > JSON_LIST_CAP;
    let mut doc = json!({
        "dry_run": dry_run,
        "csv_dir": csv_dir.display().to_string(),
        "stages": stages,
        "kept_count": plan.kept.len(),
        "skipped_count": plan.skipped.len(),
        "kept_truncated": kept_truncated,
        "skipped_truncated": skipped_truncated,
        "kept": plan.kept.iter().take(JSON_LIST_CAP).map(entry).collect::<Vec<_>>(),
        "skipped": plan.skipped.iter().take(JSON_LIST_CAP).map(entry).collect::<Vec<_>>(),
    });
    if kept_truncated || skipped_truncated {
        doc["full_plan_path"] = json!(dump_full_plan(plan, csv_dir, stages)?);
    }
    Ok(doc)
}

/// Write the complete (untruncated) plan to `solx-keep-plan-*.json` in the
/// system temp dir; return its path.
///
/// The file is created owner-only (0600) with bounded name-collision
/// retries, and stays on disk after the run. A creation or write failure is
/// an error (the document enumerates the user's scratch layout, so a
/// truncated or missing spill must never be advertised as complete).
fn dump_full_plan(plan: &Plan, csv_dir: &Path, stages: &[String]) -> Result<String, String> {
    let entry = |(stage, dir): &(String, String)| json!({"stage": stage, "dir": dir});
    let doc = json!({
        "csv_dir": csv_dir.display().to_string(),
        "stages": stages,
        "kept": plan.kept.iter().map(entry).collect::<Vec<_>>(),
        "skipped": plan.skipped.iter().map(entry).collect::<Vec<_>>(),
    });
    let temp = tempfile::Builder::new()
        .prefix("solx-keep-plan-")
        .suffix(".json")
        .tempfile()
        .map_err(|e| format!("unable to create the full-plan temp file: {e}"))?;
    let (mut file, path) = temp
        .keep()
        .map_err(|e| format!("unable to keep the full-plan temp file: {e}"))?;
    file.write_all(to_python_json(&doc).as_bytes())
        .map_err(|e| format!("unable to write {}: {e}", path.display()))?;
    Ok(path.display().to_string())
}

// --- execution -------------------------------------------------------------------

enum Task {
    Enumerate(String),
    Touch(String, Vec<PathBuf>, Kind),
}

/// Which counter a touched batch lands in.
#[derive(Clone, Copy)]
enum Kind {
    Files,
    Dirs,
}

/// What a renewal pass actually renewed.
///
/// `files` and `dirs` count entries that got fresh stamps - an entry that
/// vanished between enumeration and touch is in neither. `failures` counts
/// failed *operations*: one per entry that could not be touched, plus one
/// per directory that could not be walked.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Renewal {
    pub files: usize,
    pub dirs: usize,
    pub failures: usize,
}

impl Renewal {
    fn add(&mut self, other: &Renewal) {
        self.files += other.files;
        self.dirs += other.dirs;
        self.failures += other.failures;
    }
}

/// The serial mode's per-directory progress line.
///
/// It reports what the directory actually *renewed*, not what the walk
/// found, and drops the `ok` tag as soon as anything failed - a line that
/// reads `ok 1386 files` over a directory where every touch was refused is
/// the same under-reporting the batch counter used to do.
fn dir_status_line(one: &Renewal, directory: &str) -> String {
    let (tag, failed) = if one.failures > 0 {
        ("fail", format!(" · {} failed", one.failures))
    } else {
        ("ok", String::new())
    };
    format!(
        "  {tag:<4} {:>7} files {:>6} dirs{failed}  {directory}",
        one.files, one.dirs
    )
}

struct PoolState {
    queue: VecDeque<Task>,
    in_flight: usize,
    renewal: Renewal,
}

/// Renew `plan.kept`.
///
/// With `jobs_n <= 1` runs serially (no pool - fast and deterministic for
/// small runs). Otherwise one worker pool runs both halves: enumerate a
/// directory, shard its entries, and queue the batches as touch tasks, so a
/// single huge directory spreads its batches over every worker.
pub fn execute(plan: &Plan, jobs_n: u64, out: &Out) -> Renewal {
    if jobs_n <= 1 {
        return execute_serial(plan, out);
    }

    let state = Mutex::new(PoolState {
        queue: plan
            .kept
            .iter()
            .map(|(_, d)| Task::Enumerate(d.clone()))
            .collect(),
        in_flight: 0,
        renewal: Renewal::default(),
    });
    let ready = Condvar::new();
    let out = *out;

    std::thread::scope(|scope| {
        for _ in 0..jobs_n {
            scope.spawn(|| worker(&state, &ready, &out));
        }
    });

    state.into_inner().expect("pool lock").renewal
}

fn worker(state: &Mutex<PoolState>, ready: &Condvar, out: &Out) {
    loop {
        let task = {
            let mut s = state.lock().expect("pool lock");
            loop {
                if let Some(task) = s.queue.pop_front() {
                    s.in_flight += 1;
                    break task;
                }
                if s.in_flight == 0 {
                    // Nothing queued and nothing running: the pipeline drained.
                    ready.notify_all();
                    return;
                }
                s = ready.wait(s).expect("pool lock");
            }
        };

        match task {
            Task::Enumerate(d) => {
                let walk = enumerate_dir(&d);
                let mut s = state.lock().expect("pool lock");
                if walk.msg == "ok" {
                    for batch in shard(walk.files, BATCH) {
                        s.queue
                            .push_back(Task::Touch(d.clone(), batch, Kind::Files));
                    }
                    for batch in shard(walk.dirs, BATCH) {
                        s.queue.push_back(Task::Touch(d.clone(), batch, Kind::Dirs));
                    }
                } else if !walk.msg.starts_with("skipped") {
                    s.renewal.failures += 1;
                    out.error(&format!("FAIL enumerate {d} :: {}", walk.msg));
                }
                s.in_flight -= 1;
                ready.notify_all();
            }
            Task::Touch(d, batch, kind) => {
                let (n, errs, msg) = touch_entries(&batch);
                let mut s = state.lock().expect("pool lock");
                match kind {
                    Kind::Files => s.renewal.files += n,
                    Kind::Dirs => s.renewal.dirs += n,
                }
                if errs > 0 {
                    s.renewal.failures += errs;
                    out.error(&format!("FAIL touch {d} :: {msg}"));
                }
                s.in_flight -= 1;
                ready.notify_all();
            }
        }
    }
}

fn execute_serial(plan: &Plan, out: &Out) -> Renewal {
    let mut renewal = Renewal::default();
    for (_, d) in &plan.kept {
        let walk = enumerate_dir(d);
        if walk.msg != "ok" {
            if !walk.msg.starts_with("skipped") {
                renewal.failures += 1;
                out.error(&format!("FAIL enumerate {d} :: {}", walk.msg));
            }
            continue;
        }
        let mut one = Renewal::default();
        for (batch, kind) in shard(walk.files, BATCH)
            .into_iter()
            .map(|b| (b, Kind::Files))
            .chain(shard(walk.dirs, BATCH).into_iter().map(|b| (b, Kind::Dirs)))
        {
            let (n, errs, tmsg) = touch_entries(&batch);
            match kind {
                Kind::Files => one.files += n,
                Kind::Dirs => one.dirs += n,
            }
            if errs > 0 {
                one.failures += errs;
                out.error(&format!("FAIL touch {d} :: {tmsg}"));
            }
        }
        if !out.json_mode {
            out.status(&dir_status_line(&one, d));
        }
        renewal.add(&one);
    }
    renewal
}

#[cfg(test)]
mod tests {
    use super::*;
    use filetime::FileTime;
    use std::fs;

    fn keep(include: &[&str], exclude: &[&str]) -> KeepRules {
        KeepRules::new(
            &include.iter().map(|s| s.to_string()).collect::<Vec<_>>(),
            &exclude.iter().map(|s| s.to_string()).collect::<Vec<_>>(),
        )
    }

    fn write_csv(path: &Path, dirs: &[&str]) {
        let mut lines = vec!["Directory,LastAccess,Size".to_string()];
        lines.extend(dirs.iter().map(|d| format!("{d},2026-01-01,1G")));
        fs::write(path, lines.join("\n") + "\n").unwrap();
    }

    fn stages_all() -> Vec<String> {
        STAGE_ORDER.iter().map(|s| s.to_string()).collect()
    }

    // ---- planning ------------------------------------------------------------

    #[test]
    fn load_csv_dirs_reads_directory_column() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("scratch-dirs-pending-removal.csv");
        write_csv(&p, &["/scratch/sparky/a", "/scratch/sparky/b"]);
        assert_eq!(
            load_csv_dirs(&p).unwrap(),
            ["/scratch/sparky/a", "/scratch/sparky/b"]
        );
    }

    #[test]
    fn load_csv_dirs_missing_file() {
        let dir = tempfile::tempdir().unwrap();
        assert!(load_csv_dirs(&dir.path().join("absent.csv"))
            .unwrap()
            .is_empty());
    }

    #[test]
    fn load_csv_dirs_directory_not_first_column() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("x.csv");
        fs::write(&p, "User,Directory,Size\nsparky,/scratch/sparky/a,12G\n").unwrap();
        assert_eq!(load_csv_dirs(&p).unwrap(), ["/scratch/sparky/a"]);
    }

    #[test]
    fn load_csv_dirs_bom_header_yields_no_directories() {
        // A BOM is part of the first header cell's name, so the column
        // lookup misses and the file contributes nothing.
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("bom.csv");
        fs::write(&p, b"\xEF\xBB\xBFDirectory,Size\n/scratch/sparky/a,1G\n").unwrap();
        assert!(load_csv_dirs(&p).unwrap().is_empty());
        // With the Directory column not first, the BOM lands on another
        // header and the column still resolves.
        let p2 = dir.path().join("bom2.csv");
        fs::write(&p2, b"\xEF\xBB\xBFSize,Directory\n1G,/scratch/sparky/a\n").unwrap();
        assert_eq!(load_csv_dirs(&p2).unwrap(), ["/scratch/sparky/a"]);
    }

    #[test]
    fn load_csv_dirs_invalid_utf8_record_is_error() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("bad.csv");
        fs::write(&p, b"Directory,Size\n/scratch/sparky/\xFF\xFE,1G\n").unwrap();
        let err = load_csv_dirs(&p).unwrap_err();
        assert!(err.contains("unable to read"));
        assert!(err.contains("bad.csv"));
    }

    #[test]
    fn load_csv_dirs_unreadable_file_is_error() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("locked.csv");
        write_csv(&p, &["/scratch/sparky/a"]);
        fs::set_permissions(&p, fs::Permissions::from_mode(0o000)).unwrap();
        let err = load_csv_dirs(&p).unwrap_err();
        fs::set_permissions(&p, fs::Permissions::from_mode(0o644)).unwrap();
        assert!(err.contains("unable to read"));
        assert!(err.contains("locked.csv"));
    }

    #[test]
    fn build_plan_filters_by_keep() {
        let dir = tempfile::tempdir().unwrap();
        write_csv(
            &dir.path().join("scratch-dirs-pending-removal.csv"),
            &["/scratch/sparky/proj-a", "/scratch/sparky/proj-z"],
        );
        write_csv(
            &dir.path().join("scratch-dirs-over-90days.csv"),
            &["/scratch/sparky/proj-b"],
        );
        let rules = keep(&["/scratch/sparky/proj-a", "/scratch/sparky/proj-b"], &[]);
        let plan = build_plan(dir.path(), &stages_all(), &rules).unwrap();
        let kept: Vec<&str> = plan.kept.iter().map(|(_, d)| d.as_str()).collect();
        assert_eq!(kept, ["/scratch/sparky/proj-a", "/scratch/sparky/proj-b"]);
        let skipped: Vec<&str> = plan.skipped.iter().map(|(_, d)| d.as_str()).collect();
        assert_eq!(skipped, ["/scratch/sparky/proj-z"]);
    }

    #[test]
    fn build_plan_dedupes_across_stages() {
        let dir = tempfile::tempdir().unwrap();
        write_csv(
            &dir.path().join("scratch-dirs-pending-removal.csv"),
            &["/scratch/sparky/a"],
        );
        write_csv(
            &dir.path().join("scratch-dirs-over-90days.csv"),
            &["/scratch/sparky/a"],
        );
        let rules = keep(&["/scratch/sparky/a"], &[]);
        let plan = build_plan(dir.path(), &stages_all(), &rules).unwrap();
        assert_eq!(plan.kept.len(), 1);
        assert_eq!(plan.kept[0].0, "pending"); // first stage wins
    }

    #[test]
    fn build_plan_exclude_carve_out() {
        let dir = tempfile::tempdir().unwrap();
        write_csv(
            &dir.path().join("scratch-dirs-pending-removal.csv"),
            &[
                "/scratch/sparky/proj/run-1",
                "/scratch/sparky/proj/__pycache__",
            ],
        );
        let rules = keep(&["/scratch/sparky/proj/**"], &["**/__pycache__"]);
        let plan = build_plan(dir.path(), &["pending".to_string()], &rules).unwrap();
        let kept: Vec<&str> = plan.kept.iter().map(|(_, d)| d.as_str()).collect();
        assert_eq!(kept, ["/scratch/sparky/proj/run-1"]);
        let skipped: Vec<&str> = plan.skipped.iter().map(|(_, d)| d.as_str()).collect();
        assert_eq!(skipped, ["/scratch/sparky/proj/__pycache__"]);
    }

    #[test]
    fn build_plan_negation_last_match_wins() {
        // `!` carve-outs within the include list (gitignore last-match-wins).
        let dir = tempfile::tempdir().unwrap();
        let rules = keep(&["/scratch/sparky/proj", "!**/__pycache__"], &[]);
        write_csv(
            &dir.path().join("scratch-dirs-pending-removal.csv"),
            &[
                "/scratch/sparky/proj/run",
                "/scratch/sparky/proj/__pycache__",
                "/scratch/sparky/x",
            ],
        );
        let plan = build_plan(dir.path(), &["pending".to_string()], &rules).unwrap();
        let kept: Vec<&str> = plan.kept.iter().map(|(_, d)| d.as_str()).collect();
        assert_eq!(kept, ["/scratch/sparky/proj/run"]);
    }

    // ---- shard / enumerate / touch (the renewal mechanism) ----------------------

    #[test]
    fn shard_even_batches() {
        let files: Vec<PathBuf> = (0..10).map(|i| PathBuf::from(format!("f{i}"))).collect();
        let batches = shard(files.clone(), 3);
        let sizes: Vec<usize> = batches.iter().map(|b| b.len()).collect();
        assert_eq!(sizes, [3, 3, 3, 1]);
        let flat: Vec<PathBuf> = batches.into_iter().flatten().collect();
        assert_eq!(flat, files);
    }

    #[test]
    fn shard_empty() {
        assert!(shard(Vec::new(), BATCH).is_empty());
    }

    #[test]
    fn enumerate_dir_lists_all_including_hidden_and_ignored() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("a.txt"), "x").unwrap();
        fs::write(dir.path().join(".hidden"), "x").unwrap();
        fs::create_dir(dir.path().join("sub")).unwrap();
        fs::write(dir.path().join("sub/b.txt"), "x").unwrap();
        // A .gitignore plus an ignored file: both must still be listed.
        fs::write(dir.path().join(".gitignore"), "ignored.txt\n").unwrap();
        fs::write(dir.path().join("ignored.txt"), "x").unwrap();

        let walk = enumerate_dir(dir.path().to_str().unwrap());
        assert_eq!(walk.msg, "ok");
        assert!(walk.files.iter().all(|p| p.is_file()));
        // 5 regular files: a.txt, .hidden, sub/b.txt, .gitignore, ignored.txt
        assert_eq!(walk.files.len(), 5);
        // The flagged directory itself comes first, then `sub`.
        assert_eq!(
            walk.dirs,
            [dir.path().to_path_buf(), dir.path().join("sub")]
        );
    }

    #[test]
    fn enumerate_dir_skips_symlinked_dirs() {
        // `find -type d` does not count a symlink to a directory, and the
        // walker does not descend into one either.
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir(dir.path().join("real")).unwrap();
        fs::write(dir.path().join("real/inside.txt"), "x").unwrap();
        std::os::unix::fs::symlink(dir.path().join("real"), dir.path().join("link")).unwrap();
        let walk = enumerate_dir(dir.path().to_str().unwrap());
        assert_eq!(walk.msg, "ok");
        assert_eq!(walk.dirs.len(), 2); // root + real
        assert_eq!(walk.files.len(), 1); // real/inside.txt, not through the link
    }

    #[test]
    fn enumerate_dir_skips_symlinked_files() {
        // `find -type f` does not count symlinks; neither does the walker.
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("real.txt"), "x").unwrap();
        std::os::unix::fs::symlink(dir.path().join("real.txt"), dir.path().join("link.txt"))
            .unwrap();
        let walk = enumerate_dir(dir.path().to_str().unwrap());
        assert_eq!(walk.msg, "ok");
        assert_eq!(walk.files.len(), 1);
    }

    #[test]
    fn enumerate_dir_not_a_directory() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("nope");
        let walk = enumerate_dir(missing.to_str().unwrap());
        assert!(walk.files.is_empty());
        assert!(walk.dirs.is_empty());
        assert!(walk.msg.starts_with("skipped"));
    }

    /// Backdate an entry so a renewal is visible.
    fn backdate(p: &Path) {
        let old = FileTime::from_unix_time(FileTime::now().unix_seconds() - 8_640_000, 0);
        filetime::set_file_times(p, old, old).unwrap();
    }

    fn is_fresh(p: &Path) -> bool {
        let mtime = FileTime::from_last_modification_time(&p.metadata().unwrap());
        mtime.unix_seconds() > FileTime::now().unix_seconds() - 10
    }

    #[test]
    fn touch_entries_refreshes_times() {
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("stale.txt");
        fs::write(&f, "x").unwrap();
        backdate(&f);

        let (renewed, errors, _) = touch_entries(std::slice::from_ref(&f));
        assert_eq!((renewed, errors), (1, 0));
        assert!(is_fresh(&f));
    }

    #[test]
    fn touch_entries_refreshes_a_directory() {
        // A directory's own stamp only moves when an entry is added or
        // removed, so `keep` has to touch it directly.
        let dir = tempfile::tempdir().unwrap();
        let sub = dir.path().join("sub");
        fs::create_dir(&sub).unwrap();
        backdate(&sub);

        let (renewed, errors, _) = touch_entries(std::slice::from_ref(&sub));
        assert_eq!((renewed, errors), (1, 0));
        assert!(is_fresh(&sub));
    }

    #[test]
    fn touch_entries_missing_path_is_silent_skip() {
        let dir = tempfile::tempdir().unwrap();
        let ghost = dir.path().join("gone.txt");
        let (renewed, errors, msg) = touch_entries(std::slice::from_ref(&ghost));
        assert_eq!((renewed, errors), (0, 0)); // not renewed, not a failure
        assert_eq!(msg, "ok");
        assert!(!ghost.exists()); // never created
    }

    #[test]
    fn touch_entries_counts_every_failure_in_the_batch() {
        // Every failure counts, and the message names the first one plus how
        // many followed - a whole shard can fail (a collaborator's files
        // before the utimensat fix), and one line must not stand for 2000.
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("regular.txt");
        fs::write(&f, "x").unwrap();
        // A path *through* a regular file is ENOTDIR, not NotFound.
        let bad: Vec<PathBuf> = (0..3).map(|i| f.join(format!("ghost-{i}"))).collect();
        let batch: Vec<PathBuf> = bad.iter().cloned().chain([f.clone()]).collect();

        let (renewed, errors, msg) = touch_entries(&batch);
        assert_eq!((renewed, errors), (1, 3));
        assert!(
            msg.starts_with(&format!("touch {}", bad[0].display())),
            "{msg}"
        );
        assert!(msg.ends_with("(and 2 more in this batch)"), "{msg}");
    }

    #[test]
    fn touch_entries_empty_batch() {
        assert_eq!(touch_entries(&[]), (0, 0, "ok".to_string()));
    }

    #[test]
    fn dir_status_line_reports_renewed_counts() {
        assert_eq!(
            dir_status_line(
                &Renewal {
                    files: 1386,
                    dirs: 694,
                    failures: 0
                },
                "/scratch/sparky/proj"
            ),
            "  ok      1386 files    694 dirs  /scratch/sparky/proj"
        );
    }

    #[test]
    fn dir_status_line_drops_the_ok_tag_when_anything_failed() {
        // What the walk found is not what got renewed: a directory whose
        // every touch was refused must not print as `ok`.
        assert_eq!(
            dir_status_line(
                &Renewal {
                    files: 0,
                    dirs: 0,
                    failures: 1386
                },
                "/scratch/sparky/proj"
            ),
            "  fail       0 files      0 dirs · 1386 failed  /scratch/sparky/proj"
        );
    }

    #[test]
    fn execute_serial_counts_and_skips() {
        let dir = tempfile::tempdir().unwrap();
        let real = dir.path().join("proj");
        fs::create_dir(&real).unwrap();
        fs::write(real.join("a"), "x").unwrap();
        fs::write(real.join("b"), "x").unwrap();
        let plan = Plan {
            kept: vec![
                ("pending".to_string(), real.display().to_string()),
                ("pending".to_string(), "/does/not/exist".to_string()),
            ],
            skipped: vec![],
        };
        let out = Out {
            json_mode: true,
            interactive: false,
        };
        // Two files plus the kept directory itself; the missing dir is a
        // benign skip, not a failure.
        assert_eq!(
            execute(&plan, 1, &out),
            Renewal {
                files: 2,
                dirs: 1,
                failures: 0
            }
        );
    }

    #[test]
    fn execute_parallel_matches_serial_counts() {
        let dir = tempfile::tempdir().unwrap();
        let mut kept = Vec::new();
        for d in 0..5 {
            let sub = dir.path().join(format!("d{d}"));
            fs::create_dir(&sub).unwrap();
            for f in 0..7 {
                fs::write(sub.join(format!("f{f}")), "x").unwrap();
            }
            kept.push(("pending".to_string(), sub.display().to_string()));
        }
        let plan = Plan {
            kept,
            skipped: vec![],
        };
        let out = Out {
            json_mode: true,
            interactive: false,
        };
        // 5 dirs x 7 files, plus each of the 5 kept dirs themselves.
        assert_eq!(
            execute(&plan, 4, &out),
            Renewal {
                files: 35,
                dirs: 5,
                failures: 0
            }
        );
    }

    #[test]
    fn default_jobs_within_bounds() {
        let n = default_jobs();
        assert!((1..=8).contains(&n));
    }
}
