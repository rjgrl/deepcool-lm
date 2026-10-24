//! Pre-sign-in display and graceful handover to the same user's tray app.

use super::ffi::*;
use crate::{config, engine::Engine, log};
use std::sync::Arc;

const STOP: &str = "Global\\DeepCoolLMBootStop";
const DONE: &str = "Global\\DeepCoolLMBootDone";

struct LocalMemory(*mut std::ffi::c_void);

struct Token(HANDLE);

impl Drop for Token {
    fn drop(&mut self) {
        unsafe { CloseHandle(self.0); }
    }
}

impl Drop for LocalMemory {
    fn drop(&mut self) {
        unsafe { LocalFree(self.0); }
    }
}

fn event_security() -> Result<LocalMemory, String> {
    let mut token = std::ptr::null_mut();
    if unsafe { OpenProcessToken(GetCurrentProcess(), 0x8, &mut token) } == 0 { // TOKEN_QUERY
        return Err(format!("Cannot read boot event owner: {}", std::io::Error::last_os_error()));
    }
    let token = Token(token);
    let mut size = 0;
    unsafe { GetTokenInformation(token.0, 1, std::ptr::null_mut(), 0, &mut size); } // TokenUser
    if size == 0 {
        return Err(format!("Cannot size boot event owner: {}", std::io::Error::last_os_error()));
    }
    // TOKEN_USER contains a pointer and must be aligned, including its SID buffer.
    let mut buffer = vec![0usize; (size as usize).div_ceil(std::mem::size_of::<usize>())];
    if unsafe { GetTokenInformation(token.0, 1, buffer.as_mut_ptr().cast(), size, &mut size) } == 0 {
        return Err(format!("Cannot read boot event owner: {}", std::io::Error::last_os_error()));
    }
    let sid = unsafe { (*(buffer.as_ptr().cast::<TOKEN_USER>())).User.Sid };
    let mut text = std::ptr::null_mut();
    if unsafe { ConvertSidToStringSidW(sid, &mut text) } == 0 {
        return Err(format!("Cannot format boot event owner: {}", std::io::Error::last_os_error()));
    }
    let _text_memory = LocalMemory(text.cast());
    let mut len = 0;
    unsafe { while *text.add(len) != 0 { len += 1; } }
    let sid = String::from_utf16_lossy(unsafe { std::slice::from_raw_parts(text, len) });
    // S4U defaults can exclude the interactive token. Grant only this user and SYSTEM;
    // label medium so a normal tray can signal events created by an elevated worker.
    let sddl = wide(&format!("D:P(A;;GA;;;SY)(A;;GA;;;{sid})S:(ML;;NW;;;ME)"));
    let mut descriptor = std::ptr::null_mut();
    if unsafe { ConvertStringSecurityDescriptorToSecurityDescriptorW(sddl.as_ptr(), 1, &mut descriptor, std::ptr::null_mut()) } == 0 {
        return Err(format!("Cannot secure boot event: {}", std::io::Error::last_os_error()));
    }
    Ok(LocalMemory(descriptor))
}

struct Event(HANDLE);

// Each handle has one owner; Windows events can be waited on from any thread.
unsafe impl Send for Event {}

impl Event {
    fn create(name: &str) -> Result<(Self, bool), String> {
        let security = event_security()?;
        let mut attributes = SECURITY_ATTRIBUTES {
            nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: security.0,
            bInheritHandle: 0,
        };
        let handle = unsafe { CreateEventW((&mut attributes as *mut SECURITY_ATTRIBUTES).cast(), 1, 0, wide(name).as_ptr()) };
        let error = unsafe { GetLastError() };
        if handle.is_null() {
            return Err(format!("Cannot create boot event (Windows error {})", error));
        }
        Ok((Self(handle), error == ERROR_ALREADY_EXISTS))
    }

    fn signal(&self) -> Result<(), String> {
        if unsafe { SetEvent(self.0) } == 0 {
            return Err(format!("Cannot signal boot event: {}", std::io::Error::last_os_error()));
        }
        Ok(())
    }

    fn wait(&self, timeout_ms: u32) -> Result<bool, String> {
        match unsafe { WaitForSingleObject(self.0, timeout_ms) } {
            0 => Ok(true),
            WAIT_TIMEOUT => Ok(false),
            _ => Err(format!("Cannot wait for boot event: {}", std::io::Error::last_os_error())),
        }
    }
}

impl Drop for Event {
    fn drop(&mut self) {
        unsafe { CloseHandle(self.0); }
    }
}

struct BootSession {
    stop: Event,
    done: Event,
}

impl BootSession {
    fn new(stop_name: &str, done_name: &str) -> Result<Option<Self>, String> {
        let (stop, exists) = Event::create(stop_name)?;
        if exists {
            return Ok(None); // another boot worker or the interactive app already owns the screen
        }
        let (done, exists) = Event::create(done_name)?;
        if exists {
            return Err("Previous boot handover has not finished".into());
        }
        Ok(Some(Self { stop, done }))
    }
}

impl Drop for BootSession {
    fn drop(&mut self) {
        let _ = self.done.signal();
    }
}

/// Kept alive by the interactive app so a delayed boot task cannot take over.
pub struct UserSession {
    _stop: Event,
}

