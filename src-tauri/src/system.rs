//! The gauges: how busy the processor is, how full the memory is and, when the computer has a
//! graphics card that can be read, how full its video memory is.

use serde::Serialize;
use std::sync::Mutex;
use sysinfo::System;

#[derive(Debug, Clone, Copy, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Meters {
    /// Percent of processor in use, 0 to 100.
    pub cpu: u8,
    /// Percent of memory in use, 0 to 100.
    pub memory: u8,
    /// Percent of the graphics card's own memory in use, 0 to 100. None when it cannot be read,
    /// and the page then shows no VRAM gauge at all rather than a wrong one.
    pub vram: Option<u8>,
    /// Bytes of video memory in use and in all, for the "3.1 / 8 GB" line under the gauge.
    pub vram_used: Option<u64>,
    pub vram_total: Option<u64>,
}

/// The graphics card's instance in the "GPU Adapter Memory" counters starts with its LUID, high
/// part first: `luid_0x00000000_0x0000D1A3_phys_0`. One card can have several instances.
pub fn luid_prefix(high: i32, low: u32) -> String {
    format!("luid_0x{:08x}_0x{:08x}", high as u32, low)
}

/// Adds up the dedicated memory in use across the instances that belong to one card.
pub fn vram_in_use<'a>(prefix: &str, readings: impl IntoIterator<Item = (&'a str, i64)>) -> Option<u64> {
    let mut found = false;
    let mut sum: u64 = 0;
    for (name, bytes) in readings {
        if name.to_ascii_lowercase().starts_with(prefix) && bytes >= 0 {
            found = true;
            sum = sum.saturating_add(bytes as u64);
        }
    }
    found.then_some(sum)
}

/// Reads the graphics card with the most dedicated memory (the real GPU on a laptop that also has
/// one built into the processor). Software adapters are skipped.
#[cfg(windows)]
mod gpu {
    use windows::core::{w, PCWSTR};
    use windows::Win32::Graphics::Dxgi::{CreateDXGIFactory1, IDXGIFactory1, DXGI_ADAPTER_FLAG_SOFTWARE};
    use windows::Win32::System::Performance::{
        PdhAddEnglishCounterW, PdhCloseQuery, PdhCollectQueryData, PdhGetFormattedCounterArrayW, PdhOpenQueryW,
        PDH_FMT_COUNTERVALUE_ITEM_W, PDH_FMT_LARGE, PDH_HCOUNTER, PDH_HQUERY, PDH_MORE_DATA,
    };

    pub struct Gpu {
        query: PDH_HQUERY,
        counter: PDH_HCOUNTER,
        prefix: String,
        pub total: u64,
    }

    // SAFETY: the PDH handles are only touched while the meters' Mutex is held, one thread at a time.
    unsafe impl Send for Gpu {}

    impl Drop for Gpu {
        fn drop(&mut self) {
            // SAFETY: the query was opened by PdhOpenQueryW and is closed exactly once.
            unsafe { PdhCloseQuery(self.query) };
        }
    }

    impl Gpu {
        pub fn open() -> Option<Self> {
            // SAFETY: plain DXGI calls; every result is checked before use.
            let (high, low, total) = unsafe {
                let factory: IDXGIFactory1 = CreateDXGIFactory1().ok()?;
                let mut best: Option<(i32, u32, u64)> = None;
                let mut i = 0;
                while let Ok(adapter) = factory.EnumAdapters1(i) {
                    i += 1;
                    let Ok(desc) = adapter.GetDesc1() else { continue };
                    if desc.Flags & (DXGI_ADAPTER_FLAG_SOFTWARE.0 as u32) != 0 {
                        continue;
                    }
                    let total = desc.DedicatedVideoMemory as u64;
                    if total > 0 && best.is_none_or(|(_, _, b)| total > b) {
                        best = Some((desc.AdapterLuid.HighPart, desc.AdapterLuid.LowPart, total));
                    }
                }
                best?
            };
            let mut query = PDH_HQUERY::default();
            let mut counter = PDH_HCOUNTER::default();
            // SAFETY: both out-pointers are live locals; the query is closed if the counter fails.
            unsafe {
                if PdhOpenQueryW(PCWSTR::null(), 0, &mut query) != 0 {
                    return None;
                }
                if PdhAddEnglishCounterW(query, w!("\\GPU Adapter Memory(*)\\Dedicated Usage"), 0, &mut counter) != 0 {
                    PdhCloseQuery(query);
                    return None;
                }
                PdhCollectQueryData(query);
            }
            Some(Gpu { query, counter, prefix: super::luid_prefix(high, low), total })
        }

