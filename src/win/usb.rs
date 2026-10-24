//! The cooler over WinUSB. The LM360 asks Windows for the WinUSB driver itself
//! (Microsoft OS descriptors), so no driver install is needed.

use super::ffi::*;
use super::sys::{reg_strings, reg_subkeys};
use crate::platform::UsbLink;
use std::ffi::c_void;

const VID_PID: &str = "vid_3633&pid_0026";
const EP_OUT: u8 = 0x01;
const EP_IN: u8 = 0x81;

/// Every USB device node registers this interface.
const GUID_DEVINTERFACE_USB_DEVICE: GUID = GUID { data1: 0xA5DCBF10, data2: 0x6530, data3: 0x11D2, data4: [0x90, 0x1F, 0x00, 0xC0, 0x4F, 0xB9, 0x51, 0xED] };

struct WinUsb {
    file: HANDLE,
    iface: WINUSB_INTERFACE_HANDLE,
    path: String,
}

// The handles are only used behind the engine's device lock.
unsafe impl Send for WinUsb {}

impl Drop for WinUsb {
    fn drop(&mut self) {
        unsafe {
            WinUsb_Free(self.iface);
            CloseHandle(self.file);
        }
    }
}

impl UsbLink for WinUsb {
    fn write(&mut self, data: &[u8]) -> Result<usize, String> {
        let mut written = 0u32;
        let ok = unsafe { WinUsb_WritePipe(self.iface, EP_OUT, data.as_ptr(), data.len() as u32, &mut written, std::ptr::null_mut()) };
        if ok == 0 {
            return Err(format!("WinUsb_WritePipe failed (error {})", unsafe { GetLastError() }));
        }
        Ok(written as usize)
    }

    fn read(&mut self, buf: &mut [u8], timeout_ms: u32) -> Result<usize, String> {
        let mut t = timeout_ms;
        let mut got = 0u32;
        unsafe {
            WinUsb_SetPipePolicy(self.iface, EP_IN, PIPE_TRANSFER_TIMEOUT, 4, &mut t as *mut u32 as *mut c_void);
            if WinUsb_ReadPipe(self.iface, EP_IN, buf.as_mut_ptr(), buf.len() as u32, &mut got, std::ptr::null_mut()) == 0 {
                return Err(format!("no reply (error {})", GetLastError()));
            }
        }
        Ok(got as usize)
    }

    fn describe(&self) -> String {
        let mut d = [0u8; 18];
        let mut got = 0u32;
        let mut out = format!("Device path: {}", self.path);
        if unsafe { WinUsb_GetDescriptor(self.iface, 1, 0, 0, d.as_mut_ptr(), 18, &mut got) } != 0 && got >= 18 {
            out += &format!(
                "\nUSB {:x}.{:02x}, VID:PID {:04x}:{:04x}, device release {:x}.{:02x}",
                d[3],
                d[2],
                u16::from_le_bytes([d[8], d[9]]),
                u16::from_le_bytes([d[10], d[11]]),
                d[13],
                d[12]
            );
            for (label, index) in [("Manufacturer", d[14]), ("Product", d[15]), ("Serial", d[16])] {
                if index != 0 {
                    out += &format!("\n{}: {}", label, self.string(index).unwrap_or_else(|| "(unreadable)".into()));
                }
            }
        }
        out
    }
}

