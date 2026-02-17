#![doc = include_str!("../README.md")]

use futures::{
  Stream,
  channel::mpsc::{self, Receiver, Sender},
};
use log::{debug, error, info, trace, warn};
use std::sync::{
  Arc, Mutex,
  atomic::{AtomicBool, AtomicUsize, Ordering},
  mpsc::sync_channel,
};
use std::{
  collections::HashMap,
  fmt::Display,
  path::PathBuf,
  pin::Pin,
  task::{Context, Poll},
  thread::JoinHandle,
  time::{Duration, Instant},
};

mod body;
pub use body::*;
mod body_senders;
use body_senders::*;
mod driver;
use driver::Driver;
mod error;
pub use error::*;

mod event_listener;
pub use event_listener::*;

pub(crate) mod logging;
use logging::*;

mod observer;
use observer::Observer;
mod stream;
pub use stream::*;

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(windows)]
mod win;

impl IntoIterator for Formats {
  type Item = Format;
  type IntoIter = std::vec::IntoIter<Format>;

  #[inline]
  fn into_iter(self) -> Self::IntoIter {
    self.data.into_iter()
  }
}

impl<'a> IntoIterator for &'a Formats {
  type Item = &'a Format;
  type IntoIter = std::slice::Iter<'a, Format>;

  #[inline]
  fn into_iter(self) -> Self::IntoIter {
    self.data.iter()
  }
}

pub struct ClipboardContext<'a> {
  formats: &'a Formats,
  #[cfg(target_os = "linux")]
  x11: &'a linux::observer::X11Context,
  #[cfg(target_os = "macos")]
  pasteboard: &'a objc2::rc::Retained<objc2_app_kit::NSPasteboard>,
}

impl ClipboardContext<'_> {
  #[must_use]
  #[inline]
  pub const fn formats(&self) -> &Formats {
    self.formats
  }

  #[must_use]
  #[inline]
  pub fn has_format(&self, name: &str) -> bool {
    self.formats.iter().any(|d| d.name.as_ref() == name)
  }

  #[must_use]
  #[inline]
  pub fn get_format(&self, name: &str) -> Option<&Format> {
    self.formats.iter().find(|d| d.name.as_ref() == name)
  }

  #[must_use]
  #[inline]
  pub fn get_u32(&self, name: &str) -> Option<u32> {
    self
      .get_format_data(name)
      .and_then(|bytes| Some(u32::from_ne_bytes(bytes.try_into().ok()?)))
  }

  #[must_use]
  #[inline]
  pub fn get_format_data(&self, name: &str) -> Option<Vec<u8>> {
    self
      .formats
      .iter()
      .find(|d| d.name.as_ref() == name)
      .and_then(|f| self.get_data(f))
  }
}

pub type Gatekeeper = Box<dyn Fn(&ClipboardContext) -> bool + Send + Sync>;

#[derive(Debug, Clone)]
pub struct Format {
  pub(crate) name: Arc<str>,
  #[cfg(not(target_os = "macos"))]
  pub(crate) id: u32,
  #[cfg(target_os = "macos")]
  pub(crate) id: objc2::rc::Retained<objc2_foundation::NSString>,
}

impl Format {
  #[must_use]
  #[inline]
  pub fn name(&self) -> &str {
    &self.name
  }
}

#[derive(Default)]
pub struct Formats {
  pub(crate) data: Vec<Format>,
}

impl Formats {
  #[inline]
  pub fn iter(&self) -> std::slice::Iter<'_, Format> {
    self.data.iter()
  }

  #[cfg(not(target_os = "macos"))]
  #[must_use]
  #[inline]
  pub fn contains_id(&self, id: u32) -> bool {
    self.data.iter().any(|d| d.id == id)
  }
}

impl FromIterator<Format> for Formats {
  fn from_iter<T: IntoIterator<Item = Format>>(iter: T) -> Self {
    Self {
      data: iter.into_iter().collect(),
    }
  }
}
