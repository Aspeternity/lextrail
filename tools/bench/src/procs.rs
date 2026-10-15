//! Process tree, memory and CPU time of the app and its WebView2 children.

use std::collections::BTreeSet;
use std::time::Duration;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessInfo {
    pub pid: u32,
    pub parent: u32,
    pub name: String,
}

/// `root` plus every process descended from it.
pub fn tree(processes: &[ProcessInfo], root: u32) -> BTreeSet<u32> {
    let mut found = BTreeSet::from([root]);
    // Repeat until no new children appear; process lists are small.
    loop {
        let before = found.len();
        for p in processes {
            // PIDs can be reused: a parent id pointing at itself or at 0 is not a child.
            if p.pid != p.parent && found.contains(&p.parent) {
                found.insert(p.pid);
            }
        }
        if found.len() == before {
            return found;
        }
    }
}

/// Memory of one process or a sum over several, in bytes.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Memory {
    /// Private working set: what Task Manager's "Memory" column shows. Primary metric.
    pub private_working_set: u64,
    /// Full working set, including pages shared with other processes (double-counts
    /// shared pages when summed).
    pub working_set: u64,
    /// Private commit (private bytes), including memory paged out to disk.
    pub private_commit: u64,
}

impl std::ops::Add for Memory {
    type Output = Self;

    fn add(self, other: Self) -> Self {
        Self {
            private_working_set: self.private_working_set + other.private_working_set,
            working_set: self.working_set + other.working_set,
            private_commit: self.private_commit + other.private_commit,
        }
    }
}

impl std::iter::Sum for Memory {
    fn sum<I: Iterator<Item = Self>>(iter: I) -> Self {
        iter.fold(Self::default(), |a, b| a + b)
    }
}

pub fn mib(bytes: u64) -> f64 {
    bytes as f64 / (1024.0 * 1024.0)
}

/// A measured process tree.
#[derive(Debug, Clone, Default)]
pub struct TreeSample {
    pub processes: Vec<(ProcessInfo, Memory)>,
}

impl TreeSample {
    pub fn total(&self) -> Memory {
        self.processes.iter().map(|(_, m)| *m).sum()
    }

    pub fn count(&self) -> usize {
        self.processes.len()
    }
}

#[cfg(windows)]
pub use windows_impl::{cpu_time, list_processes, memory};

#[cfg(windows)]
mod windows_impl {
    use std::mem::size_of;
    use std::time::Duration;

    use anyhow::{Context, Result};
    use windows::Win32::Foundation::{CloseHandle, FILETIME, HANDLE};
    use windows::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW,
        TH32CS_SNAPPROCESS,
    };
    use windows::Win32::System::ProcessStatus::{
        GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS, PROCESS_MEMORY_COUNTERS_EX2,
    };
    use windows::Win32::System::Threading::{
        GetProcessTimes, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
    };

    use super::{Memory, ProcessInfo};

    pub fn list_processes() -> Result<Vec<ProcessInfo>> {
        // SAFETY: standard ToolHelp snapshot iteration; the handle is closed below.
        unsafe {
            let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0)?;
            let mut entry = PROCESSENTRY32W {
                dwSize: size_of::<PROCESSENTRY32W>() as u32,
                ..Default::default()
            };
            let mut list = Vec::new();
            let mut ok = Process32FirstW(snapshot, &mut entry).is_ok();
            while ok {
                let len = entry
                    .szExeFile
                    .iter()
                    .position(|&c| c == 0)
                    .unwrap_or(entry.szExeFile.len());
                list.push(ProcessInfo {
                    pid: entry.th32ProcessID,
                    parent: entry.th32ParentProcessID,
                    name: String::from_utf16_lossy(&entry.szExeFile[..len]),
                });
                ok = Process32NextW(snapshot, &mut entry).is_ok();
            }
            let _ = CloseHandle(snapshot);
            Ok(list)
        }
    }

    struct Process(HANDLE);

    impl Process {
        fn open(pid: u32) -> Result<Self> {
            // SAFETY: plain OpenProcess; the handle is closed on drop.
            let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) }
                .with_context(|| format!("cannot open process {pid}"))?;
            Ok(Self(handle))
        }
    }

    impl Drop for Process {
        fn drop(&mut self) {
            // SAFETY: the handle came from OpenProcess and is closed once.
            let _ = unsafe { CloseHandle(self.0) };
        }
    }

    pub fn memory(pid: u32) -> Result<Memory> {
        let process = Process::open(pid)?;
        let mut counters = PROCESS_MEMORY_COUNTERS_EX2 {
            cb: size_of::<PROCESS_MEMORY_COUNTERS_EX2>() as u32,
            ..Default::default()
        };
        // SAFETY: `counters` is a correctly sized EX2 struct, passed as its base type.
        unsafe {
            GetProcessMemoryInfo(
                process.0,
                &mut counters as *mut _ as *mut PROCESS_MEMORY_COUNTERS,
                counters.cb,
            )
        }
        .with_context(|| format!("cannot read memory of process {pid}"))?;
        Ok(Memory {
            private_working_set: counters.PrivateWorkingSetSize as u64,
            working_set: counters.WorkingSetSize as u64,
            private_commit: counters.PrivateUsage as u64,
        })
    }

    /// Kernel + user CPU time consumed so far.
    pub fn cpu_time(pid: u32) -> Result<Duration> {
        let process = Process::open(pid)?;
        let (mut created, mut exited, mut kernel, mut user) = Default::default();
        // SAFETY: all four out-pointers are valid FILETIMEs.
        unsafe { GetProcessTimes(process.0, &mut created, &mut exited, &mut kernel, &mut user) }
            .with_context(|| format!("cannot read CPU time of process {pid}"))?;
        Ok(filetime(kernel) + filetime(user))
    }

    fn filetime(t: FILETIME) -> Duration {
        let ticks = (u64::from(t.dwHighDateTime) << 32) | u64::from(t.dwLowDateTime);
        Duration::from_nanos(ticks * 100)
    }
}

