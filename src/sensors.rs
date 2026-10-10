//! Sensor readings: CPU/RAM/disk/network from the OS, temperatures and clocks
//! from LibreHardwareMonitor's web server, NVIDIA GPU fallback via NVML.

use crate::json::{self, Json};
use crate::platform;
use std::collections::{BTreeMap, HashMap, VecDeque};
use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::{Duration, Instant};

#[derive(Clone, Debug)]
pub struct Reading {
    pub name: String,
    pub group: String,
    pub value: Option<f64>,
    pub unit: String,
}

pub type Snapshot = BTreeMap<String, Reading>;

/// The headline numbers (log lines and the classic screen).
#[derive(Clone, Debug, Default)]
pub struct Info {
    pub cpu_temp: f64,
    pub gpu_temp: f64,
    pub cpu_percent: f64,
    pub cpu_freq: f64,
    pub gpu_usage: Option<f64>,
}

// ---------------------------------------------------------------------------
// LibreHardwareMonitor

pub fn lhm_url() -> String {
    std::env::var("DEEPCOOL_LM_LHM_URL").unwrap_or_else(|_| "http://127.0.0.1:8085/data.json".into())
}

/// One sensor from LibreHardwareMonitor's data.json.
#[derive(Clone, Debug)]
pub struct LhmSensor {
    pub id: String,       // e.g. /amdcpu/0/temperature/2
    pub name: String,     // e.g. Core (Tctl/Tdie)
    pub hardware: String, // e.g. AMD Ryzen 7 7800X3D
    pub kind: String,     // Temperature, Load, Clock, ...
    pub parent: String,   // hardware id, e.g. /amdcpu/0
    pub value: Option<f64>,
}

fn number_in(text: &str) -> Option<f64> {
    // '45.0 °C' / '45,0 °C' / '4700 MHz' -> f64
    let t: String = text.chars().skip_while(|c| !(c.is_ascii_digit() || *c == '-')).collect();
    let mut s = String::new();
    for (i, c) in t.chars().enumerate() {
        if c.is_ascii_digit() || (c == '-' && i == 0) {
            s.push(c);
        } else if (c == '.' || c == ',') && !s.contains('.') {
            s.push('.');
        } else {
            break;
        }
    }
    s.trim_end_matches('.').parse().ok()
}

const GROUP_TYPES: [(&str, &str); 2] = [("temperatures", "Temperature"), ("clocks", "Clock")];
const ICON_PARENTS: [(&str, &str); 5] =
    [("cpu.png", "/cpu"), ("nvidia.png", "/gpu-nvidia"), ("ati.png", "/gpu-amd"), ("amd.png", "/gpu-amd"), ("intel.png", "/gpu-intel")];

/// Flatten the data.json tree (new format with SensorId, or older formatted-only trees).
pub fn parse_lhm(root: &Json) -> Vec<LhmSensor> {
    let mut out = Vec::new();
    walk(root, None, String::new(), String::new(), &mut out);
    out
}

fn walk(node: &Json, parent: Option<String>, hardware: String, group: String, out: &mut Vec<LhmSensor>) {
    let children = node.get("Children").and_then(|c| c.as_array()).cloned().unwrap_or_default();
    let mut parent = parent;
    let mut hardware = hardware;
    if let Some(hid) = node.get("HardwareId").and_then(|v| v.as_str()) {
        parent = Some(hid.to_string());
        hardware = node.str_or("Text", "").to_string();
    } else {
        let icon = node.str_or("ImageURL", "").rsplit('/').next().unwrap_or("").to_ascii_lowercase();
        if let Some((_, p)) = ICON_PARENTS.iter().find(|(i, _)| *i == icon) {
            parent = Some(p.to_string()); // pre-0.9.5 trees have no ids
            hardware = node.str_or("Text", "").to_string();
        }
    }
    let sensor_id = node.get("SensorId").and_then(|v| v.as_str());
    let group_type = GROUP_TYPES.iter().find(|(g, _)| *g == group).map(|(_, t)| *t);
    if sensor_id.is_some() || (children.is_empty() && group_type.is_some()) {
        let value = match node.get("RawValue") {
            Some(Json::Num(n)) => Some(*n),
            _ => number_in(node.str_or("Value", "")),
        };
        let kind = node.get("Type").and_then(|v| v.as_str()).map(String::from).unwrap_or_else(|| group_type.unwrap_or("").to_string());
        let id = sensor_id.map(String::from).unwrap_or_default();
        let parent_id = if !id.is_empty() {
            let mut parts: Vec<&str> = id.rsplitn(3, '/').collect();
            parts.pop().unwrap_or("").to_string()
        } else {
            parent.clone().unwrap_or_default()
        };
        out.push(LhmSensor { id, name: node.str_or("Text", "").to_string(), hardware, kind, parent: parent_id, value });
        return;
    }
    let text = node.str_or("Text", "").to_ascii_lowercase();
    for c in &children {
        walk(c, parent.clone(), hardware.clone(), text.clone(), out);
    }
}

