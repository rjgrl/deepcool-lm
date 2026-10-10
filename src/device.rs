//! The cooler's screen: frames and commands over the USB bulk pipe.

use crate::platform::{self, UsbLink};
use crate::util;

pub struct Lm360 {
    link: Box<dyn UsbLink>,
}

impl Lm360 {
    pub fn open() -> Result<Lm360, String> {
        Ok(Lm360 { link: platform::open_usb()? })
    }

    fn write(&mut self, data: &[u8]) -> bool {
        match self.link.write(data) {
            Ok(n) if n == data.len() => true,
            Ok(n) => {
                log!("USB write short: {} of {} bytes", n, data.len());
                false
            }
            Err(e) => {
                log!("USB write error: {}", e);
                false
            }
        }
    }

    /// Send one 320x240 RGB565 frame. False if the write failed.
    pub fn send_frame(&mut self, frame: &[u8]) -> bool {
        self.write(&util::frame_header()) && self.write(frame)
    }

    pub fn init(&mut self) -> bool {
        self.write(&util::init_packet())
    }

    pub fn set_brightness(&mut self, level: u8) -> bool {
        self.write(&util::brightness_packet(level))
    }

    pub fn zen_toggle(&mut self) -> bool {
        self.write(&util::zen_packet())
    }

    pub fn read_reply(&mut self) -> Result<Vec<u8>, String> {
        let mut buf = [0u8; 512];
        let n = self.link.read(&mut buf, 1000)?;
        Ok(buf[..n].to_vec())
    }

    pub fn describe(&self) -> String {
        self.link.describe()
    }
}
