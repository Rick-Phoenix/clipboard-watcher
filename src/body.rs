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
  RawImage(RawImage),
  PngImage {
    bytes: Vec<u8>,
    path: Option<PathBuf>,
  },
  FileList(Vec<PathBuf>),
  Custom {
    name: Arc<str>,
    data: Vec<u8>,
  },
}

impl Body {
  pub(crate) fn new_png(bytes: Vec<u8>, path: Option<PathBuf>) -> Self {
    if log::log_enabled!(log::Level::Debug) {
      if let Some(path) = &path {
        debug!(
          "Found PNG image. Size: {:.2}MB, Path: {}",
          bytes_to_mb(bytes.len()),
          path.display()
        );
      } else {
        debug!(
          "Found PNG image. Size: {:.2}MB, Path: None",
          bytes_to_mb(bytes.len())
        );
      };
    }

    Self::PngImage { bytes, path }
  }

  #[cfg(not(target_os = "linux"))]
  pub(crate) fn new_image(image: image::DynamicImage, path: Option<PathBuf>) -> Self {
    let rgb = image.into_rgb8();

    let (width, height) = rgb.dimensions();
    let image = RawImage {
      bytes: rgb.into_raw(),
      path,
      width,
      height,
    };

    if log::log_enabled!(log::Level::Debug) {
      image.log_info();
    }

    Self::RawImage(image)
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

/// An image from the clipboard, normalized to raw rgb8 bytes.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct RawImage {
  /// The rgb8 bytes of the image.
  pub bytes: Vec<u8>,
  /// The width of the image
  pub width: u32,
  /// The height of the image
  pub height: u32,
  /// The path to the image's file (if one can be detected).
  pub path: Option<PathBuf>,
}

impl RawImage {
  /// Checks whether the clipboard has a file path attached to it.
  pub fn has_path(&self) -> bool {
    self.path.is_some()
  }

  #[cfg(not(target_os = "linux"))]
  pub(crate) fn log_info(&self) {
    if let Some(path) = &self.path {
      debug!(
        "Found raw image. Size: {:.2}MB, Path: {}",
        bytes_to_mb(self.bytes.len()),
        path.display()
      );
    } else {
      debug!(
        "Found raw image. Size: {:.2}MB, Path: None",
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