const CPU_TEMP_PREFERENCE: [&str; 7] = ["cpu package", "core (tctl/tdie)", "core (tctl)", "tctl/tdie", "core max", "core average", "cpu cores"];
const GPU_TEMP_PREFERENCE: [&str; 2] = ["gpu core", "gpu hot spot"];
const GPU_VENDOR_PREFERENCE: [&str; 4] = ["nvidia", "amd", "ati", ""]; // discrete before integrated

/// Pick CPU temperature, GPU temperature, average CPU clock (GHz) and GPU load.
pub fn pick_headlines(sensors: &[LhmSensor]) -> (Option<f64>, Option<f64>, Option<f64>, Option<f64>) {
    let mut cpu_temps: Vec<(String, f64)> = Vec::new();
    let mut gpu_temps: Vec<(String, String, f64)> = Vec::new();
    let mut gpu_loads: Vec<(String, f64)> = Vec::new();
    let mut clocks = Vec::new();
    for s in sensors {
        let Some(v) = s.value else { continue };
        let (name, parent) = (s.name.to_ascii_lowercase(), s.parent.to_ascii_lowercase());
        let is_gpu = parent.contains("gpu");
        let is_cpu = parent.contains("cpu") && !is_gpu;
        match s.kind.as_str() {
            "Temperature" if !name.contains("distance") => {
                if is_cpu && !cpu_temps.iter().any(|(n, _)| *n == name) {
                    cpu_temps.push((name, v));
                } else if is_gpu {
                    gpu_temps.push((parent, name, v));
                }
            }
            "Clock" if is_cpu => {
                if is_core_clock(&name) {
                    clocks.push(v);
                }
            }
            "Load" if is_gpu && name == "gpu core" => gpu_loads.push((parent, v)),
            _ => {}
        }
    }
    let cpu_temp = CPU_TEMP_PREFERENCE
        .iter()
        .find_map(|p| cpu_temps.iter().find(|(n, _)| n == p).map(|(_, v)| *v))
        .or_else(|| cpu_temps.iter().map(|(_, v)| *v).fold(None, |m: Option<f64>, v| Some(m.map_or(v, |m| m.max(v)))));
    let vendor_rank = |parent: &str| GPU_VENDOR_PREFERENCE.iter().position(|v| parent.contains(v)).unwrap_or(9);
    let gpu_temp = gpu_temps
        .iter()
        .min_by_key(|(p, n, _)| (vendor_rank(p), GPU_TEMP_PREFERENCE.iter().position(|g| g == n).unwrap_or(9)))
        .map(|(_, _, v)| *v);
    let gpu_usage = gpu_loads.iter().min_by_key(|(p, _)| vendor_rank(p)).map(|(_, v)| *v);
    let cpu_freq = if clocks.is_empty() { None } else { Some(clocks.iter().sum::<f64>() / clocks.len() as f64 / 1000.0) };
    (cpu_temp.map(round1), gpu_temp.map(round1), cpu_freq.map(round2), gpu_usage.map(round1))
}

fn is_core_clock(name: &str) -> bool {
    // "Core #1" / "CPU Core #1", but not "Core #1 (Effective)"
    if name.contains("effective") {
        return false;
    }
    name.find("core #").map(|i| name[i + 6..].chars().next().map(|c| c.is_ascii_digit()).unwrap_or(false)).unwrap_or(false)
}

pub fn unit_for(kind: &str) -> &'static str {
    match kind {
        "Temperature" => "°C",
        "Load" | "Control" | "Level" | "Humidity" => "%",
        "Clock" => "MHz",
        "Voltage" => "V",
        "Power" => "W",
        "Fan" => "RPM",
        "Data" => "GB",
        "SmallData" => "MB",
        "Throughput" => "MB/s",
        "Frequency" => "Hz",
        "Flow" => "L/h",
        "Current" => "A",
        "Noise" => "dBA",
        "Energy" => "mWh",
        "TimeSpan" => "s",
        _ => "",
    }
}

fn round1(v: f64) -> f64 {
    (v * 10.0).round() / 10.0
}
fn round2(v: f64) -> f64 {
    (v * 100.0).round() / 100.0
}

