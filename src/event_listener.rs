use std::{
  sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
  },
  time::Duration,
};

use futures::channel::mpsc;

use crate::error::ClipboardError;
use crate::{
  body::{BodySenders, BodySendersDropHandle},
  driver::Driver,
  stream::StreamId,
  ClipboardStream,
};

/// Clipboard event change listener.
///
/// Listen for clipboard change events and notifies [`ClipboardStream`].
pub struct ClipboardEventListener {
  driver: Option<Driver>,
  body_senders: Arc<BodySenders>,
  id: AtomicUsize,
}

pub struct ClipboardEventListenerBuilder {
  pub(crate) interval: Option<Duration>,
  pub(crate) custom_formats: Vec<Arc<str>>,
  pub(crate) max_image_bytes: Option<usize>,
  pub(crate) max_bytes: Option<usize>,
}

impl ClipboardEventListenerBuilder {
  pub fn interval(mut self, duration: Duration) -> Self {
    self.interval = Some(duration);
    self
  }

  pub fn with_custom_formats<I, S>(mut self, formats: I) -> Self
  where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
  {
    self.custom_formats = formats.into_iter().map(|s| s.as_ref().into()).collect();
    self
  }

  pub fn max_image_size(mut self, max_bytes: usize) -> Self {
    self.max_image_bytes = Some(max_bytes);
    self
  }

  pub fn max_size(mut self, max_bytes: usize) -> Self {
    self.max_bytes = Some(max_bytes);
    self
  }

  pub fn spawn(self) -> Result<ClipboardEventListener, ClipboardError> {
    let body_senders = Arc::new(BodySenders::new());

    let driver = Driver::new(
      body_senders.clone(),
      self.interval,
      self.custom_formats,
      self.max_image_bytes,
      self.max_bytes,
    )?;
    Ok(ClipboardEventListener {
      driver: Some(driver),
      body_senders,
      id: AtomicUsize::new(0),
    })
  }
}

impl ClipboardEventListener {
  pub fn builder() -> ClipboardEventListenerBuilder {
    ClipboardEventListenerBuilder {
      interval: None,
      custom_formats: vec![],
      max_image_bytes: None,
      max_bytes: None,
    }
  }

  /// Creates a new [`ClipboardEventListener`] that monitors clipboard changes in a dedicated OS thread.
  pub fn spawn() -> Result<Self, ClipboardError> {
    Self::builder().spawn()
  }

  /// Creates a [`ClipboardStream`] for receiving clipboard change items as [`Body`].
  ///
  /// # Buffer size
  /// This method takes a buffer size. Items are buffered when not received immediately.
  /// The actual buffer capacity is `buf_size + 2`, where the extra `2` accounts for the
  /// number of internal senders used by the library.
  ///
  /// # Example
  /// ```
  /// # use clipboard_stream::{Body, ClipboardEventListener, ClipboardStream};
  /// # #[tokio::main]
  /// # async fn main() {
  ///     let mut event_listener = ClipboardEventListener::spawn();
  ///
  ///     let buf_size = 32;
  ///     let stream = event_listener.new_stream(buf_size);
  /// # }
  /// ```
  /// [`Body`]: crate::Body
  pub fn new_stream(&mut self, buffer: usize) -> ClipboardStream {
    let (tx, rx) = mpsc::channel(buffer);
    let id = StreamId(self.id.fetch_add(1, Ordering::Relaxed));
    self.body_senders.register(id.clone(), tx);
    let drop_handle = BodySendersDropHandle::new(self.body_senders.clone());

    ClipboardStream {
      id,
      body_rx: Box::pin(rx),
      drop_handle,
    }
  }
}

impl Drop for ClipboardEventListener {
  fn drop(&mut self) {
    drop(self.driver.take())
  }
}