        /// Bytes of the card's dedicated memory in use now.
        pub fn used(&self) -> Option<u64> {
            // SAFETY: the buffer is sized by PDH's own first answer and read only up to `count` items.
            unsafe {
                if PdhCollectQueryData(self.query) != 0 {
                    return None;
                }
                let (mut size, mut count) = (0u32, 0u32);
                if PdhGetFormattedCounterArrayW(self.counter, PDH_FMT_LARGE, &mut size, &mut count, None) != PDH_MORE_DATA {
                    return None;
                }
                let items = (size as usize).div_ceil(std::mem::size_of::<PDH_FMT_COUNTERVALUE_ITEM_W>());
                let mut buffer = vec![PDH_FMT_COUNTERVALUE_ITEM_W::default(); items.max(1)];
                if PdhGetFormattedCounterArrayW(self.counter, PDH_FMT_LARGE, &mut size, &mut count, Some(buffer.as_mut_ptr())) != 0 {
                    return None;
                }
                let count = (count as usize).min(buffer.len());
                let readings: Vec<(String, i64)> = buffer[..count]
                    .iter()
                    .filter(|item| item.FmtValue.CStatus == 0)
                    .map(|item| (item.szName.to_string().unwrap_or_default(), item.FmtValue.Anonymous.largeValue))
                    .collect();
                super::vram_in_use(&self.prefix, readings.iter().map(|(n, b)| (n.as_str(), *b)))
            }
        }
    }
}

#[cfg(not(windows))]
mod gpu {
    pub struct Gpu {
        pub total: u64,
    }
    impl Gpu {
        pub fn open() -> Option<Self> {
            None
        }
        pub fn used(&self) -> Option<u64> {
            None
        }
    }
}

/// Percent, rounded and kept within 0..=100.
pub fn percent(part: f64, whole: f64) -> u8 {
    if whole <= 0.0 || !part.is_finite() || !whole.is_finite() {
        return 0;
    }
    (part / whole * 100.0).round().clamp(0.0, 100.0) as u8
}

/// Processor time the whole computer has spent since it started, in 100-nanosecond ticks.
/// On Windows the kernel figure already includes the idle time.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CpuTimes {
    pub idle: u64,
    pub kernel: u64,
    pub user: u64,
}

/// How busy the processor was between two readings. None when no time has passed or the
/// counters went backwards, so the caller keeps its last answer instead of showing a wrong one.
pub fn busy_percent(before: CpuTimes, after: CpuTimes) -> Option<u8> {
    let idle = after.idle.checked_sub(before.idle)?;
    let total = after.kernel.checked_sub(before.kernel)? + after.user.checked_sub(before.user)?;
    if total == 0 {
        return None;
    }
    Some(percent(total.saturating_sub(idle) as f64, total as f64))
}

/// Reads processor time straight from the Windows kernel. Unlike sysinfo's CPU figure, this does
/// not use Windows performance counters, which are missing or damaged on some computers. There
/// sysinfo quietly reports 0% while Task Manager still works.
#[cfg(windows)]
pub(crate) fn cpu_times() -> Option<CpuTimes> {
    use windows_sys::Win32::Foundation::FILETIME;
    use windows_sys::Win32::System::Threading::GetSystemTimes;
    let ticks = |t: FILETIME| (u64::from(t.dwHighDateTime) << 32) | u64::from(t.dwLowDateTime);
    let zero = FILETIME { dwLowDateTime: 0, dwHighDateTime: 0 };
    let (mut idle, mut kernel, mut user) = (zero, zero, zero);
    // SAFETY: the three pointers are to live, writable FILETIME values for the whole call.
    let ok = unsafe { GetSystemTimes(&mut idle, &mut kernel, &mut user) };
    (ok != 0).then(|| CpuTimes { idle: ticks(idle), kernel: ticks(kernel), user: ticks(user) })
}

#[cfg(not(windows))]
pub(crate) fn cpu_times() -> Option<CpuTimes> {
    None
}

struct State {
    system: System,
    last_times: Option<CpuTimes>,
    last_cpu: u8,
    gpu: Option<gpu::Gpu>,
}

/// Processor use needs two readings apart in time, so the state lives for the whole run.
/// When the system cannot be read at all, the gauges show zero and Castle carries on.
pub struct SystemMeters(Mutex<Option<State>>);

impl Default for SystemMeters {
    fn default() -> Self {
        let state = System::new().ok().map(|system| State { system, last_times: cpu_times(), last_cpu: 0, gpu: gpu::Gpu::open() });
        Self(Mutex::new(state))
    }
}