/// Fetch data.json from LibreHardwareMonitor (plain HTTP to localhost, no proxy).
pub fn http_get_json(url: &str, timeout: Duration) -> Result<Json, String> {
    let rest = url.strip_prefix("http://").ok_or("only http:// URLs are supported")?;
    let (hostport, path) = match rest.find('/') {
        Some(i) => (&rest[..i], &rest[i..]),
        None => (rest, "/"),
    };
    let addr = if hostport.contains(':') { hostport.to_string() } else { format!("{}:80", hostport) };
    let sock: std::net::SocketAddr = std::net::ToSocketAddrs::to_socket_addrs(&addr)
        .map_err(|e| e.to_string())?
        .next()
        .ok_or("no address")?;
    let mut s = TcpStream::connect_timeout(&sock, timeout).map_err(|e| e.to_string())?;
    s.set_read_timeout(Some(timeout)).ok();
    s.set_write_timeout(Some(timeout)).ok();
    write!(s, "GET {} HTTP/1.1\r\nHost: {}\r\nConnection: close\r\nAccept: application/json\r\n\r\n", path, hostport)
        .map_err(|e| e.to_string())?;
    let mut raw = Vec::new();
    s.read_to_end(&mut raw).map_err(|e| e.to_string())?;
    let split = raw.windows(4).position(|w| w == b"\r\n\r\n").ok_or("bad HTTP response")?;
    let head = String::from_utf8_lossy(&raw[..split]).to_ascii_lowercase();
    if !head.starts_with("http/1.") || !head.split_whitespace().nth(1).map(|c| c == "200").unwrap_or(false) {
        return Err(format!("HTTP error: {}", head.lines().next().unwrap_or("")));
    }
    let mut body = raw[split + 4..].to_vec();
    if head.contains("transfer-encoding: chunked") {
        body = dechunk(&body)?;
    }
    json::parse(&String::from_utf8_lossy(&body))
}

fn dechunk(mut data: &[u8]) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    loop {
        let line_end = data.windows(2).position(|w| w == b"\r\n").ok_or("bad chunk")?;
        let size_text = String::from_utf8_lossy(&data[..line_end]);
        let size = usize::from_str_radix(size_text.split(';').next().unwrap_or("").trim(), 16).map_err(|_| "bad chunk size")?;
        data = &data[line_end + 2..];
        if size == 0 {
            return Ok(out);
        }
        if data.len() < size {
            return Err("truncated chunk".into());
        }
        out.extend_from_slice(&data[..size]);
        data = data.get(size + 2..).unwrap_or(&[]);
    }
}

// ---------------------------------------------------------------------------
// Hub

pub struct SensorHub {
    prev_cpu: Option<platform::CpuTimes>,
    prev_net: Option<(Instant, u64, u64)>,
    lhm_next_try: Instant,
    lhm_source_logged: bool,
    hint_logged: bool,
    nvml: Option<platform::Nvml>,
    nvml_tried: bool,
    pub info: Info,
}

impl SensorHub {
    pub fn new() -> Self {
        SensorHub {
            prev_cpu: None,
            prev_net: None,
            lhm_next_try: Instant::now(),
            lhm_source_logged: false,
            hint_logged: false,
            nvml: None,
            nvml_tried: false,
            info: Info::default(),
        }
    }

    fn lhm(&mut self) -> Vec<LhmSensor> {
        if Instant::now() < self.lhm_next_try {
            return Vec::new();
        }
        match http_get_json(&lhm_url(), Duration::from_secs(2)) {
            Ok(root) => {
                if !self.lhm_source_logged {
                    crate::log!("Temperatures: reading from LibreHardwareMonitor web server ({})", lhm_url());
                    self.lhm_source_logged = true;
                }
                parse_lhm(&root)
            }
            Err(_) => {
                self.lhm_next_try = Instant::now() + Duration::from_secs(15);
                self.lhm_source_logged = false;
                Vec::new()
            }
        }
    }

