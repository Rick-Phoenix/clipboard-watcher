mod body;
mod driver;
pub mod error;
mod event_listener;
#[cfg(target_os = "linux")]
mod linux;
pub(crate) mod logging;
#[cfg(target_os = "macos")]
mod macos;
mod observer;
mod stream;
#[cfg(windows)]
mod win;

pub use stream::{ClipboardStream, StreamId};

pub use crate::{
  body::{Body, RawImage},
  event_listener::ClipboardEventListener,
};