impl SystemMeters {
    pub fn read(&self) -> Meters {
        let mut guard = self.0.lock().unwrap();
        let Some(state) = guard.as_mut() else {
            return Meters { cpu: 0, memory: 0, vram: None, vram_used: None, vram_total: None };
        };
        state.system.refresh_memory();
        let cpu = match cpu_times() {
            // Windows: the kernel's own processor times, no performance counters needed.
            Some(now) => {
                if let Some(busy) = state.last_times.and_then(|before| busy_percent(before, now)) {
                    state.last_cpu = busy;
                }
                state.last_times = Some(now);
                state.last_cpu
            }
            None => {
                state.system.refresh_cpu_usage();
                percent(state.system.global_cpu_usage() as f64, 100.0)
            }
        };
        let total = state.gpu.as_ref().map(|g| g.total);
        let used = state.gpu.as_ref().and_then(|g| g.used());
        Meters {
            cpu,
            memory: percent(state.system.used_memory() as f64, state.system.total_memory() as f64),
            vram: used.zip(total).map(|(u, t)| percent(u as f64, t as f64)),
            vram_used: used,
            vram_total: used.and(total),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percent_is_rounded_and_kept_in_range() {
        assert_eq!(percent(1.0, 4.0), 25);
        assert_eq!(percent(2.0, 3.0), 67);
        assert_eq!(percent(5.0, 4.0), 100);
        assert_eq!(percent(1.0, 0.0), 0);
        assert_eq!(percent(f64::NAN, 1.0), 0);
    }

    #[test]
    fn luid_prefix_matches_the_counter_instance_names() {
        assert_eq!(luid_prefix(0, 0xD1A3), "luid_0x00000000_0x0000d1a3");
        assert_eq!(luid_prefix(-1, 1), "luid_0xffffffff_0x00000001");
    }

    #[test]
    fn vram_in_use_adds_only_the_chosen_card() {
        let readings = [
            ("luid_0x00000000_0x0000D1A3_phys_0", 3_000),
            ("luid_0x00000000_0x0000D1A3_phys_1", 500),
            ("luid_0x00000000_0x0000AAAA_phys_0", 9_999),
        ];
        assert_eq!(vram_in_use("luid_0x00000000_0x0000d1a3", readings), Some(3_500));
        assert_eq!(vram_in_use("luid_0x00000000_0x0000bbbb", readings), None);
    }

    #[cfg(windows)]
    #[test]
    #[ignore = "needs a graphics card; run by hand: cargo test vram_reads -- --ignored --nocapture"]
    fn vram_reads_on_this_computer() {
        let meters = SystemMeters::default();
        let m = meters.read();
        println!("vram {:?}% used {:?} total {:?}", m.vram, m.vram_used, m.vram_total);
        assert!(m.vram.is_some());
    }

    #[test]
    fn this_computer_reports_some_memory_in_use() {
        let meters = SystemMeters::default();
        let first = meters.read();
        assert!(first.memory > 0 && first.memory <= 100);
        assert!(first.cpu <= 100);
    }

    fn times(idle: u64, kernel: u64, user: u64) -> CpuTimes {
        CpuTimes { idle, kernel, user }
    }

    #[test]
    fn busy_percent_counts_kernel_and_user_time_minus_idle() {
        // 1000 ticks passed: 600 kernel (of which 500 idle) + 400 user, so 500 of 1000 were busy.
        assert_eq!(busy_percent(times(0, 0, 0), times(500, 600, 400)), Some(50));
        assert_eq!(busy_percent(times(100, 100, 0), times(1100, 1100, 0)), Some(0));
        assert_eq!(busy_percent(times(0, 0, 0), times(0, 300, 700)), Some(100));
    }

    #[test]
    fn busy_percent_refuses_readings_it_cannot_trust() {
        assert_eq!(busy_percent(times(5, 5, 5), times(5, 5, 5)), None);
        assert_eq!(busy_percent(times(9, 9, 9), times(1, 1, 1)), None);
    }

    #[cfg(windows)]
    #[test]
    fn windows_reports_processor_time_without_performance_counters() {
        let first = cpu_times().expect("GetSystemTimes answers");
        let mut spin = 0u64;
        for i in 0..20_000_000u64 {
            spin = spin.wrapping_add(i ^ (spin >> 3));
        }
        std::hint::black_box(spin);
        let second = cpu_times().expect("GetSystemTimes answers");
        assert!(second.kernel + second.user > first.kernel + first.user);
        assert!(busy_percent(first, second).is_some());
    }
}
