//! Windows API declarations (only what this app uses). Libraries are linked
//! with `raw-dylib`, so no Windows SDK import libraries are needed to build.
//! `--cfg wincheck` type-checks this file on other platforms (nothing links).
#![allow(non_snake_case, non_camel_case_types, dead_code, clippy::upper_case_acronyms)]

use std::ffi::c_void;

pub type BOOL = i32;
pub type HANDLE = *mut c_void;
pub type HWND = *mut c_void;
pub type HMENU = *mut c_void;
pub type HICON = *mut c_void;
pub type HINSTANCE = *mut c_void;
pub type HKEY = *mut c_void;
pub type WPARAM = usize;
pub type LPARAM = isize;
pub type LRESULT = isize;
pub type WNDPROC = unsafe extern "system" fn(HWND, u32, WPARAM, LPARAM) -> LRESULT;

pub const INVALID_HANDLE_VALUE: HANDLE = -1isize as HANDLE;
pub const ERROR_ALREADY_EXISTS: u32 = 183;

#[repr(C)]
pub struct SECURITY_ATTRIBUTES {
    pub nLength: u32,
    pub lpSecurityDescriptor: *mut c_void,
    pub bInheritHandle: BOOL,
}

#[repr(C)]
pub struct SID_AND_ATTRIBUTES {
    pub Sid: *mut c_void,
    pub Attributes: u32,
}

#[repr(C)]
pub struct TOKEN_USER {
    pub User: SID_AND_ATTRIBUTES,
}

#[repr(C)]
pub struct TOKEN_DEFAULT_DACL {
    pub DefaultDacl: *mut c_void,
}

#[repr(C)]
#[derive(Clone, Copy, Default, PartialEq, Debug)]
pub struct GUID {
    pub data1: u32,
    pub data2: u16,
    pub data3: u16,
    pub data4: [u8; 8],
}

