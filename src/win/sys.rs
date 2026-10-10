//! Windows implementations of the platform services (see platform/mod.rs).

use super::ffi::*;
use crate::platform::{CpuTimes, Memory};
use crate::util::LocalTime;
use std::collections::HashMap;
use std::ffi::c_void;
use std::sync::Mutex;

pub use super::gdiplus::{font_families, new_canvas, open_image};
pub use super::usb::open_usb;

pub fn cpu_times() -> Option<CpuTimes> {
    let (mut idle, mut kernel, mut user) = (FILETIME::default(), FILETIME::default(), FILETIME::default());
    if unsafe { GetSystemTimes(&mut idle, &mut kernel, &mut user) } == 0 {
        return None;
    }
    // kernel time includes idle time
    Some(CpuTimes { idle: idle.get(), total: kernel.get() + user.get() })
}

/// Current clock of CPU 0 in MHz (often reported as the base clock).
pub fn cpu_mhz() -> Option<f64> {
    #[repr(C)]
    #[derive(Clone, Copy, Default)]
    struct ProcessorPowerInformation {
        number: u32,
        max_mhz: u32,
        current_mhz: u32,
        mhz_limit: u32,
        max_idle_state: u32,
        current_idle_state: u32,
    }
    let n = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1).max(1);
    let mut buf = vec![ProcessorPowerInformation::default(); n * 2];
    let len = (buf.len() * std::mem::size_of::<ProcessorPowerInformation>()) as u32;
    let status = unsafe { CallNtPowerInformation(11, std::ptr::null_mut(), 0, buf.as_mut_ptr() as *mut c_void, len) };
    if status != 0 || buf[0].current_mhz == 0 {
        return None;
    }
    Some(buf[0].current_mhz as f64)
}

pub fn memory() -> Option<Memory> {
    let mut m = MEMORYSTATUSEX { dwLength: std::mem::size_of::<MEMORYSTATUSEX>() as u32, ..Default::default() };
    if unsafe { GlobalMemoryStatusEx(&mut m) } == 0 {
        return None;
    }
    Some(Memory { total: m.ullTotalPhys, used: m.ullTotalPhys.saturating_sub(m.ullAvailPhys) })
}

/// Percent used of the Windows drive.
pub fn system_disk_usage() -> Option<f64> {
    let drive = std::env::var("SystemDrive").unwrap_or_else(|_| "C:".into());
    let root = wide(&format!("{}\\", drive));
    let (mut avail, mut total, mut free) = (0u64, 0u64, 0u64);
    if unsafe { GetDiskFreeSpaceExW(root.as_ptr(), &mut avail, &mut total, &mut free) } == 0 || total == 0 {
        return None;
    }
    Some((total - free) as f64 * 100.0 / total as f64)
}

/// Totals of bytes received/sent on all network adapters. The OS counters
/// here are 32-bit, so deltas are accumulated into 64-bit totals.
pub fn net_bytes() -> Option<(u64, u64)> {
    static STATE: Mutex<Option<(HashMap<u32, (u32, u32)>, u64, u64)>> = Mutex::new(None);
    let mut size = 0u32;
    unsafe { GetIfTable(std::ptr::null_mut(), &mut size, 0) };
    if size == 0 {
        return None;
    }
    let mut buf = vec![0u32; size as usize / 4 + 1];
    if unsafe { GetIfTable(buf.as_mut_ptr() as *mut u8, &mut size, 0) } != 0 {
        return None;
    }
    let bytes = unsafe { std::slice::from_raw_parts(buf.as_ptr() as *const u8, buf.len() * 4) };
    let rd = |o: usize| u32::from_le_bytes([bytes[o], bytes[o + 1], bytes[o + 2], bytes[o + 3]]);
    let count = rd(0) as usize;
    const ROW: usize = 860;
    let mut rows: Vec<(u32, u32, u32)> = Vec::new();
    for i in 0..count {
        let base = 4 + i * ROW;
        if base + ROW > bytes.len() {
            break;
        }
        let index = rd(base + 512);
        let kind = rd(base + 516);
        let oper = rd(base + 544);
        let (rx, tx) = (rd(base + 552), rd(base + 576));
        // skip loopback and adapters that aren't up; filter drivers show up as
        // extra rows with the same counters, so count each pair of counters once
        if kind == 24 || oper < 4 || (rx == 0 && tx == 0) || rows.iter().any(|r| r.1 == rx && r.2 == tx) {
            continue;
        }
        rows.push((index, rx, tx));
    }
    let mut guard = STATE.lock().ok()?;
    let (prev, total_rx, total_tx) = guard.get_or_insert_with(|| (HashMap::new(), 0, 0));
    let mut next = HashMap::new();
    for (index, rx, tx) in rows {
        if let Some(&(prx, ptx)) = prev.get(&index) {
            *total_rx += rx.wrapping_sub(prx) as u64;
            *total_tx += tx.wrapping_sub(ptx) as u64;
        }
        next.insert(index, (rx, tx));
    }
    *prev = next;
    Some((*total_rx, *total_tx))
}

// --- NVIDIA GPU (nvml.dll ships with the driver) ---------------------------------

type NvmlDevice = *mut c_void;

