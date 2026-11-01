use serde_json::{json, Value};

pub fn observe(pid: u32, stopped: &std::sync::atomic::AtomicBool) -> Value {
    use std::sync::atomic::Ordering;
    let Some(process) = Process::open(pid) else {
        return unavailable();
    };
    let mut result = unavailable();
    let mut samples = 0;
    loop {
        let sample = process.read();
        for key in ["peak_working_set_bytes", "peak_commit_bytes", "cpu_sec"] {
            if let Some(number) = sample[key].as_f64() {
                // A queried CPU counter may legitimately be zero. Missing is a different value.
                if (number > 0.0 || key == "cpu_sec")
                    && result[key].as_f64().is_none_or(|old| number >= old)
                {
                    result[key] = sample[key].clone();
                }
            }
        }
        samples += 1;
        if stopped.load(Ordering::Relaxed) {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    result["availability"] = json!(if result["peak_working_set_bytes"].is_number() {
        "observed"
    } else {
        "unavailable"
    });
    result["samples"] = json!(samples);
    result["sampling_interval_ms"] = json!(20);
    result["peak_method"] =
        json!("maximum observed OS high-water mark; final retained-handle query");
    result
}

pub fn unavailable() -> Value {
    json!({"scope": "worker_process_only", "peak_working_set_bytes": null,
        "peak_commit_bytes": null, "cpu_sec": null,
        "availability": "unsupported_platform_or_process_query_unavailable"})
}

#[cfg(not(windows))]
pub struct Process;
#[cfg(not(windows))]
impl Process {
    pub fn open(_: u32) -> Option<Self> {
        None
    }
    pub fn read(&self) -> Value {
        unavailable()
    }
}

#[cfg(windows)]
pub struct Process(windows::Win32::Foundation::HANDLE);

#[cfg(windows)]
impl Process {
    pub fn open(pid: u32) -> Option<Self> {
        use windows::Win32::System::Threading::{
            OpenProcess, PROCESS_QUERY_INFORMATION, PROCESS_VM_READ,
        };
        unsafe {
            OpenProcess(PROCESS_QUERY_INFORMATION | PROCESS_VM_READ, false, pid)
                .ok()
                .map(Self)
        }
    }

    pub fn read(&self) -> Value {
        use windows::Win32::{
            Foundation::FILETIME,
            System::{
                ProcessStatus::{GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS_EX},
                Threading::GetProcessTimes,
            },
        };
        // The retained handle remains valid after the supervised child is reaped.
        let mut counters = PROCESS_MEMORY_COUNTERS_EX::default();
        let size = std::mem::size_of_val(&counters) as u32;
        counters.cb = size;
        let memory = unsafe {
            GetProcessMemoryInfo(
                self.0,
                (&mut counters as *mut PROCESS_MEMORY_COUNTERS_EX).cast(),
                size,
            )
        }
        .is_ok();
        let (mut created, mut exited, mut kernel, mut user) = (
            FILETIME::default(),
            FILETIME::default(),
            FILETIME::default(),
            FILETIME::default(),
        );
        let times =
            unsafe { GetProcessTimes(self.0, &mut created, &mut exited, &mut kernel, &mut user) }
                .is_ok();
        let seconds = |time: FILETIME| {
            ((u64::from(time.dwHighDateTime) << 32) | u64::from(time.dwLowDateTime)) as f64
                / 10_000_000.0
        };
        json!({"scope": "worker_process_only", "availability": if memory && times { "available" } else { "partial_or_unavailable" },
            "working_set_bytes": memory.then_some(counters.WorkingSetSize),
            "commit_bytes": memory.then_some(counters.PrivateUsage),
            "peak_working_set_bytes": memory.then_some(counters.PeakWorkingSetSize),
            "peak_commit_bytes": memory.then_some(counters.PeakPagefileUsage),
            "cpu_sec": times.then(|| seconds(kernel) + seconds(user))})
    }
}

#[cfg(windows)]
impl Drop for Process {
    fn drop(&mut self) {
        let _ = unsafe { windows::Win32::Foundation::CloseHandle(self.0) };
    }
}
