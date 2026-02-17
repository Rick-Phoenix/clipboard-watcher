#![doc = include_str!("../README.md")]

use futures::{
  Stream,
  channel::mpsc::{self, Receiver, Sender},
};
use log::{debug, error};
use std::sync::{
  Arc, Mutex,
  atomic::{AtomicBool, AtomicUsize, Ordering},
};
use std::{
  collections::HashMap,
  path::PathBuf,
  pin::Pin,
  task::{Context, Poll},
  thread::JoinHandle,
  time::Duration,
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

mod observer;
mod stream;
pub use stream::*;

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(windows)]
mod win;
