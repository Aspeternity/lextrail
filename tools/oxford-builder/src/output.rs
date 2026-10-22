//! Writing an output file without ever leaving a half-written one
//! (`docs/specs/m1.5-oxford.md` 3): the data goes to a temporary file in the output folder,
//! `VACUUM INTO` writes the finished database to a second temporary file, and once every
//! output file is finished they replace their targets. Temporary files are removed on any
//! failure.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, bail};
use rusqlite::Connection;

/// Removes its files when dropped (unless [`TempFiles::keep`] took them out).
#[derive(Default)]
pub struct TempFiles(Vec<PathBuf>);

impl TempFiles {
    /// A new temporary file name next to `target`: `<name>.tmp-<random>`.
    pub fn next(&mut self, target: &Path) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos());
        let name = format!(
            "{}.tmp-{:x}{:x}{}",
            file_name(target),
            std::process::id(),
            nanos,
            self.0.len()
        );
        let path = target.with_file_name(name);
        self.0.push(path.clone());
        path
    }

    fn keep(&mut self, path: &Path) {
        self.0.retain(|p| p != path);
    }
}

impl Drop for TempFiles {
    fn drop(&mut self) {
        for path in &self.0 {
            remove_with_journal(path);
        }
    }
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

fn remove_with_journal(path: &Path) {
    let _ = fs::remove_file(path);
    for suffix in ["-journal", "-wal", "-shm"] {
        let mut p = path.as_os_str().to_owned();
        p.push(suffix);
        let _ = fs::remove_file(PathBuf::from(p));
    }
}

/// Removes temporary files of `target` left by an earlier run that was killed.
pub fn remove_stale(target: &Path) -> Result<()> {
    let Some(dir) = target.parent() else {
        return Ok(());
    };
    let prefix = format!("{}.tmp-", file_name(target));
    for item in fs::read_dir(dir).with_context(|| format!("reading {}", dir.display()))? {
        let item = item?;
        if item.file_name().to_string_lossy().starts_with(&prefix) {
            let _ = fs::remove_file(item.path());
        }
    }
    Ok(())
}

/// Stops when the target exists and is in use (LexTrail has it open), before anything is
/// written.
pub fn ensure_replaceable(target: &Path) -> Result<()> {
    if !target.exists() {
        return Ok(());
    }
    if in_use(target) {
        bail!(
            "{} is in use: LexTrail is using this file. Quit LexTrail or turn the Oxford \
             dictionary off in its settings, then run the converter again.",
            target.display()
        );
    }
    Ok(())
}

#[cfg(windows)]
fn in_use(target: &Path) -> bool {
    use std::os::windows::fs::OpenOptionsExt;
    // No sharing: fails while any other process has the file open.
    fs::OpenOptions::new()
        .read(true)
        .share_mode(0)
        .open(target)
        .is_err()
}

#[cfg(not(windows))]
fn in_use(_target: &Path) -> bool {
    false
}

/// Writes the finished database of `conn` to a second temporary file with `VACUUM INTO`,
/// closes `conn`, and returns that file (still a temporary file: [`replace`] moves it over
/// the target).
pub fn compact(conn: Connection, temps: &mut TempFiles, target: &Path) -> Result<PathBuf> {
    let compact = temps.next(target);
    conn.execute("VACUUM INTO ?1", [compact.to_string_lossy()])
        .with_context(|| format!("writing {}", compact.display()))?;
    conn.close().map_err(|(_, e)| e)?;
    Ok(compact)
}

/// Moves finished files (from [`compact`]) over their targets, after checking that no
/// target is in use, so that either all targets are replaced or (when one is in use) none.
pub fn replace(temps: &mut TempFiles, files: &[(PathBuf, &Path)]) -> Result<()> {
    for (_, target) in files {
        ensure_replaceable(target)?;
    }
    for (compact, target) in files {
        fs::rename(compact, target).with_context(|| format!("replacing {}", target.display()))?;
        temps.keep(compact);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn temp_files_are_removed_when_dropped() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("x.db");
        let path = {
            let mut temps = TempFiles::default();
            let path = temps.next(&target);
            fs::write(&path, b"partial").unwrap();
            path
        };
        assert!(!path.exists());
        assert!(!target.exists());
    }

    #[test]
    fn replace_moves_the_compacted_file_and_leaves_no_temp_files() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("x.db");
        fs::write(&target, b"old").unwrap();
        let mut temps = TempFiles::default();
        let work = temps.next(&target);
        let conn = Connection::open(&work).unwrap();
        conn.execute_batch("CREATE TABLE t (v INTEGER); INSERT INTO t VALUES (7);")
            .unwrap();
        let compact = compact(conn, &mut temps, &target).unwrap();
        replace(&mut temps, &[(compact, &target)]).unwrap();
        drop(temps);
        let v: i64 = Connection::open(&target)
            .unwrap()
            .query_row("SELECT v FROM t", [], |r| r.get(0))
            .unwrap();
        assert_eq!(v, 7);
        let names: Vec<_> = fs::read_dir(dir.path())
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(names, ["x.db"]);
    }

    #[test]
    fn stale_temp_files_are_removed() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("x.db");
        fs::write(dir.path().join("x.db.tmp-abc"), b"").unwrap();
        fs::write(dir.path().join("y.db.tmp-abc"), b"").unwrap();
        remove_stale(&target).unwrap();
        assert!(!dir.path().join("x.db.tmp-abc").exists());
        assert!(dir.path().join("y.db.tmp-abc").exists());
    }

    #[cfg(windows)]
    #[test]
    fn a_file_open_elsewhere_is_not_replaced() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("x.db");
        fs::write(&target, b"old").unwrap();
        let open = fs::File::open(&target).unwrap();
        assert!(ensure_replaceable(&target).is_err());
        drop(open);
        ensure_replaceable(&target).unwrap();
    }
}