/// Measures memory of the whole tree rooted at `root`. Processes that exit while
/// being measured are skipped.
#[cfg(windows)]
pub fn sample_tree(root: u32) -> anyhow::Result<TreeSample> {
    let all = list_processes()?;
    let ids = tree(&all, root);
    let processes = all
        .into_iter()
        .filter(|p| ids.contains(&p.pid))
        .filter_map(|p| memory(p.pid).ok().map(|m| (p, m)))
        .collect();
    Ok(TreeSample { processes })
}

/// Total CPU time of the tree rooted at `root`, keyed by pid so a later sample can be
/// diffed against processes that still exist.
#[cfg(windows)]
pub fn tree_cpu(root: u32) -> anyhow::Result<std::collections::BTreeMap<u32, Duration>> {
    let all = list_processes()?;
    Ok(tree(&all, root)
        .into_iter()
        .filter_map(|pid| cpu_time(pid).ok().map(|t| (pid, t)))
        .collect())
}

/// CPU consumed between two `tree_cpu` samples by processes present in both.
pub fn cpu_delta(
    before: &std::collections::BTreeMap<u32, Duration>,
    after: &std::collections::BTreeMap<u32, Duration>,
) -> Duration {
    after
        .iter()
        .filter_map(|(pid, t)| before.get(pid).map(|b| t.saturating_sub(*b)))
        .sum()
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;

    fn p(pid: u32, parent: u32, name: &str) -> ProcessInfo {
        ProcessInfo {
            pid,
            parent,
            name: name.into(),
        }
    }

    #[test]
    fn tree_includes_grandchildren_in_any_order() {
        let list = vec![
            p(30, 20, "renderer"), // listed before its parent
            p(10, 1, "app"),
            p(20, 10, "msedgewebview2 browser"),
            p(21, 20, "gpu"),
            p(99, 1, "unrelated"),
        ];
        assert_eq!(tree(&list, 10), BTreeSet::from([10, 20, 21, 30]));
    }

    #[test]
    fn tree_of_lonely_process_is_itself() {
        assert_eq!(tree(&[p(5, 1, "x")], 5), BTreeSet::from([5]));
    }

    #[test]
    fn self_parented_entries_do_not_loop() {
        let list = vec![p(10, 1, "app"), p(0, 0, "idle"), p(4, 0, "system")];
        assert_eq!(tree(&list, 0), BTreeSet::from([0, 4]));
    }

    #[test]
    fn memory_sums() {
        let a = Memory {
            private_working_set: 1,
            working_set: 10,
            private_commit: 100,
        };
        let total: Memory = [a, a, a].into_iter().sum();
        assert_eq!(
            total,
            Memory {
                private_working_set: 3,
                working_set: 30,
                private_commit: 300
            }
        );
    }

    #[test]
    fn cpu_delta_ignores_processes_that_came_or_went() {
        let ms = Duration::from_millis;
        let before = BTreeMap::from([(1, ms(100)), (2, ms(50))]);
        let after = BTreeMap::from([(1, ms(130)), (3, ms(999))]);
        assert_eq!(cpu_delta(&before, &after), ms(30));
    }

    #[test]
    fn mib_conversion() {
        assert_eq!(mib(3 * 1024 * 1024), 3.0);
    }
}
