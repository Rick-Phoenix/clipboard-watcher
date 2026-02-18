#![doc = include_str!("../README.md")]

use futures::{
  Stream,
  channel::mpsc::{self, Receiver, Sender},
};
use log::{debug, error, info, trace, warn};
use std::{
  collections::HashMap,
  fmt::Display,
  path::PathBuf,
  pin::Pin,
  sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicUsize, Ordering},
    mpsc::sync_channel,
  },
  task::{Context, Poll},
  thread::JoinHandle,
  time::Duration,
};

mod body;
pub use body::*;

mod body_senders;
use body_senders::*;

mod error;
pub use error::*;

mod event_listener;
pub use event_listener::*;

mod logging;
use logging::*;

mod stream;
pub use stream::*;

mod formats;
pub use formats::*;

#[cfg(target_os = "linux")]
mod linux {
  pub(crate) mod driver;
  pub(crate) mod observer;
}
#[cfg(target_os = "macos")]
mod macos {
  pub(crate) mod driver;
  pub(crate) mod observer;
}
#[cfg(windows)]
mod win {
  mod driver;
  mod observer;
}

pub(crate) trait Observer {
  fn observe(&mut self, body_senders: Arc<BodySenders>);
}

/// The struct that is responsible for starting and stopping the Observer.
#[derive(Debug)]
pub(crate) struct Driver {
  /// This is cloned and passed to the Observer threads to give them the interruption signal
  pub(crate) stop: Arc<AtomicBool>,

  /// This is the handle of the spawned Observer thread.
  pub(crate) handle: Option<JoinHandle<()>>,
}

/// The context for the clipboard content
#[derive(Clone, Copy)]
pub struct ClipboardContext<'a> {
  formats: &'a Formats,
  #[cfg(target_os = "linux")]
  x11: &'a linux::observer::X11Context,
  #[cfg(target_os = "macos")]
  pasteboard: &'a objc2::rc::Retained<objc2_app_kit::NSPasteboard>,
}

impl ClipboardContext<'_> {
  /// Returns the list of [`Format`]s currently available on the clipboard.
  #[must_use]
  #[inline]
  pub const fn formats(&self) -> &Formats {
    self.formats
  }

  /// Checks if a particular format is currently present in the clipboard.
  #[must_use]
  #[inline]
  pub fn has_format(&self, name: &str) -> bool {
    self.formats.iter().any(|d| d.name.as_ref() == name)
  }

  /// Attempts to extract a particular [`Format`] from the list of available formats.
  #[must_use]
  #[inline]
  pub fn get_format(&self, name: &str) -> Option<&Format> {
    self.formats.iter().find(|d| d.name.as_ref() == name)
  }

  /// Attempts to read the content of a particular format as a 32 bit integer.
  #[must_use]
  #[inline]
  pub fn get_format_as_u32(&self, name: &str) -> Option<u32> {
    self
      .get_format_data(name)
      .and_then(|bytes| Some(u32::from_ne_bytes(bytes.try_into().ok()?)))
  }

  /// Attempts to read the raw data for a particular format.
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

/// Receives the [`ClipboardContext`] and returns a boolean that indicates whether the content should
/// be processed or not.
///
/// Can be useful to read particular formats like `ExcludeClipboardContentFromMonitorProcessing` that are
/// placed in the clipboard by other applications.
pub trait Gatekeeper: Send + Sync + 'static {
  fn check(&self, ctx: ClipboardContext) -> bool;
}

impl<F> Gatekeeper for F
where
  F: Fn(ClipboardContext) -> bool + Send + Sync + 'static,
{
  #[inline]
  fn check(&self, ctx: ClipboardContext) -> bool {
    (self)(ctx)
  }
}

#[derive(Default)]
pub struct DefaultGatekeeper;

impl Gatekeeper for DefaultGatekeeper {
  #[inline]
  fn check(&self, _: ClipboardContext) -> bool {
    true
  }
}