impl WinUsb {
    fn string(&self, index: u8) -> Option<String> {
        let mut buf = [0u8; 255];
        let mut got = 0u32;
        if unsafe { WinUsb_GetDescriptor(self.iface, 3, index, 0x0409, buf.as_mut_ptr(), buf.len() as u32, &mut got) } == 0 || got < 2 {
            return None;
        }
        let n = (buf[0] as usize).min(got as usize);
        let units: Vec<u16> = buf[2..n].chunks(2).filter(|c| c.len() == 2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
        Some(String::from_utf16_lossy(&units))
    }
}

/// Device paths for an interface class that belong to the cooler.
fn interface_paths(guid: &GUID) -> Vec<String> {
    let mut out = Vec::new();
    unsafe {
        let set = SetupDiGetClassDevsW(guid, std::ptr::null(), std::ptr::null_mut(), DIGCF_PRESENT | DIGCF_DEVICEINTERFACE);
        if set.is_null() || set == INVALID_HANDLE_VALUE {
            return out;
        }
        for index in 0..256 {
            let mut data = SP_DEVICE_INTERFACE_DATA { cbSize: std::mem::size_of::<SP_DEVICE_INTERFACE_DATA>() as u32, InterfaceClassGuid: GUID::default(), Flags: 0, Reserved: 0 };
            if SetupDiEnumDeviceInterfaces(set, std::ptr::null_mut(), guid, index, &mut data) == 0 {
                break;
            }
            let mut required = 0u32;
            SetupDiGetDeviceInterfaceDetailW(set, &mut data, std::ptr::null_mut(), 0, &mut required, std::ptr::null_mut());
            if required < 8 {
                continue;
            }
            // SP_DEVICE_INTERFACE_DETAIL_DATA_W: cbSize (8 on 64-bit, 6 on 32-bit), then the path
            let mut buf = vec![0u32; required as usize / 4 + 2];
            buf[0] = if cfg!(target_pointer_width = "64") { 8 } else { 6 };
            if SetupDiGetDeviceInterfaceDetailW(set, &mut data, buf.as_mut_ptr() as *mut c_void, required, std::ptr::null_mut(), std::ptr::null_mut()) == 0 {
                continue;
            }
            let units = std::slice::from_raw_parts((buf.as_ptr() as *const u16).add(2), (required as usize - 4) / 2);
            let path = from_wide(units);
            if path.to_ascii_lowercase().contains(VID_PID) {
                out.push(path);
            }
        }
        SetupDiDestroyDeviceInfoList(set);
    }
    out
}

/// Interface GUIDs the device asked for in its descriptors (stored by Windows
/// under the device's "Device Parameters" key).
fn registered_guids() -> Vec<GUID> {
    let mut out = Vec::new();
    let base = r"SYSTEM\CurrentControlSet\Enum\USB";
    for dev in reg_subkeys(HKEY_LOCAL_MACHINE, base) {
        if !dev.to_ascii_lowercase().starts_with(VID_PID) {
            continue;
        }
        for inst in reg_subkeys(HKEY_LOCAL_MACHINE, &format!(r"{}\{}", base, dev)) {
            let key = format!(r"{}\{}\{}\Device Parameters", base, dev, inst);
            for name in ["DeviceInterfaceGUIDs", "DeviceInterfaceGUID"] {
                for s in reg_strings(HKEY_LOCAL_MACHINE, &key, Some(name)) {
                    if let Some(g) = GUID::parse(&s) {
                        if !out.contains(&g) {
                            out.push(g);
                        }
                    }
                }
            }
        }
    }
    out
}

fn open_path(path: &str) -> Result<WinUsb, String> {
    unsafe {
        let file = CreateFileW(
            wide(path).as_ptr(),
            GENERIC_READ | GENERIC_WRITE,
            0, // one display owner across boot, interactive sessions and other software
            std::ptr::null_mut(),
            OPEN_EXISTING,
            FILE_ATTRIBUTE_NORMAL | FILE_FLAG_OVERLAPPED,
            std::ptr::null_mut(),
        );
        if file == INVALID_HANDLE_VALUE {
            let e = GetLastError();
            return Err(match e {
                5 => "access denied (is DeepCool's own app, or another copy of this one, using the cooler?)".into(),
                32 => "the cooler is already in use by another display process".into(),
                _ => format!("can't open the device (error {})", e),
            });
        }
        let mut iface: WINUSB_INTERFACE_HANDLE = std::ptr::null_mut();
        if WinUsb_Initialize(file, &mut iface) == 0 {
            let e = GetLastError();
            CloseHandle(file);
            return Err(format!("the device isn't using the WinUSB driver (error {})", e));
        }
        let mut timeout = 5000u32;
        WinUsb_SetPipePolicy(iface, EP_OUT, PIPE_TRANSFER_TIMEOUT, 4, &mut timeout as *mut u32 as *mut c_void);
        Ok(WinUsb { file, iface, path: path.to_string() })
    }
}

pub fn open_usb() -> Result<Box<dyn UsbLink>, String> {
    let mut guids = registered_guids();
    if !guids.contains(&GUID_DEVINTERFACE_USB_DEVICE) {
        guids.push(GUID_DEVINTERFACE_USB_DEVICE);
    }
    let mut last_err = "device not found".to_string();
    for g in &guids {
        for path in interface_paths(g) {
            match open_path(&path) {
                Ok(dev) => return Ok(Box::new(dev)),
                Err(e) => last_err = e,
            }
        }
    }
    Err(last_err)
}
