//! Free disk space, this program's memory and CPU use, for the status bar. Windows and Linux
//! ask the system directly; elsewhere the values are simply not shown.

use std::time::Instant;

#[derive(Default, Clone, Copy, Debug)]
pub struct Snapshot {
    /// Free bytes on the drive holding the current file (or the working folder).
    pub disk_free: Option<u64>,
    /// Working set of this program, bytes.
    pub mem: Option<u64>,
    /// CPU use of this program, percent of the whole processor (all cores), as Task Manager shows.
    pub cpu: Option<f32>,
}

pub struct Monitor {
    last: Option<(Instant, u64)>,
    pub snap: Snapshot,
    at: Option<Instant>,
}

impl Default for Monitor {
    fn default() -> Self {
        Monitor {
            last: None,
            snap: Snapshot::default(),
            at: None,
        }
    }
}

impl Monitor {
    /// Refresh about once a second.
    pub fn tick(&mut self, path: Option<&std::path::Path>) -> Snapshot {
        if self
            .at
            .map(|t| t.elapsed().as_secs_f32() < 1.0)
            .unwrap_or(false)
        {
            return self.snap;
        }
        self.at = Some(Instant::now());
        let dir = path
            .and_then(|p| p.parent().map(|d| d.to_path_buf()))
            .or_else(|| std::env::current_dir().ok());
        self.snap.disk_free = dir.and_then(|d| imp::disk_free(&d));
        self.snap.mem = imp::memory();
        if let Some(cpu_now) = imp::cpu_time_100ns() {
            let now = Instant::now();
            if let Some((t0, c0)) = self.last {
                let wall = now.duration_since(t0).as_secs_f64();
                if wall > 0.0 {
                    let busy = (cpu_now.saturating_sub(c0)) as f64 * 1e-7;
                    // share of all logical processors, like Task Manager's Processes tab
                    let cores = std::thread::available_parallelism()
                        .map(|n| n.get())
                        .unwrap_or(1)
                        .max(1) as f64;
                    self.snap.cpu = Some((busy / wall / cores * 100.0).clamp(0.0, 100.0) as f32);
                }
            }
            self.last = Some((now, cpu_now));
        }
        self.snap
    }
}

/// Memory the program can still allocate (None where it can't be asked).
pub fn available_memory() -> Option<u64> {
    imp::available()
}

#[cfg(windows)]
mod imp {
    use std::os::windows::ffi::OsStrExt;

    #[repr(C)]
    #[derive(Default)]
    struct FileTime {
        low: u32,
        high: u32,
    }

    #[repr(C)]
    #[derive(Default)]
    struct ProcessMemoryCounters {
        cb: u32,
        page_fault_count: u32,
        peak_working_set_size: usize,
        working_set_size: usize,
        quota_peak_paged_pool_usage: usize,
        quota_paged_pool_usage: usize,
        quota_peak_non_paged_pool_usage: usize,
        quota_non_paged_pool_usage: usize,
        pagefile_usage: usize,
        peak_pagefile_usage: usize,
    }

    #[repr(C)]
    #[derive(Default)]
    struct MemoryStatusEx {
        length: u32,
        memory_load: u32,
        total_phys: u64,
        avail_phys: u64,
        total_page_file: u64,
        avail_page_file: u64,
        total_virtual: u64,
        avail_virtual: u64,
        avail_extended_virtual: u64,
    }

    /// Memory the program can still allocate: the system's commit headroom (RAM plus page
    /// file); past it an allocation fails.
    pub fn available() -> Option<u64> {
        let mut m = MemoryStatusEx {
            length: std::mem::size_of::<MemoryStatusEx>() as u32,
            ..Default::default()
        };
        // SAFETY: the struct has the documented layout and its size is set.
        let ok = unsafe { GlobalMemoryStatusEx(&mut m) };
        if ok != 0 {
            Some(m.avail_page_file.min(m.avail_virtual))
        } else {
            None
        }
    }

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GlobalMemoryStatusEx(m: *mut MemoryStatusEx) -> i32;
        fn GetDiskFreeSpaceExW(
            dir: *const u16,
            free_to_caller: *mut u64,
            total: *mut u64,
            total_free: *mut u64,
        ) -> i32;
        fn GetCurrentProcess() -> *mut core::ffi::c_void;
        fn GetProcessTimes(
            p: *mut core::ffi::c_void,
            creation: *mut FileTime,
            exit: *mut FileTime,
            kernel: *mut FileTime,
            user: *mut FileTime,
        ) -> i32;
        fn K32GetProcessMemoryInfo(
            p: *mut core::ffi::c_void,
            counters: *mut ProcessMemoryCounters,
            cb: u32,
        ) -> i32;
    }

    pub fn disk_free(dir: &std::path::Path) -> Option<u64> {
        let wide: Vec<u16> = dir
            .as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        let (mut free, mut total, mut tfree) = (0u64, 0u64, 0u64);
        // SAFETY: the path is NUL-terminated and the out pointers point at live u64s.
        let ok = unsafe { GetDiskFreeSpaceExW(wide.as_ptr(), &mut free, &mut total, &mut tfree) };
        if ok != 0 { Some(free) } else { None }
    }

    pub fn memory() -> Option<u64> {
        let mut c = ProcessMemoryCounters {
            cb: std::mem::size_of::<ProcessMemoryCounters>() as u32,
            ..Default::default()
        };
        // SAFETY: the counters struct has the documented layout and its size is passed in.
        let ok = unsafe { K32GetProcessMemoryInfo(GetCurrentProcess(), &mut c, c.cb) };
        if ok != 0 {
            Some(c.working_set_size as u64)
        } else {
            None
        }
    }

    pub fn cpu_time_100ns() -> Option<u64> {
        let (mut a, mut b, mut k, mut u) = (
            FileTime::default(),
            FileTime::default(),
            FileTime::default(),
            FileTime::default(),
        );
        // SAFETY: four live FILETIME out parameters for the current process.
        let ok = unsafe { GetProcessTimes(GetCurrentProcess(), &mut a, &mut b, &mut k, &mut u) };
        if ok == 0 {
            return None;
        }
        let t = |f: &FileTime| ((f.high as u64) << 32) | f.low as u64;
        Some(t(&k) + t(&u))
    }
}

