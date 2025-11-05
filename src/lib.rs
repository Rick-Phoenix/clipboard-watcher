//! Async stream of clipboard change events.
//!
//! Provides real-time clipboard monitoring through an async [`Stream`] interface.
//!
//! The main part of this crate is [`ClipboardStream`].
//! This struct implements [`Stream`].
//!
//! # Example
//! The following example shows how to receive clipboard items:
//!
//! ```no_run
//! use clipboard_stream::{ClipboardEventListener, Body};
//! use futures::stream::StreamExt;
//!
//! #[tokio::main]
//! async fn main() {
//!     // Spawn a clipboard event listener
//!     let mut event_listener = ClipboardEventListener::spawn();
//!
//!     // Create a new stream
//!     let mut stream = event_listener.new_stream(32);
//!
//!     while let Some(body) = stream.next().await {
//!         if let Body::Utf8String(text) = body {
//!             println!("{}", text);
//!         }
//!     }
//! }
//! ```
//!
//! # Runtime
//! Internally, this crate spawns a small dedicated OS thread to listen for clipboard events.
//! The API itself is `Future`-based and does not depend on any specific async runtime,
//! so it works with [`tokio`](https://docs.rs/tokio), [`smol`](https://docs.rs/smol), or any runtime compatible with
//! [`futures`](https://docs.rs/futures).
//!
//! # Platforms
//! - macOS
//!
//! Currently supported on **macOS only**. Windows support is planned for a future release.
//!
//! [`Stream`]: https://docs.rs/futures/latest/futures/stream/trait.Stream.html
//! [`ClipboardStream`]: crate::stream::ClipboardStream
mod body;
mod driver;
pub mod error;
mod event_listener;
pub(crate) mod image;
#[cfg(target_os = "macos")]
mod macos;
mod observer;
mod stream;
#[cfg(windows)]
mod win;

pub use stream::{ClipboardStream, StreamId};

pub use crate::{
  body::{Body, MimeType},
  event_listener::ClipboardEventListener,
};