fn user_session_named(stop_name: &str, done_name: &str, timeout_ms: u32) -> Result<UserSession, String> {
    let (stop, _) = Event::create(stop_name)?;
    stop.signal()?;
    let handle = unsafe { OpenEventW(0x00100000, 0, wide(done_name).as_ptr()) }; // SYNCHRONIZE
    if handle.is_null() {
        let error = unsafe { GetLastError() };
        if error != 2 { // ERROR_FILE_NOT_FOUND: no boot worker to wait for
            return Err(format!("Cannot open boot completion event (Windows error {})", error));
        }
    } else if !Event(handle).wait(timeout_ms)? {
        return Err("Boot display did not release the cooler within 15 seconds; try starting the app again".into());
    }
    Ok(UserSession { _stop: stop })
}

pub fn user_session() -> Result<UserSession, String> {
    user_session_named(STOP, DONE, 15_000)
}

pub fn run(rotation: i32) -> Result<(), String> {
    log::init(&config::data_dir().join("boot.log"));
    let Some(session) = BootSession::new(STOP, DONE)? else {
        log!("Boot display skipped: another display owner is already running");
        return Ok(());
    };
    if session.stop.wait(0)? {
        return Ok(());
    }
    log!("DeepCool LM boot display starting");
    let engine = Arc::new(Engine::new(rotation, 1.0));
    let display_engine = engine.clone();
    let display = std::thread::Builder::new().name("boot-display".into()).spawn(move || display_engine.run()).map_err(|e| e.to_string())?;
    let wait_result = loop {
        match session.stop.wait(1000) {
            Ok(true) => break Ok(()),
            Ok(false) if !display.is_finished() => {},
            Ok(false) => break Err("Boot display thread stopped unexpectedly".into()),
            Err(e) => break Err(e),
        }
    };
    engine.shutdown(false);
    let joined = display.join();
    log!("Boot display released the cooler");
    joined.map_err(|_| "Boot display thread panicked".to_string())?;
    wait_result
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;

    fn names(test: &str) -> (String, String) {
        let base = format!("Global\\DeepCoolLMTest{}{}", std::process::id(), test);
        (format!("{}Stop", base), format!("{}Done", base))
    }

    #[test]
    fn user_waits_for_boot_to_release_and_blocks_late_boot() {
        let (stop, done) = names("Handover");
        let boot = BootSession::new(&stop, &done).unwrap().unwrap();
        assert!(BootSession::new(&stop, &done).unwrap().is_none());
        let released = Arc::new(AtomicBool::new(false));
        let released_worker = released.clone();
        let worker = std::thread::spawn(move || {
            assert!(boot.stop.wait(5000).unwrap());
            released_worker.store(true, Ordering::SeqCst);
            drop(boot);
        });
        let user = user_session_named(&stop, &done, 5000).unwrap();
        assert!(released.load(Ordering::SeqCst));
        assert!(BootSession::new(&stop, &done).unwrap().is_none());
        worker.join().unwrap();
        drop(user);
        assert!(BootSession::new(&stop, &done).unwrap().is_some());
    }

    #[test]
    fn user_starting_first_blocks_boot() {
        let (stop, done) = names("UserFirst");
        let user = user_session_named(&stop, &done, 100).unwrap();
        assert!(BootSession::new(&stop, &done).unwrap().is_none());
        drop(user);
        assert!(BootSession::new(&stop, &done).unwrap().is_some());
    }

    #[test]
    fn failed_handover_does_not_start_another_display_owner() {
        let (stop, done) = names("Timeout");
        let boot = BootSession::new(&stop, &done).unwrap().unwrap();
        assert!(user_session_named(&stop, &done, 10).is_err());
        assert!(boot.stop.wait(0).unwrap());
        drop(boot);
    }

    #[test]
    fn control_event_access_does_not_depend_on_logon_default_dacl() {
        struct Restore(HANDLE, *mut std::ffi::c_void);
        impl Drop for Restore {
            fn drop(&mut self) {
                unsafe {
                    RevertToSelf();
                    if !self.0.is_null() { CloseHandle(self.0); }
                    if !self.1.is_null() { LocalFree(self.1); }
                }
            }
        }
        let mut restore = Restore(std::ptr::null_mut(), std::ptr::null_mut());
        unsafe {
            assert_ne!(ImpersonateSelf(2), 0); // SecurityImpersonation
            assert_ne!(OpenThreadToken(GetCurrentThread(), 0x80, 1, &mut restore.0), 0); // TOKEN_ADJUST_DEFAULT
            assert_ne!(ConvertStringSecurityDescriptorToSecurityDescriptorW(wide("D:(A;;GA;;;SY)").as_ptr(), 1, &mut restore.1, std::ptr::null_mut()), 0);
            let mut acl = std::ptr::null_mut();
            let mut present = 0;
            let mut defaulted = 0;
            assert_ne!(GetSecurityDescriptorDacl(restore.1, &mut present, &mut acl, &mut defaulted), 0);
            let dacl = TOKEN_DEFAULT_DACL { DefaultDacl: acl };
            assert_ne!(SetTokenInformation(restore.0, 6, &dacl as *const _ as *const _, std::mem::size_of_val(&dacl) as u32), 0); // TokenDefaultDacl
        }
        let (name, _) = names("RestrictedDacl");
        let (_event, existed) = Event::create(&name).unwrap();
        assert!(!existed);
        let handle = unsafe { OpenEventW(0x00100002, 0, wide(&name).as_ptr()) };
        assert!(!handle.is_null(), "same user must be able to signal and wait regardless of the creator's logon DACL");
        let reopened = Event(handle);
        reopened.signal().unwrap();
        assert!(reopened.wait(0).unwrap());
    }
}