#[cfg(target_os = "linux")]
mod imp {
    /// Memory the program can still take: available RAM plus free swap.
    pub fn available() -> Option<u64> {
        let text = std::fs::read_to_string("/proc/meminfo").ok()?;
        let kb = |key: &str| -> Option<u64> {
            text.lines()
                .find(|l| l.starts_with(key))?
                .split_whitespace()
                .nth(1)?
                .parse::<u64>()
                .ok()
        };
        Some((kb("MemAvailable:")? + kb("SwapFree:").unwrap_or(0)) * 1024)
    }

    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;

    pub fn disk_free(dir: &std::path::Path) -> Option<u64> {
        let path = CString::new(dir.as_os_str().as_bytes()).ok()?;
        let mut stat: libc::statvfs = unsafe { std::mem::zeroed() };
        // SAFETY: `path` is NUL-terminated and `stat` is a live, correctly typed out parameter.
        if unsafe { libc::statvfs(path.as_ptr(), &mut stat) } != 0 {
            return None;
        }
        if stat.f_bavail > 0 && stat.f_frsize > 0 {
            Some(stat.f_bavail.saturating_mul(stat.f_frsize as u64))
        } else {
            Some(0)
        }
    }

    pub fn memory() -> Option<u64> {
        super::parse_vm_rss(&std::fs::read_to_string("/proc/self/status").ok()?)
    }

    pub fn cpu_time_100ns() -> Option<u64> {
        // /proc/self/stat counts utime and stime in clock ticks, not in seconds.
        let hz = unsafe { libc::sysconf(libc::_SC_CLK_TCK) };
        if hz <= 0 {
            return None;
        }
        super::parse_cpu_ticks(&std::fs::read_to_string("/proc/self/stat").ok()?, hz as u64)
    }
}

#[cfg(all(not(windows), not(target_os = "linux")))]
mod imp {
    pub fn available() -> Option<u64> {
        None
    }

    pub fn disk_free(_dir: &std::path::Path) -> Option<u64> {
        None
    }
    pub fn memory() -> Option<u64> {
        None
    }
    pub fn cpu_time_100ns() -> Option<u64> {
        None
    }
}

/// Resident set size from a `/proc/self/status` snapshot, in bytes.
#[cfg(any(test, target_os = "linux"))]
fn parse_vm_rss(text: &str) -> Option<u64> {
    let line = text.lines().find(|l| l.starts_with("VmRSS:"))?;
    let kb: u64 = line.split_whitespace().nth(1)?.parse().ok()?;
    Some(kb.saturating_mul(1024))
}

/// User + system CPU ticks from a `/proc/self/stat` snapshot, in 100 ns units.
/// The command name in field 2 can contain spaces and parentheses, so split after its last ')'.
#[cfg(any(test, target_os = "linux"))]
fn parse_cpu_ticks(text: &str, ticks_per_sec: u64) -> Option<u64> {
    let after_comm = &text[text.rfind(')')? + 1..];
    let f: Vec<&str> = after_comm.split_whitespace().collect();
    let utime: u64 = f.get(11)?.parse().ok()?;
    let stime: u64 = f.get(12)?.parse().ok()?;
    Some(utime.checked_add(stime)?.saturating_mul(10_000_000) / ticks_per_sec.max(1))
}

#[cfg(test)]
mod tests {
    #[cfg(any(test, target_os = "linux"))]
    #[test]
    fn parses_proc_status_and_stat() {
        let status = "Name:\taudiocraft\nVmPeak:\t 900000 kB\nVmRSS:\t  12345 kB\nThreads:\t4\n";
        assert_eq!(super::parse_vm_rss(status), Some(12345 * 1024));
        assert_eq!(super::parse_vm_rss("Name:\taudiocraft\n"), None);
        let stat = "1234 (my (odd) name) S 1 2 3 4 5 6 7 8 9 10 20 5 0 0";
        assert_eq!(super::parse_cpu_ticks(stat, 100), Some(2_500_000));
        assert_eq!(super::parse_cpu_ticks(stat, 0), Some(250_000_000));
    }

    #[test]
    fn readings_are_sane_when_available() {
        let mut m = super::Monitor::default();
        let s = m.tick(None);
        if let Some(f) = s.disk_free {
            assert!(f > 0);
        }
        if let Some(b) = s.mem {
            assert!(b > 1024 * 1024);
        }
    }
}