impl GUID {
    /// Parse "{xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx}" (braces optional).
    pub fn parse(s: &str) -> Option<GUID> {
        let s = s.trim().trim_start_matches('{').trim_end_matches('}');
        let parts: Vec<&str> = s.split('-').collect();
        if parts.len() != 5 || [8, 4, 4, 4, 12] != [parts[0].len(), parts[1].len(), parts[2].len(), parts[3].len(), parts[4].len()] {
            return None;
        }
        let tail = format!("{}{}", parts[3], parts[4]);
        let mut data4 = [0u8; 8];
        for (i, b) in data4.iter_mut().enumerate() {
            *b = u8::from_str_radix(&tail[i * 2..i * 2 + 2], 16).ok()?;
        }
        Some(GUID {
            data1: u32::from_str_radix(parts[0], 16).ok()?,
            data2: u16::from_str_radix(parts[1], 16).ok()?,
            data3: u16::from_str_radix(parts[2], 16).ok()?,
            data4,
        })
    }
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct FILETIME {
    pub low: u32,
    pub high: u32,
}

impl FILETIME {
    pub fn get(&self) -> u64 {
        (self.high as u64) << 32 | self.low as u64
    }
}

#[repr(C)]
#[derive(Default)]
pub struct SYSTEMTIME {
    pub wYear: u16,
    pub wMonth: u16,
    pub wDayOfWeek: u16,
    pub wDay: u16,
    pub wHour: u16,
    pub wMinute: u16,
    pub wSecond: u16,
    pub wMilliseconds: u16,
}

#[repr(C)]
#[derive(Default)]
pub struct MEMORYSTATUSEX {
    pub dwLength: u32,
    pub dwMemoryLoad: u32,
    pub ullTotalPhys: u64,
    pub ullAvailPhys: u64,
    pub ullTotalPageFile: u64,
    pub ullAvailPageFile: u64,
    pub ullTotalVirtual: u64,
    pub ullAvailVirtual: u64,
    pub ullAvailExtendedVirtual: u64,
}

#[repr(C)]
pub struct STARTUPINFOW {
    pub cb: u32,
    pub lpReserved: *mut u16,
    pub lpDesktop: *mut u16,
    pub lpTitle: *mut u16,
    pub dwX: u32,
    pub dwY: u32,
    pub dwXSize: u32,
    pub dwYSize: u32,
    pub dwXCountChars: u32,
    pub dwYCountChars: u32,
    pub dwFillAttribute: u32,
    pub dwFlags: u32,
    pub wShowWindow: u16,
    pub cbReserved2: u16,
    pub lpReserved2: *mut u8,
    pub hStdInput: HANDLE,
    pub hStdOutput: HANDLE,
    pub hStdError: HANDLE,
}

#[repr(C)]
pub struct PROCESS_INFORMATION {
    pub hProcess: HANDLE,
    pub hThread: HANDLE,
    pub dwProcessId: u32,
    pub dwThreadId: u32,
}

pub const GENERIC_READ: u32 = 0x8000_0000;
pub const GENERIC_WRITE: u32 = 0x4000_0000;
pub const FILE_SHARE_READ: u32 = 1;
pub const FILE_SHARE_WRITE: u32 = 2;
pub const OPEN_EXISTING: u32 = 3;
pub const FILE_ATTRIBUTE_NORMAL: u32 = 0x80;
pub const FILE_FLAG_OVERLAPPED: u32 = 0x4000_0000;
pub const ATTACH_PARENT_PROCESS: u32 = u32::MAX;
pub const STD_OUTPUT_HANDLE: u32 = -11i32 as u32;
pub const STD_ERROR_HANDLE: u32 = -12i32 as u32;
pub const CREATE_NO_WINDOW: u32 = 0x0800_0000;

#[cfg_attr(windows, link(name = "kernel32", kind = "raw-dylib"))]
extern "system" {
    pub fn GetSystemTimes(idle: *mut FILETIME, kernel: *mut FILETIME, user: *mut FILETIME) -> BOOL;
    pub fn GlobalMemoryStatusEx(buf: *mut MEMORYSTATUSEX) -> BOOL;
    pub fn GetDiskFreeSpaceExW(dir: *const u16, free_to_caller: *mut u64, total: *mut u64, total_free: *mut u64) -> BOOL;
    pub fn GetLocalTime(t: *mut SYSTEMTIME);
    pub fn LoadLibraryW(name: *const u16) -> HINSTANCE;
    pub fn GetProcAddress(module: HINSTANCE, name: *const u8) -> *mut c_void;
    pub fn GetModuleHandleW(name: *const u16) -> HINSTANCE;
    pub fn CreateFileW(name: *const u16, access: u32, share: u32, security: *mut c_void, disposition: u32, flags: u32, template: HANDLE) -> HANDLE;
    pub fn CloseHandle(h: HANDLE) -> BOOL;
    pub fn CreateMutexW(security: *mut c_void, initial_owner: BOOL, name: *const u16) -> HANDLE;
    pub fn CreateEventW(security: *mut c_void, manual_reset: BOOL, initial_state: BOOL, name: *const u16) -> HANDLE;
    pub fn OpenEventW(access: u32, inherit: BOOL, name: *const u16) -> HANDLE;
    pub fn SetEvent(event: HANDLE) -> BOOL;
    pub fn GetLastError() -> u32;
    pub fn AttachConsole(pid: u32) -> BOOL;
    pub fn GetStdHandle(which: u32) -> HANDLE;
    pub fn SetStdHandle(which: u32, h: HANDLE) -> BOOL;
    pub fn CreateProcessW(
        app: *const u16,
        cmdline: *mut u16,
        proc_attr: *mut c_void,
        thread_attr: *mut c_void,
        inherit: BOOL,
        flags: u32,
        env: *mut c_void,
        cwd: *const u16,
        si: *mut STARTUPINFOW,
        pi: *mut PROCESS_INFORMATION,
    ) -> BOOL;
    pub fn TerminateProcess(h: HANDLE, code: u32) -> BOOL;
    pub fn WaitForSingleObject(h: HANDLE, ms: u32) -> u32;
    pub fn SetProcessWorkingSetSize(process: HANDLE, min: usize, max: usize) -> BOOL;
    pub fn GetCurrentProcess() -> HANDLE;
    pub fn GetCurrentThread() -> HANDLE;
    pub fn LocalFree(memory: *mut c_void) -> *mut c_void;
}

pub const WAIT_TIMEOUT: u32 = 258;

// --- iphlpapi / powrprof / advapi32 -------------------------------------------

#[cfg_attr(windows, link(name = "iphlpapi", kind = "raw-dylib"))]
extern "system" {
    /// MIB_IFTABLE: u32 count, then MIB_IFROW entries of 860 bytes.
    pub fn GetIfTable(table: *mut u8, size: *mut u32, order: BOOL) -> u32;
}

#[cfg_attr(windows, link(name = "powrprof", kind = "raw-dylib"))]
extern "system" {
    pub fn CallNtPowerInformation(level: i32, input: *mut c_void, in_len: u32, output: *mut c_void, out_len: u32) -> i32;
}

pub const HKEY_LOCAL_MACHINE: HKEY = 0x8000_0002u32 as usize as HKEY;
pub const HKEY_CURRENT_USER: HKEY = 0x8000_0001u32 as usize as HKEY;
pub const KEY_READ: u32 = 0x20019;
pub const RRF_RT_REG_SZ: u32 = 0x2;
pub const REG_SZ: u32 = 1;
pub const RRF_RT_REG_MULTI_SZ: u32 = 0x20;

#[cfg_attr(windows, link(name = "advapi32", kind = "raw-dylib"))]
extern "system" {
    pub fn SystemFunction036(buf: *mut c_void, len: u32) -> u8; // RtlGenRandom
    pub fn OpenProcessToken(process: HANDLE, access: u32, token: *mut HANDLE) -> BOOL;
    pub fn OpenThreadToken(thread: HANDLE, access: u32, as_self: BOOL, token: *mut HANDLE) -> BOOL;
    pub fn GetTokenInformation(token: HANDLE, class: u32, data: *mut c_void, size: u32, required: *mut u32) -> BOOL;
    pub fn SetTokenInformation(token: HANDLE, class: u32, data: *const c_void, size: u32) -> BOOL;
    pub fn ConvertSidToStringSidW(sid: *mut c_void, text: *mut *mut u16) -> BOOL;
    pub fn ConvertStringSecurityDescriptorToSecurityDescriptorW(text: *const u16, revision: u32, descriptor: *mut *mut c_void, size: *mut u32) -> BOOL;
    pub fn GetSecurityDescriptorDacl(descriptor: *mut c_void, present: *mut BOOL, acl: *mut *mut c_void, defaulted: *mut BOOL) -> BOOL;
    pub fn ImpersonateSelf(level: i32) -> BOOL;
    pub fn RevertToSelf() -> BOOL;
    pub fn RegOpenKeyExW(key: HKEY, subkey: *const u16, options: u32, sam: u32, result: *mut HKEY) -> i32;
    pub fn RegEnumKeyExW(key: HKEY, index: u32, name: *mut u16, name_len: *mut u32, reserved: *mut u32, class: *mut u16, class_len: *mut u32, time: *mut FILETIME) -> i32;
    pub fn RegGetValueW(key: HKEY, subkey: *const u16, value: *const u16, flags: u32, kind: *mut u32, data: *mut c_void, len: *mut u32) -> i32;
    pub fn RegCloseKey(key: HKEY) -> i32;
    pub fn RegSetKeyValueW(key: HKEY, subkey: *const u16, value: *const u16, kind: u32, data: *const c_void, len: u32) -> i32;
    pub fn RegDeleteKeyValueW(key: HKEY, subkey: *const u16, value: *const u16) -> i32;
}

// --- SetupAPI + WinUSB -------------------------------------------------------------

pub type HDEVINFO = *mut c_void;
pub const DIGCF_PRESENT: u32 = 0x2;
pub const DIGCF_DEVICEINTERFACE: u32 = 0x10;

#[repr(C)]
pub struct SP_DEVICE_INTERFACE_DATA {
    pub cbSize: u32,
    pub InterfaceClassGuid: GUID,
    pub Flags: u32,
    pub Reserved: usize,
}

#[cfg_attr(windows, link(name = "setupapi", kind = "raw-dylib"))]
extern "system" {
    pub fn SetupDiGetClassDevsW(guid: *const GUID, enumerator: *const u16, parent: HWND, flags: u32) -> HDEVINFO;
    pub fn SetupDiEnumDeviceInterfaces(set: HDEVINFO, devinfo: *mut c_void, guid: *const GUID, index: u32, data: *mut SP_DEVICE_INTERFACE_DATA) -> BOOL;
    pub fn SetupDiGetDeviceInterfaceDetailW(set: HDEVINFO, data: *mut SP_DEVICE_INTERFACE_DATA, detail: *mut c_void, size: u32, required: *mut u32, devinfo: *mut c_void) -> BOOL;
    pub fn SetupDiDestroyDeviceInfoList(set: HDEVINFO) -> BOOL;
}

pub type WINUSB_INTERFACE_HANDLE = *mut c_void;
pub const PIPE_TRANSFER_TIMEOUT: u32 = 0x03;

#[cfg_attr(windows, link(name = "winusb", kind = "raw-dylib"))]
extern "system" {
    pub fn WinUsb_Initialize(device: HANDLE, iface: *mut WINUSB_INTERFACE_HANDLE) -> BOOL;
    pub fn WinUsb_Free(iface: WINUSB_INTERFACE_HANDLE) -> BOOL;
    pub fn WinUsb_WritePipe(iface: WINUSB_INTERFACE_HANDLE, pipe: u8, buf: *const u8, len: u32, written: *mut u32, overlapped: *mut c_void) -> BOOL;
    pub fn WinUsb_ReadPipe(iface: WINUSB_INTERFACE_HANDLE, pipe: u8, buf: *mut u8, len: u32, read: *mut u32, overlapped: *mut c_void) -> BOOL;
    pub fn WinUsb_SetPipePolicy(iface: WINUSB_INTERFACE_HANDLE, pipe: u8, policy: u32, len: u32, value: *mut c_void) -> BOOL;
    pub fn WinUsb_GetDescriptor(iface: WINUSB_INTERFACE_HANDLE, kind: u8, index: u8, lang: u16, buf: *mut u8, len: u32, got: *mut u32) -> BOOL;
}

// --- GDI+ flat API --------------------------------------------------------------------

pub type GpStatus = i32;
pub type GpImage = c_void;
pub type GpBitmap = c_void;
pub type GpGraphics = c_void;
pub type GpBrush = c_void;
pub type GpPen = c_void;
pub type GpPath = c_void;
pub type GpFont = c_void;
pub type GpFontFamily = c_void;
pub type GpStringFormat = c_void;

#[repr(C)]
pub struct GdiplusStartupInput {
    pub GdiplusVersion: u32,
    pub DebugEventCallback: *mut c_void,
    pub SuppressBackgroundThread: BOOL,
    pub SuppressExternalCodecs: BOOL,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct RectF {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct PointF {
    pub x: f32,
    pub y: f32,
}

#[repr(C)]
pub struct PropertyItem {
    pub id: u32,
    pub length: u32,
    pub kind: u16,
    pub value: *mut c_void,
}

pub const PIXEL_FORMAT_32BPP_ARGB: i32 = 0x26200A;
pub const UNIT_PIXEL: i32 = 2;
pub const SMOOTHING_ANTIALIAS: i32 = 4;
pub const TEXT_ANTIALIAS_GRIDFIT: i32 = 3;
pub const PIXEL_OFFSET_HALF: i32 = 4;
pub const INTERPOLATION_HQ_BICUBIC: i32 = 7;
pub const COMPOSITING_QUALITY_HIGH: i32 = 2;
pub const LINE_CAP_ROUND: i32 = 2;
pub const LINE_JOIN_ROUND: i32 = 2;
pub const FILL_MODE_WINDING: i32 = 1;
pub const STRING_FORMAT_MEASURE_TRAILING_SPACES: i32 = 0x0800;
pub const STRING_FORMAT_NO_WRAP: i32 = 0x1000;
pub const STRING_FORMAT_NO_CLIP: i32 = 0x4000;
pub const PROPERTY_TAG_FRAME_DELAY: u32 = 0x5100;
pub const FLUSH_SYNC: i32 = 1;

#[cfg_attr(windows, link(name = "gdiplus", kind = "raw-dylib"))]
extern "system" {
    pub fn GdiplusStartup(token: *mut usize, input: *const GdiplusStartupInput, output: *mut c_void) -> GpStatus;
    pub fn GdipCreateBitmapFromScan0(w: i32, h: i32, stride: i32, format: i32, scan0: *mut u8, bitmap: *mut *mut GpBitmap) -> GpStatus;
    pub fn GdipCreateBitmapFromStream(stream: *mut c_void, bitmap: *mut *mut GpBitmap) -> GpStatus;
    pub fn GdipCreateHICONFromBitmap(bitmap: *mut GpBitmap, icon: *mut HICON) -> GpStatus;
    pub fn GdipDisposeImage(image: *mut GpImage) -> GpStatus;
    pub fn GdipGetImageWidth(image: *mut GpImage, w: *mut u32) -> GpStatus;
    pub fn GdipGetImageHeight(image: *mut GpImage, h: *mut u32) -> GpStatus;
    pub fn GdipImageGetFrameDimensionsCount(image: *mut GpImage, count: *mut u32) -> GpStatus;
    pub fn GdipImageGetFrameDimensionsList(image: *mut GpImage, ids: *mut GUID, count: u32) -> GpStatus;
    pub fn GdipImageGetFrameCount(image: *mut GpImage, dimension: *const GUID, count: *mut u32) -> GpStatus;
    pub fn GdipImageSelectActiveFrame(image: *mut GpImage, dimension: *const GUID, index: u32) -> GpStatus;
    pub fn GdipGetPropertyItemSize(image: *mut GpImage, id: u32, size: *mut u32) -> GpStatus;
    pub fn GdipGetPropertyItem(image: *mut GpImage, id: u32, size: u32, item: *mut PropertyItem) -> GpStatus;
    pub fn GdipGetImageGraphicsContext(image: *mut GpImage, g: *mut *mut GpGraphics) -> GpStatus;
    pub fn GdipDeleteGraphics(g: *mut GpGraphics) -> GpStatus;
    pub fn GdipFlush(g: *mut GpGraphics, intention: i32) -> GpStatus;
    pub fn GdipSetSmoothingMode(g: *mut GpGraphics, mode: i32) -> GpStatus;
    pub fn GdipSetTextRenderingHint(g: *mut GpGraphics, hint: i32) -> GpStatus;
    pub fn GdipSetPixelOffsetMode(g: *mut GpGraphics, mode: i32) -> GpStatus;
    pub fn GdipSetInterpolationMode(g: *mut GpGraphics, mode: i32) -> GpStatus;
    pub fn GdipSetCompositingQuality(g: *mut GpGraphics, q: i32) -> GpStatus;
    pub fn GdipGraphicsClear(g: *mut GpGraphics, argb: u32) -> GpStatus;
    pub fn GdipCreateSolidFill(argb: u32, brush: *mut *mut GpBrush) -> GpStatus;
    pub fn GdipDeleteBrush(brush: *mut GpBrush) -> GpStatus;
    pub fn GdipCreatePen1(argb: u32, width: f32, unit: i32, pen: *mut *mut GpPen) -> GpStatus;
    pub fn GdipDeletePen(pen: *mut GpPen) -> GpStatus;
    pub fn GdipSetPenStartCap(pen: *mut GpPen, cap: i32) -> GpStatus;
    pub fn GdipSetPenEndCap(pen: *mut GpPen, cap: i32) -> GpStatus;
    pub fn GdipSetPenLineJoin(pen: *mut GpPen, join: i32) -> GpStatus;
    pub fn GdipCreatePath(fill_mode: i32, path: *mut *mut GpPath) -> GpStatus;
    pub fn GdipDeletePath(path: *mut GpPath) -> GpStatus;
    pub fn GdipAddPathArc(path: *mut GpPath, x: f32, y: f32, w: f32, h: f32, start: f32, sweep: f32) -> GpStatus;
    pub fn GdipAddPathRectangle(path: *mut GpPath, x: f32, y: f32, w: f32, h: f32) -> GpStatus;
    pub fn GdipClosePathFigure(path: *mut GpPath) -> GpStatus;
    pub fn GdipFillPath(g: *mut GpGraphics, brush: *mut GpBrush, path: *mut GpPath) -> GpStatus;
    pub fn GdipDrawPath(g: *mut GpGraphics, pen: *mut GpPen, path: *mut GpPath) -> GpStatus;
    pub fn GdipFillRectangle(g: *mut GpGraphics, brush: *mut GpBrush, x: f32, y: f32, w: f32, h: f32) -> GpStatus;
    pub fn GdipFillEllipse(g: *mut GpGraphics, brush: *mut GpBrush, x: f32, y: f32, w: f32, h: f32) -> GpStatus;
    pub fn GdipDrawArc(g: *mut GpGraphics, pen: *mut GpPen, x: f32, y: f32, w: f32, h: f32, start: f32, sweep: f32) -> GpStatus;
    pub fn GdipDrawLines(g: *mut GpGraphics, pen: *mut GpPen, pts: *const PointF, count: i32) -> GpStatus;
    pub fn GdipFillPolygon(g: *mut GpGraphics, brush: *mut GpBrush, pts: *const PointF, count: i32, fill_mode: i32) -> GpStatus;
    pub fn GdipDrawImageRectRect(
        g: *mut GpGraphics,
        image: *mut GpImage,
        dx: f32,
        dy: f32,
        dw: f32,
        dh: f32,
        sx: f32,
        sy: f32,
        sw: f32,
        sh: f32,
        unit: i32,
        attrs: *mut c_void,
        callback: *mut c_void,
        data: *mut c_void,
    ) -> GpStatus;
    pub fn GdipCreateFontFamilyFromName(name: *const u16, collection: *mut c_void, family: *mut *mut GpFontFamily) -> GpStatus;
    pub fn GdipNewInstalledFontCollection(collection: *mut *mut c_void) -> GpStatus;
    pub fn GdipGetFontCollectionFamilyCount(collection: *mut c_void, count: *mut i32) -> GpStatus;
    pub fn GdipGetFontCollectionFamilyList(collection: *mut c_void, wanted: i32, families: *mut *mut GpFontFamily, found: *mut i32) -> GpStatus;
    pub fn GdipGetFamilyName(family: *mut GpFontFamily, name: *mut u16, language: u16) -> GpStatus;
    pub fn GdipGetGenericFontFamilySansSerif(family: *mut *mut GpFontFamily) -> GpStatus;
    pub fn GdipDeleteFontFamily(family: *mut GpFontFamily) -> GpStatus;
    pub fn GdipGetCellAscent(family: *mut GpFontFamily, style: i32, ascent: *mut u16) -> GpStatus;
    pub fn GdipGetCellDescent(family: *mut GpFontFamily, style: i32, descent: *mut u16) -> GpStatus;
    pub fn GdipGetEmHeight(family: *mut GpFontFamily, style: i32, em: *mut u16) -> GpStatus;
    pub fn GdipCreateFont(family: *mut GpFontFamily, size: f32, style: i32, unit: i32, font: *mut *mut GpFont) -> GpStatus;
    pub fn GdipDeleteFont(font: *mut GpFont) -> GpStatus;
    pub fn GdipStringFormatGetGenericTypographic(format: *mut *mut GpStringFormat) -> GpStatus;
    pub fn GdipCloneStringFormat(format: *mut GpStringFormat, clone: *mut *mut GpStringFormat) -> GpStatus;
    pub fn GdipSetStringFormatFlags(format: *mut GpStringFormat, flags: i32) -> GpStatus;
    pub fn GdipDeleteStringFormat(format: *mut GpStringFormat) -> GpStatus;
    pub fn GdipMeasureString(
        g: *mut GpGraphics,
        text: *const u16,
        len: i32,
        font: *mut GpFont,
        layout: *const RectF,
        format: *mut GpStringFormat,
        bounds: *mut RectF,
        fitted: *mut i32,
        lines: *mut i32,
    ) -> GpStatus;
    pub fn GdipDrawString(g: *mut GpGraphics, text: *const u16, len: i32, font: *mut GpFont, layout: *const RectF, format: *mut GpStringFormat, brush: *mut GpBrush) -> GpStatus;
}

#[cfg_attr(windows, link(name = "shlwapi", kind = "raw-dylib"))]
extern "system" {
    /// Returns an IStream* over a copy of the bytes (release with `com_release`).
    pub fn SHCreateMemStream(data: *const u8, len: u32) -> *mut c_void;
}

/// IUnknown::Release on a COM object pointer.
pub unsafe fn com_release(obj: *mut c_void) {
    if !obj.is_null() {
        let vtbl = *(obj as *const *const usize);
        let release: unsafe extern "system" fn(*mut c_void) -> u32 = std::mem::transmute(*vtbl.add(2));
        release(obj);
    }
}

// --- user32 / shell32 ------------------------------------------------------------------

#[repr(C)]
pub struct WNDCLASSEXW {
    pub cbSize: u32,
    pub style: u32,
    pub lpfnWndProc: WNDPROC,
    pub cbClsExtra: i32,
    pub cbWndExtra: i32,
    pub hInstance: HINSTANCE,
    pub hIcon: HICON,
    pub hCursor: *mut c_void,
    pub hbrBackground: *mut c_void,
    pub lpszMenuName: *const u16,
    pub lpszClassName: *const u16,
    pub hIconSm: HICON,
}

#[repr(C)]
pub struct MSG {
    pub hwnd: HWND,
    pub message: u32,
    pub wParam: WPARAM,
    pub lParam: LPARAM,
    pub time: u32,
    pub pt: POINT,
    pub lPrivate: u32,
}

#[repr(C)]
#[derive(Default, Clone, Copy)]
pub struct POINT {
    pub x: i32,
    pub y: i32,
}

#[repr(C)]
pub struct NOTIFYICONDATAW {
    pub cbSize: u32,
    pub hWnd: HWND,
    pub uID: u32,
    pub uFlags: u32,
    pub uCallbackMessage: u32,
    pub hIcon: HICON,
    pub szTip: [u16; 128],
    pub dwState: u32,
    pub dwStateMask: u32,
    pub szInfo: [u16; 256],
    pub uVersion: u32,
    pub szInfoTitle: [u16; 64],
    pub dwInfoFlags: u32,
    pub guidItem: GUID,
    pub hBalloonIcon: HICON,
}

pub const WM_DESTROY: u32 = 0x0002;
pub const WM_CLOSE: u32 = 0x0010;
pub const WM_TIMER: u32 = 0x0113;
pub const WM_COMMAND: u32 = 0x0111;
pub const WM_LBUTTONUP: u32 = 0x0202;
pub const WM_RBUTTONUP: u32 = 0x0205;
pub const WM_LBUTTONDBLCLK: u32 = 0x0203;
pub const WM_CONTEXTMENU: u32 = 0x007B;
pub const WM_NULL: u32 = 0;
pub const WM_APP: u32 = 0x8000;
pub const WM_ENDSESSION: u32 = 0x0016;
pub const WM_QUERYENDSESSION: u32 = 0x0011;
pub const NIM_ADD: u32 = 0;
pub const NIM_MODIFY: u32 = 1;
pub const NIM_DELETE: u32 = 2;
pub const NIF_MESSAGE: u32 = 1;
pub const NIF_ICON: u32 = 2;
pub const NIF_TIP: u32 = 4;
pub const MF_STRING: u32 = 0;
pub const MF_GRAYED: u32 = 1;
pub const MF_DISABLED: u32 = 2;
pub const MF_CHECKED: u32 = 8;
pub const MF_POPUP: u32 = 0x10;
pub const MF_SEPARATOR: u32 = 0x800;
pub const TPM_RIGHTBUTTON: u32 = 2;
pub const TPM_RETURNCMD: u32 = 0x100;
pub const TPM_NONOTIFY: u32 = 0x80;
pub const SW_RESTORE: i32 = 9;
pub const SW_SHOWNORMAL: i32 = 1;

#[cfg_attr(windows, link(name = "user32", kind = "raw-dylib"))]
extern "system" {
    pub fn RegisterClassExW(wc: *const WNDCLASSEXW) -> u16;
    pub fn CreateWindowExW(
        ex_style: u32,
        class: *const u16,
        title: *const u16,
        style: u32,
        x: i32,
        y: i32,
        w: i32,
        h: i32,
        parent: HWND,
        menu: HMENU,
        instance: HINSTANCE,
        param: *mut c_void,
    ) -> HWND;
    pub fn DestroyWindow(hwnd: HWND) -> BOOL;
    pub fn DefWindowProcW(hwnd: HWND, msg: u32, w: WPARAM, l: LPARAM) -> LRESULT;
    pub fn GetMessageW(msg: *mut MSG, hwnd: HWND, min: u32, max: u32) -> BOOL;
    pub fn TranslateMessage(msg: *const MSG) -> BOOL;
    pub fn DispatchMessageW(msg: *const MSG) -> LRESULT;
    pub fn PostMessageW(hwnd: HWND, msg: u32, w: WPARAM, l: LPARAM) -> BOOL;
    pub fn PostQuitMessage(code: i32);
    pub fn RegisterWindowMessageW(name: *const u16) -> u32;
    pub fn FindWindowW(class: *const u16, title: *const u16) -> HWND;
    pub fn ShowWindow(hwnd: HWND, cmd: i32) -> BOOL;
    pub fn IsIconic(hwnd: HWND) -> BOOL;
    pub fn SetForegroundWindow(hwnd: HWND) -> BOOL;
    pub fn AllowSetForegroundWindow(pid: u32) -> BOOL;
    pub fn CreatePopupMenu() -> HMENU;
    pub fn AppendMenuW(menu: HMENU, flags: u32, id: usize, text: *const u16) -> BOOL;
    pub fn SetMenuDefaultItem(menu: HMENU, item: u32, by_pos: u32) -> BOOL;
    pub fn TrackPopupMenu(menu: HMENU, flags: u32, x: i32, y: i32, reserved: i32, hwnd: HWND, rect: *const c_void) -> BOOL;
    pub fn DestroyMenu(menu: HMENU) -> BOOL;
    pub fn GetCursorPos(pt: *mut POINT) -> BOOL;
    pub fn SetTimer(hwnd: HWND, id: usize, ms: u32, func: *mut c_void) -> usize;
    pub fn DestroyIcon(icon: HICON) -> BOOL;
}

#[cfg_attr(windows, link(name = "shell32", kind = "raw-dylib"))]
extern "system" {
    pub fn Shell_NotifyIconW(msg: u32, data: *mut NOTIFYICONDATAW) -> BOOL;
    pub fn ShellExecuteW(hwnd: HWND, op: *const u16, file: *const u16, params: *const u16, dir: *const u16, show: i32) -> HINSTANCE;
}

/// NUL-terminated UTF-16 for Windows APIs.
pub fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// UTF-16 buffer (up to the first NUL) to String.
pub fn from_wide(buf: &[u16]) -> String {
    let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..end])
}