    pub fn collect(&mut self) -> Snapshot {
        let mut snap = Snapshot::new();
        let mut add = |id: &str, name: &str, group: &str, value: Option<f64>, unit: &str| {
            snap.insert(id.to_string(), Reading { name: name.into(), group: group.into(), value, unit: unit.into() });
        };

        // CPU usage from OS time counters
        let mut cpu_percent = 0.0;
        if let Some(now) = platform::cpu_times() {
            if let Some(prev) = &self.prev_cpu {
                let total = (now.total - prev.total) as f64;
                let idle = (now.idle - prev.idle) as f64;
                if total > 0.0 {
                    cpu_percent = round1(((total - idle) / total * 100.0).clamp(0.0, 100.0));
                }
            }
            self.prev_cpu = Some(now);
        }

        let lhm = self.lhm();
        let (lhm_cpu_temp, lhm_gpu_temp, lhm_clock, lhm_gpu_load) = pick_headlines(&lhm);
        let cpu_freq = lhm_clock.or_else(|| platform::cpu_mhz().map(|m| round2(m / 1000.0))).unwrap_or(0.0);

        let mut gpu_temp = lhm_gpu_temp;
        let mut gpu_usage = lhm_gpu_load;
        if gpu_temp.is_none() || gpu_usage.is_none() {
            if !self.nvml_tried {
                self.nvml_tried = true;
                self.nvml = platform::Nvml::open();
            }
            if let Some(n) = &self.nvml {
                gpu_temp = gpu_temp.or_else(|| n.temperature());
                gpu_usage = gpu_usage.or_else(|| n.utilization());
            }
        }
        if lhm_cpu_temp.is_none() && !self.hint_logged {
            self.hint_logged = true;
            crate::log!(
                "Note: no CPU temperature source found yet. Run LibreHardwareMonitor as administrator and turn on \
                 Options > Remote Web Server > Run (expected at {}). Will keep checking.",
                lhm_url()
            );
        }

        self.info = Info {
            cpu_temp: lhm_cpu_temp.unwrap_or(0.0),
            gpu_temp: gpu_temp.unwrap_or(0.0),
            cpu_percent,
            cpu_freq,
            gpu_usage,
        };
        add("cpu.temp", "CPU temperature", "Common", Some(self.info.cpu_temp), "°C");
        add("cpu.usage", "CPU usage", "Common", Some(cpu_percent), "%");
        add("cpu.clock", "CPU clock", "Common", Some(cpu_freq), "GHz");
        add("gpu.temp", "GPU temperature", "Common", Some(self.info.gpu_temp), "°C");
        add("gpu.usage", "GPU usage", "Common", gpu_usage, "%");

        if let Some(m) = platform::memory() {
            add("ram.usage", "RAM usage", "Common", Some(round1(m.used as f64 / m.total.max(1) as f64 * 100.0)), "%");
            add("ram.used", "RAM used", "Common", Some(round1(m.used as f64 / 1073741824.0)), "GB");
            add("ram.total", "RAM total", "Common", Some(round1(m.total as f64 / 1073741824.0)), "GB");
        }
        if let Some(pct) = platform::system_disk_usage() {
            add("disk.usage", "System drive usage", "Common", Some(round1(pct)), "%");
        }
        if let Some((rx, tx)) = platform::net_bytes() {
            let now = Instant::now();
            let (mut down, mut up) = (0.0, 0.0);
            if let Some((t0, r0, s0)) = self.prev_net {
                let dt = now.duration_since(t0).as_secs_f64().max(1e-3);
                down = rx.saturating_sub(r0) as f64 / dt / 1e6;
                up = tx.saturating_sub(s0) as f64 / dt / 1e6;
            }
            self.prev_net = Some((now, rx, tx));
            add("net.down", "Network download", "Common", Some(round2(down)), "MB/s");
            add("net.up", "Network upload", "Common", Some(round2(up)), "MB/s");
        }
        for s in &lhm {
            if s.id.is_empty() {
                continue;
            }
            let value = s.value.map(|v| if s.kind == "Throughput" { v / 1e6 } else { v }); // raw B/s -> MB/s
            let group = if s.hardware.is_empty() { "LibreHardwareMonitor" } else { &s.hardware };
            add(&format!("lhm:{}", s.id), &s.name, group, value, unit_for(&s.kind));
        }
        snap
    }
}

/// Recent values per sensor, for graph widgets.
pub struct History {
    max_seconds: f64,
    data: HashMap<String, VecDeque<(f64, f64)>>,
}

impl History {
    pub fn new(max_seconds: f64) -> Self {
        History { max_seconds, data: HashMap::new() }
    }

    pub fn add(&mut self, snap: &Snapshot, now: f64) {
        for (id, r) in snap {
            if let Some(v) = r.value {
                let dq = self.data.entry(id.clone()).or_default();
                dq.push_back((now, v));
                while dq.front().map(|(t, _)| *t < now - self.max_seconds).unwrap_or(false) {
                    dq.pop_front();
                }
            }
        }
    }

