use std::{
  sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
  },
  time::Duration,
};

use futures::channel::mpsc;

use crate::{
  body::{BodySenders, BodySendersDropHandle},
  driver::Driver,
  error::InitializationError,
  stream::StreamId,
  ClipboardStream,
};

/// Clipboard event change listener.
///
/// Listen for clipboard change events and notifies [`ClipboardStream`].
///
/// Use the [`builder`](ClipboardEventListener::builder) method to customize the options for the listener.
pub struct ClipboardEventListener {
  driver: Option<Driver>,
  body_senders: Arc<BodySenders>,
  id: AtomicUsize,
}

/// The builder for the [`ClipboardEventListener`]. It can be used to specify more customized options such as the polling interval, or a list of custom clipboard formats.
pub struct ClipboardEventListenerBuilder {
  pub(crate) interval: Option<Duration>,
  pub(crate) custom_formats: Vec<Arc<str>>,
  pub(crate) max_bytes: Option<u32>,
}

impl ClipboardEventListenerBuilder {
  /// Defines the polling interval for the clipboard monitoring. If unset, it defaults to 200 milliseconds.
  pub fn interval(mut self, duration: Duration) -> Self {
    self.interval = Some(duration);
    self
  }

  /// Adds a list of custom clipboard formats to the list of formats to monitor.
  ///
  /// In cases where a clipboard item can match more than one format in this list, only the first will be selected.
  ///
  /// Custom formats are always extracted with a higher priority than normal formats. See [`Body`](crate::Body) for more information about the extraction priority.
  pub fn with_custom_formats<I, S>(mut self, formats: I) -> Self
  where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
  {
    self.custom_formats = formats.into_iter().map(|s| s.as_ref().into()).collect();
    self
  }

  /// Sets a maximum allowed size limit. It only applies to custom formats or to images, but not to text-based formats like html or plain text.
  ///
  /// The various platform-specific implementations will use a performant method to check the size of the clipboard items without loading their content into a buffer, so this can be useful to avoid processing large files.
  ///
  /// The linux implementation has a fallible mechanism for getting the size of the clipboard item in a performant way, using a less performant method as a fallback.
  pub fn max_size(mut self, max_bytes: u32) -> Self {
    self.max_bytes = Some(max_bytes);
    self
  }

  /// Spawns the [`ClipboardEventListener`].
  pub fn spawn(self) -> Result<ClipboardEventListener, InitializationError> {
    let body_senders = Arc::new(BodySenders::new());

    let driver = Driver::new(
      body_senders.clone(),
      self.interval,
      self.custom_formats,
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
  /// Creates an instance of a [`ClipboardEventListenerBuilder`], which can be used to specify custom options for the listener.
  pub fn builder() -> ClipboardEventListenerBuilder {
    ClipboardEventListenerBuilder {
      interval: None,
      custom_formats: vec![],
      max_bytes: None,
    }
  }

  /// Creates a new [`ClipboardEventListener`] that monitors clipboard changes in a dedicated OS thread.
  ///
  /// Uses all of the default options.
  pub fn spawn() -> Result<Self, InitializationError> {
    Self::builder().spawn()
  }

  /// Creates a [`ClipboardStream`] for receiving clipboard change items as [`Body`].
  ///
  /// # Buffer size
  /// This method takes a buffer size. Items are buffered when not received immediately.
  /// The actual buffer capacity is `buf_size + 2`, where the extra `2` accounts for the
  /// number of internal senders used by the library.
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