pub struct Nvml {
    device: NvmlDevice,
    temperature: unsafe extern "C" fn(NvmlDevice, u32, *mut u32) -> i32,
    utilization: unsafe extern "C" fn(NvmlDevice, *mut [u32; 2]) -> i32,
}

unsafe impl Send for Nvml {}

impl Nvml {
    pub fn open() -> Option<Nvml> {
        unsafe {
            let mut lib = LoadLibraryW(wide("nvml.dll").as_ptr());
            if lib.is_null() {
                let pf = std::env::var("ProgramFiles").unwrap_or_else(|_| r"C:\Program Files".into());
                lib = LoadLibraryW(wide(&format!(r"{}\NVIDIA Corporation\NVSMI\nvml.dll", pf)).as_ptr());
            }
            if lib.is_null() {
                return None;
            }
            let sym = |name: &[u8]| {
                let p = GetProcAddress(lib, name.as_ptr());
                if p.is_null() {
                    None
                } else {
                    Some(p)
                }
            };
            let init: unsafe extern "C" fn() -> i32 = std::mem::transmute(sym(b"nvmlInit_v2\0")?);
            let by_index: unsafe extern "C" fn(u32, *mut NvmlDevice) -> i32 = std::mem::transmute(sym(b"nvmlDeviceGetHandleByIndex_v2\0")?);
            let temperature = std::mem::transmute::<*mut c_void, unsafe extern "C" fn(NvmlDevice, u32, *mut u32) -> i32>(sym(b"nvmlDeviceGetTemperature\0")?);
            let utilization = std::mem::transmute::<*mut c_void, unsafe extern "C" fn(NvmlDevice, *mut [u32; 2]) -> i32>(sym(b"nvmlDeviceGetUtilizationRates\0")?);
            if init() != 0 {
                return None;
            }
            let mut device: NvmlDevice = std::ptr::null_mut();
            if by_index(0, &mut device) != 0 {
                return None;
            }
            Some(Nvml { device, temperature, utilization })
        }
    }

    pub fn temperature(&self) -> Option<f64> {
        let mut t = 0u32;
        (unsafe { (self.temperature)(self.device, 0, &mut t) } == 0).then_some(t as f64)
    }

    pub fn utilization(&self) -> Option<f64> {
        let mut u = [0u32; 2];
        (unsafe { (self.utilization)(self.device, &mut u) } == 0).then_some(u[0] as f64)
    }
}

pub fn local_time() -> LocalTime {
    let mut t = SYSTEMTIME::default();
    unsafe { GetLocalTime(&mut t) };
    LocalTime {
        hour: t.wHour as u32,
        minute: t.wMinute as u32,
        second: t.wSecond as u32,
        day: t.wDay as u32,
        month: t.wMonth as u32,
        weekday: t.wDayOfWeek as u32,
    }
}

pub fn random_bytes(n: usize) -> Vec<u8> {
    let mut buf = vec![0u8; n];
    let ok = unsafe { SystemFunction036(buf.as_mut_ptr() as *mut c_void, n as u32) } != 0;
    if !ok {
        // practically never happens; still avoid a predictable key
        let seed = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
        let mut x = seed as u64 ^ (std::process::id() as u64) << 32 ^ 0x9E37_79B9_7F4A_7C15;
        for b in buf.iter_mut() {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            *b = x as u8;
        }
    }
    buf
}

// --- registry helpers used by usb.rs and tray.rs -------------------------------

/// A REG_SZ or REG_MULTI_SZ value as strings.
pub fn reg_strings(root: HKEY, subkey: &str, value: Option<&str>) -> Vec<String> {
    let sk = wide(subkey);
    let val = value.map(wide);
    let vp = val.as_ref().map(|v| v.as_ptr()).unwrap_or(std::ptr::null());
    let mut len = 0u32;
    let flags = RRF_RT_REG_SZ | RRF_RT_REG_MULTI_SZ;
    unsafe {
        if RegGetValueW(root, sk.as_ptr(), vp, flags, std::ptr::null_mut(), std::ptr::null_mut(), &mut len) != 0 || len == 0 {
            return Vec::new();
        }
        let mut buf = vec![0u16; len as usize / 2 + 1];
        if RegGetValueW(root, sk.as_ptr(), vp, flags, std::ptr::null_mut(), buf.as_mut_ptr() as *mut c_void, &mut len) != 0 {
            return Vec::new();
        }
        buf.truncate(len as usize / 2);
        buf.split(|&c| c == 0).filter(|s| !s.is_empty()).map(String::from_utf16_lossy).collect()
    }
}

/// Names of the subkeys of a registry key.
pub fn reg_subkeys(root: HKEY, subkey: &str) -> Vec<String> {
    let mut out = Vec::new();
    unsafe {
        let mut key: HKEY = std::ptr::null_mut();
        if RegOpenKeyExW(root, wide(subkey).as_ptr(), 0, KEY_READ, &mut key) != 0 {
            return out;
        }
        for i in 0.. {
            let mut name = [0u16; 256];
            let mut len = name.len() as u32;
            let r = RegEnumKeyExW(key, i, name.as_mut_ptr(), &mut len, std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null_mut());
            if r != 0 {
                break;
            }
            out.push(String::from_utf16_lossy(&name[..len as usize]));
        }
        RegCloseKey(key);
    }
    out
}
