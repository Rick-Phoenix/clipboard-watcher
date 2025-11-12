//! Async stream of clipboard change events.
//!
//! Provides real-time clipboard monitoring through an async [`Stream`](futures::Stream) interface.
//!
//! The main part of this crate is [`ClipboardStream`].
//! This struct implements [`Stream`](futures::Stream).
//!
//! # Runtime
//! Internally, this crate spawns a small dedicated OS thread to listen for clipboard events.
//! The API itself is `Future`-based and does not depend on any specific async runtime,
//! so it works with [`tokio`](https://docs.rs/tokio), [`smol`](https://docs.rs/smol), or any runtime compatible with
//! [`futures`](::futures).
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
