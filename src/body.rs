use std::{
  collections::HashMap,
  path::PathBuf,
  sync::{Arc, Mutex},
};

use futures::channel::mpsc::Sender;
use log::{debug, error};

use crate::{error::ClipboardResult, logging::bytes_to_mb, stream::StreamId};

/// The content extracted from the clipboard.
///
/// To avoid extracting all types of content each time, only one of them is chosen, in the following order of priority:
///
/// - Custom formats (in the order they are given, if present)
/// - Image (see [`ClipboardImage`] for more info)
/// - File list
/// - HTML
/// - Plain text
///
/// When a clipboard item can fit more than one of these formats, only the one with the highest priority will be chosen.
///
/// When selecting a single image as a file, the item will be processed as an Image (with a defined file path), falling back to a single-item file list in case the processing of the image goes wrong.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(untagged))]
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Body {
  Html(String),
  PlainText(String),
  Image(ClipboardImage),
  FileList(Vec<PathBuf>),
  Custom { name: Arc<str>, data: Vec<u8> },
}

impl Body {
  pub(crate) fn new_image(bytes: Vec<u8>, path: Option<PathBuf>) -> Self {
    let image = ClipboardImage { bytes, path };

    if log::log_enabled!(log::Level::Debug) {
      image.log_info();
    }

    Self::Image(image)
  }

  pub(crate) fn new_custom(name: Arc<str>, data: Vec<u8>) -> Self {
    if log::log_enabled!(log::Level::Debug) {
      debug!(
        "Found content with custom format `{name}`. Size: {:.2}MB",
        bytes_to_mb(data.len())
      );
    }

    Self::Custom { name, data }
  }

  pub(crate) fn new_file_list(files: Vec<PathBuf>) -> Self {
    if log::log_enabled!(log::Level::Debug) {
      debug!("Found file list with {} elements: {files:?}", files.len());
    }

    Self::FileList(files)
  }

  pub(crate) fn new_html(html: String) -> Self {
    if log::log_enabled!(log::Level::Debug) {
      debug!("Found html content");
    }

    Self::Html(html)
  }

  pub(crate) fn new_text(text: String) -> Self {
    if log::log_enabled!(log::Level::Debug) {
      debug!("Found text content");
    }

    Self::PlainText(text)
  }
}

/// An image from the clipboard, normalized to the PNG format.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ClipboardImage {
  /// The bytes that compose the image, encoded in the PNG format.
  pub bytes: Vec<u8>,
  /// The path to the image's file (if one can be detected).
  pub path: Option<PathBuf>,
}

impl ClipboardImage {
  /// Checks whether the clipboard has a file path attached to it.
  pub fn has_path(&self) -> bool {
    self.path.is_some()
  }

  pub(crate) fn log_info(&self) {
    if let Some(path) = &self.path {
      debug!(
        "Found image. Size: {:.2}MB, Path: {}",
        bytes_to_mb(self.bytes.len()),
        path.display()
      );
    } else {
      debug!(
        "Found image. Size: {:.2}MB, Path: None",
        bytes_to_mb(self.bytes.len())
      );
    }
  }
}

#[derive(Debug)]
pub(crate) struct BodySenders {
  senders: Mutex<HashMap<StreamId, Sender<ClipboardResult>>>,
}

impl BodySenders {
  pub(crate) fn new() -> Self {
    BodySenders {
      senders: Mutex::default(),
    }
  }

  /// Register Sender that was specified [`StreamId`].
  pub(crate) fn register(&self, id: StreamId, tx: Sender<ClipboardResult>) {
    let mut guard = self.senders.lock().unwrap();
    guard.insert(id, tx);
  }

  /// Close channel and unregister sender that was specified [`StreamId`]
  fn unregister(&self, id: &StreamId) {
    let mut guard = self.senders.lock().unwrap();
    guard.remove(id);
  }

  pub(crate) fn send_all(&self, result: ClipboardResult) {
    let mut senders = self.senders.lock().unwrap();

    for sender in senders.values_mut() {
      match sender.try_send(result.clone()) {
        Ok(_) => {}
        Err(e) => error!("Failed to send the clipboard data: {e}"),
      };
    }
  }
}

/// Handler for Cleaning up buffer(channel).
///
/// Close channel and unregister a specified [`StreamId`] of sender.
#[derive(Debug)]
pub(crate) struct BodySendersDropHandle(Arc<BodySenders>);

impl BodySendersDropHandle {
  pub(crate) fn new(senders: Arc<BodySenders>) -> Self {
    BodySendersDropHandle(senders)
  }

  pub(crate) fn drop(&self, id: &StreamId) {
    self.0.unregister(id);
  }
}
