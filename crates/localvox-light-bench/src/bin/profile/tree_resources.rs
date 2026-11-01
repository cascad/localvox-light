//! Sample simultaneous memory separately from per-process high-water marks.
use super::*;
use std::collections::{HashMap, HashSet};

pub fn observe(root: u32, stopped: &AtomicBool) -> Value {
    let mut known = HashSet::from([root]);
    let mut processes = HashMap::new();
    let mut rows = HashMap::<u32, Value>::new();
    let mut peak_working = None::<u64>;
    let mut peak_commit = None::<u64>;
    let mut samples = 0u64;
    let mut partial = 0u64;
    loop {
        let snapshot = snapshot();
        discover(&mut known, &snapshot);
        let live: HashSet<_> = snapshot.iter().map(|(pid, _)| *pid).collect();
        for pid in &known {
            if !processes.contains_key(pid) {
                if let Some(process) = resources::Process::open(*pid) {
                    processes.insert(*pid, process);
                }
            }
        }
        let mut current_working = 0u64;
        let mut current_commit = 0u64;
        let mut readable = 0;
        let mut missing = 0;
        for pid in &known {
            let sample = processes.get(pid).map(|process| process.read());
            if let Some(sample) = &sample {
                let row = rows.entry(*pid).or_insert_with(|| json!({"pid":pid}));
                for field in ["peak_working_set_bytes", "peak_commit_bytes", "cpu_sec"] {
                    if let Some(number) = sample[field].as_f64() {
                        if row[field].as_f64().is_none_or(|old| number >= old) {
                            row[field] = sample[field].clone();
                        }
                    }
                }
            }
            if live.contains(pid) {
                match sample.as_ref().and_then(|s| {
                    Some((
                        s["working_set_bytes"].as_u64()?,
                        s["commit_bytes"].as_u64()?,
                    ))
                }) {
                    Some((working, commit)) => {
                        current_working += working;
                        current_commit += commit;
                        readable += 1;
                    }
                    None => missing += 1,
                }
            }
        }
        if readable > 0 {
            peak_working = Some(peak_working.unwrap_or(0).max(current_working));
            peak_commit = Some(peak_commit.unwrap_or(0).max(current_commit));
        }
        if missing > 0 {
            partial += 1;
        }
        samples += 1;
        if stopped.load(Ordering::Relaxed) {
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    let mut rows: Vec<_> = rows.into_values().collect();
    rows.sort_by_key(|row| row["pid"].as_u64());
    let cpu_complete =
        rows.len() == known.len() && rows.iter().all(|row| row["cpu_sec"].is_number());
    json!({"scope":"coordinator_and_observed_descendants", "sampling_interval_ms":100,
        "availability":if peak_working.is_some() {"observed"} else {"unsupported_or_unavailable"},
        "peak_sampled_working_set_bytes":peak_working,"peak_sampled_commit_bytes":peak_commit,
        "cpu_sec_observed":if cpu_complete {Some(rows.iter().filter_map(|r|r["cpu_sec"].as_f64()).sum::<f64>())} else {None},
        "processes_discovered":known.len(),"processes_queried":rows.len(),"processes":rows,
        "samples":samples,"partial_memory_samples":partial,
        "limits":"short-lived descendants may be missed; simultaneous sampled totals are not sums of individual peaks; external Ollama excluded"})
}

fn discover(known: &mut HashSet<u32>, snapshot: &[(u32, u32)]) {
    loop {
        let before = known.len();
        for (pid, parent) in snapshot {
            if known.contains(parent) {
                known.insert(*pid);
            }
        }
        if before == known.len() {
            break;
        }
    }
}

#[cfg(not(windows))]
fn snapshot() -> Vec<(u32, u32)> {
    Vec::new()
}

#[cfg(windows)]
fn snapshot() -> Vec<(u32, u32)> {
    use windows::Win32::{
        Foundation::CloseHandle,
        System::Diagnostics::ToolHelp::{
            CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
            TH32CS_SNAPPROCESS,
        },
    };
    let mut rows = Vec::new();
    unsafe {
        let Ok(handle) = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) else {
            return rows;
        };
        let mut entry = PROCESSENTRY32W {
            dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
            ..Default::default()
        };
        if Process32FirstW(handle, &mut entry).is_ok() {
            loop {
                rows.push((entry.th32ProcessID, entry.th32ParentProcessID));
                if Process32NextW(handle, &mut entry).is_err() {
                    break;
                }
            }
        }
        let _ = CloseHandle(handle);
    }
    rows
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn discovery_includes_nested_children_without_unrelated_processes() {
        let mut known = HashSet::from([8]);
        discover(&mut known, &[(12, 11), (5, 4), (11, 8), (8, 2), (13, 5)]);
        assert_eq!(known, HashSet::from([8, 11, 12]));
    }
    #[cfg(windows)]
    #[test]
    fn real_supervised_child_tree_has_sampled_totals_and_separate_process_peaks() {
        let mut command = Command::new(std::env::current_exe().unwrap());
        command
            .args(["--ignored", "--exact", "tests::profile_child"])
            .env("LOCALVOX_PROFILE_TEST_CHILD", "tree");
        let result = measure_with_scope(
            command,
            Duration::from_secs(8),
            &AtomicBool::new(false),
            true,
        )
        .unwrap();
        assert_eq!(result["status"], "success");
        let usage = &result["resources"];
        assert!(usage["processes_queried"].as_u64().unwrap() >= 2);
        let peaks: u64 = usage["processes"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|p| p["peak_working_set_bytes"].as_u64())
            .sum();
        assert!(usage["peak_sampled_working_set_bytes"].as_u64().unwrap() <= peaks);
        assert!(usage["peak_sampled_working_set_bytes"].as_u64().unwrap() >= 32 * 1024 * 1024);
        assert!(usage["cpu_sec_observed"].as_f64().unwrap() >= 0.0);
    }
    #[cfg(windows)]
    #[test]
    fn sampler_retains_real_root_counters_without_fabricating_tree_peaks() {
        let result = observe(std::process::id(), &AtomicBool::new(true));
        assert_eq!(result["availability"], "observed");
        assert!(result["peak_sampled_working_set_bytes"].as_u64().unwrap() > 0);
        assert!(result["cpu_sec_observed"].as_f64().unwrap() >= 0.0);
        assert_eq!(result["scope"], "coordinator_and_observed_descendants");
        assert!(result["processes_discovered"].as_u64().unwrap() >= 1);
    }
}
