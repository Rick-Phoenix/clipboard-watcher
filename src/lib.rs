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

	fn into_iter(self) -> Self::IntoIter {
		self.data.into_iter()
	}
}

impl<'a> IntoIterator for &'a Formats {
	type Item = &'a Format;
	type IntoIter = std::slice::Iter<'a, Format>;

	fn into_iter(self) -> Self::IntoIter {
		self.data.iter()
	}
}

pub struct ClipboardContext<'a> {
	pub formats: &'a Formats,
	#[cfg(target_os = "linux")]
	x11: &'a linux::observer::X11Context,
	#[cfg(target_os = "macos")]
	pasteboard: &'a objc2::rc::Retained<objc2_app_kit::NSPasteboard>,
}

pub type Gatekeeper = Box<dyn Fn(&ClipboardContext) -> bool + Send + Sync>;

#[derive(Debug, Clone)]
pub struct Format {
	pub name: Arc<str>,
	#[cfg(not(target_os = "macos"))]
	pub id: u32,
	#[cfg(target_os = "macos")]
	pub id: objc2::rc::Retained<objc2_foundation::NSString>,
}

#[derive(Default)]
pub struct Formats {
	pub data: Vec<Format>,
}

impl Formats {
	pub fn iter(&self) -> std::slice::Iter<'_, Format> {
		self.data.iter()
	}

	#[cfg(not(target_os = "macos"))]
	#[must_use]
	pub fn contains_id(&self, id: u32) -> bool {
		self.data.iter().any(|d| d.id == id)
	}

	#[must_use]
	pub fn contains_name(&self, name: &str) -> bool {
		self.data.iter().any(|d| d.name.as_ref() == name)
	}
}

impl FromIterator<Format> for Formats {
	fn from_iter<T: IntoIterator<Item = Format>>(iter: T) -> Self {
		Self {
			data: iter.into_iter().collect(),
		}
	}
}