    pub fn series(&self, id: &str, seconds: f64, now: f64) -> Vec<(f64, f64)> {
        self.data.get(id).map(|dq| dq.iter().filter(|(t, _)| *t >= now - seconds).copied().collect()).unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sensor(text: &str, id: &str, kind: &str, raw: f64) -> String {
        format!(r#"{{"Text":"{}","SensorId":"{}","Type":"{}","RawValue":{},"Value":"x","Children":[]}}"#, text, id, kind, raw)
    }

    #[test]
    fn new_format() {
        let doc = format!(
            r#"{{"Text":"Sensor","Children":[{{"Text":"PC","Children":[
              {{"Text":"AMD Ryzen 7 7800X3D","HardwareId":"/amdcpu/0","Children":[
                {{"Text":"Clocks","Children":[{},{},{}]}},
                {{"Text":"Temperatures","Children":[{},{}]}}]}},
              {{"Text":"NVIDIA GeForce RTX 4070","HardwareId":"/gpu-nvidia/0","Children":[
                {{"Text":"Temperatures","Children":[{},{}]}},{{"Text":"Load","Children":[{}]}}]}},
              {{"Text":"Intel UHD","HardwareId":"/gpu-intel-integrated/0","Children":[{{"Text":"Temperatures","Children":[{}]}}]}}
            ]}}]}}"#,
            sensor("Bus Speed", "/amdcpu/0/clock/0", "Clock", 100.0),
            sensor("Core #1", "/amdcpu/0/clock/1", "Clock", 4950.2),
            sensor("Core #2", "/amdcpu/0/clock/2", "Clock", 4849.8),
            sensor("Core (Tctl/Tdie)", "/amdcpu/0/temperature/2", "Temperature", 61.25),
            sensor("CCD1 (Tdie)", "/amdcpu/0/temperature/3", "Temperature", 58.0),
            sensor("GPU Core", "/gpu-nvidia/0/temperature/0", "Temperature", 36.0),
            sensor("GPU Hot Spot", "/gpu-nvidia/0/temperature/2", "Temperature", 45.1),
            sensor("GPU Core", "/gpu-nvidia/0/load/0", "Load", 12.0),
            sensor("GPU Core", "/gpu-intel-integrated/0/temperature/0", "Temperature", 44.0),
        );
        let s = parse_lhm(&json::parse(&doc).unwrap());
        assert_eq!(s.len(), 9);
        assert_eq!(s[3].parent, "/amdcpu/0");
        assert_eq!(s[3].hardware, "AMD Ryzen 7 7800X3D");
        let (ct, gt, clk, gl) = pick_headlines(&s);
        assert_eq!((ct, gt, clk, gl), (Some(61.3), Some(36.0), Some(4.9), Some(12.0)));
    }

    #[test]
    fn old_format_with_comma_decimals() {
        let doc = r#"{"Text":"Sensor","Children":[{"Text":"PC","ImageURL":"images_icon/computer.png","Children":[
            {"Text":"Intel Core i9","ImageURL":"images_icon/cpu.png","Children":[
              {"Text":"Temperatures","Children":[{"Text":"CPU Package","Value":"72,0 °C","Children":[]},{"Text":"Core Max","Value":"75,0 °C","Children":[]}]},
              {"Text":"Clocks","Children":[{"Text":"CPU Core #1","Value":"5100,0 MHz","Children":[]}]}]},
            {"Text":"Radeon","ImageURL":"images_icon/ati.png","Children":[{"Text":"Temperatures","Children":[{"Text":"GPU Core","Value":"50,0 °C","Children":[]}]}]}]}]}"#;
        let s = parse_lhm(&json::parse(doc).unwrap());
        let (ct, gt, clk, _) = pick_headlines(&s);
        assert_eq!((ct, gt, clk), (Some(72.0), Some(50.0), Some(5.1)));
    }

    #[test]
    fn helpers() {
        assert_eq!(number_in("45,5 °C"), Some(45.5));
        assert_eq!(number_in("4700 MHz"), Some(4700.0));
        assert_eq!(number_in("-3.5"), Some(-3.5));
        assert!(is_core_clock("cpu core #3") && !is_core_clock("core #1 (effective)") && !is_core_clock("bus speed"));
        assert_eq!(dechunk(b"4\r\nWiki\r\n5\r\npedia\r\n0\r\n\r\n").unwrap(), b"Wikipedia");
    }

    #[test]
    fn history_window() {
        let mut h = History::new(10.0);
        let mut snap = Snapshot::new();
        for t in 0..30 {
            snap.insert("a".into(), Reading { name: "a".into(), group: "".into(), value: Some(t as f64), unit: "".into() });
            h.add(&snap, t as f64);
        }
        assert_eq!(h.series("a", 5.0, 29.0).len(), 6);
        assert_eq!(h.series("a", 60.0, 29.0).len(), 11); // trimmed to max_seconds
    }
}
