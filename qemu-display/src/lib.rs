#![doc = include_str!("../README.md")]
#![allow(clippy::too_many_arguments)]

pub use zbus;

pub mod util;
#[cfg(windows)]
mod win32;

mod error;
pub use error::*;

mod vm;
pub use vm::*;

mod audio;
pub use audio::*;

mod chardev;
pub use chardev::*;

mod clipboard;
pub use clipboard::*;

mod console;
pub use console::*;

mod console_listener;
pub use console_listener::*;

mod keyboard;
pub use keyboard::*;

mod mouse;
pub use mouse::*;

mod display;
pub use display::*;

mod multi_touch;
pub use multi_touch::*;

#[cfg(unix)]
mod usbredir;
#[cfg(unix)]
pub use usbredir::*;

#[cfg(test)]
mod tests {
    #[test]
    fn it_works() {
        assert_eq!(2 + 2, 4);
    }
}
