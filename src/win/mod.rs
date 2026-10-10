//! Windows-only code: system information, WinUSB, GDI+ drawing and the tray.
pub mod ffi;
pub mod gdiplus;
pub mod sys;
pub mod tray;
pub mod usb;

/// Show output in the terminal that started us (the exe is a GUI program, so
/// Windows doesn't give it a console of its own).
pub fn attach_console() {
    use ffi::*;
    unsafe {
        let have = |which| {
            let h = GetStdHandle(which);
            !h.is_null() && h != INVALID_HANDLE_VALUE
        };
        if have(STD_OUTPUT_HANDLE) {
            return; // already redirected to a file or pipe
        }
        if AttachConsole(ATTACH_PARENT_PROCESS) != 0 && !have(STD_OUTPUT_HANDLE) {
            let out = CreateFileW(wide("CONOUT$").as_ptr(), GENERIC_READ | GENERIC_WRITE, FILE_SHARE_WRITE, std::ptr::null_mut(), OPEN_EXISTING, 0, std::ptr::null_mut());
            if out != INVALID_HANDLE_VALUE {
                SetStdHandle(STD_OUTPUT_HANDLE, out);
                SetStdHandle(STD_ERROR_HANDLE, out);
            }
        }
    }
}
