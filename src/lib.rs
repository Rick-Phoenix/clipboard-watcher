#![doc = include_str!("../README.md")]

use futures::{
  Stream,
  channel::mpsc::{self, Receiver, Sender},
};
use std::sync::{
  Arc, Mutex,
  atomic::{AtomicBool, AtomicUsize, Ordering},
};
use std::{
  collections::HashMap,
  path::PathBuf,
  pin::Pin,
  task::{Context, Poll},
  time::Duration,
};

mod body;
use body::*;
mod driver;
use driver::Driver;
pub mod error;
use error::*;

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
