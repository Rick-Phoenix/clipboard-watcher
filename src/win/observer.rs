use std::{
  collections::HashMap,
  num::NonZeroU32,
  path::PathBuf,
  sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
  },
  time::Duration,
};

use clipboard_win::{Clipboard, Getter, formats};
use log::{debug, error, info};

use crate::{
  Body,
  body::{BodySenders, ClipboardImage},
  error::{ClipboardError, ExtractionError},
  observer::Observer,
};

pub(super) struct WinObserver {
  stop: Arc<AtomicBool>,
  monitor: clipboard_win::Monitor,
  html_format: Option<clipboard_win::formats::Html>,
  png_format: Option<NonZeroU32>,
  custom_formats: HashMap<Arc<str>, NonZeroU32>,
  interval: Duration,
  max_size: Option<u32>,
}

struct FormatTooLarge;

impl From<FormatTooLarge> for ExtractionError {
  fn from(_: FormatTooLarge) -> Self {
    Self::SizeTooLarge
  }
}

// To allow early exits later on, we use Err for cases when the format has been found but is over the allowed size
// (so that other formats are not checked needlessly), and use a normal boolean to signal presence
fn format_is_valid(format_id: u32, max_bytes: Option<u32>) -> Result<bool, FormatTooLarge> {
  match max_bytes {
    Some(max) => match clipboard_win::size(format_id) {
      Some(size) => {
        if max as usize > size.get() {
          Ok(true)
        } else {
          // Invalid side, we use an error to exit early later on
          Err(FormatTooLarge)
        }
      }
      // Format is not present at all
      None => Ok(false),
    },
    // Cannot say, we try to access it directly later on
    None => Ok(true),
  }
}

impl WinObserver {
  pub(super) fn new(
    stop: Arc<AtomicBool>,
    monitor: clipboard_win::Monitor,
    custom_formats: Vec<Arc<str>>,
    interval: Option<Duration>,
    max_bytes: Option<u32>,
  ) -> Result<Self, String> {
    let html_format = clipboard_win::formats::Html::new();
    let png_format = clipboard_win::register_format("PNG");

    let custom_formats_map: Result<HashMap<Arc<str>, NonZeroU32>, String> = custom_formats
      .into_iter()
      .map(|name| {
        if let Some(id) = clipboard_win::register_format(name.as_ref()) {
          Ok((name, id))
        } else {
          Err(format!("Failed to register custom clipboard type `{name}`"))
        }
      })
      .collect();

    Ok(WinObserver {
      stop,
      monitor,
      html_format,
      png_format,
      custom_formats: custom_formats_map?,
      interval: interval.unwrap_or_else(|| Duration::from_millis(200)),
      max_size: max_bytes,
    })
  }

  fn extract_clipboard_format(
    format_id: u32,
    max_bytes: Option<u32>,
  ) -> Result<Option<Vec<u8>>, ExtractionError> {
    use clipboard_win::formats;

    match format_is_valid(format_id, max_bytes)?
      .then(|| clipboard_win::get(formats::RawData(format_id)).ok())
      .flatten()
    {
      Some(data) => Ok(Some(data)),
      None => Ok(None),
    }
  }

  pub(super) fn extract_image_bytes(&self) -> Result<Option<Vec<u8>>, ExtractionError> {
    use clipboard_win::formats;

    use crate::image::convert_dib_to_png;

    let max_size = self.max_size;

    if let Some(png_code) = self.png_format
      && let Some(png_bytes) = Self::extract_clipboard_format(png_code.get(), max_size)?
    {
      debug!("Loaded png from clipboard");
      Ok(Some(png_bytes))
    } else if let Some(bytes) = Self::extract_clipboard_format(formats::CF_DIBV5, max_size)?
      && let Ok(png_bytes) = convert_dib_to_png(&bytes).ok_or(ExtractionError::ConversionError)
    {
      debug!("Loaded DIBV5 from clipboard. Converting to PNG...");

      Ok(Some(png_bytes))
    } else if let Some(bytes) = Self::extract_clipboard_format(formats::CF_DIB, max_size)?
      && let Ok(png_bytes) = convert_dib_to_png(&bytes).ok_or(ExtractionError::ConversionError)
    {
      debug!("Loaded DIB from clipboard. Converting to PNG...");

      Ok(Some(png_bytes))
    } else {
      Ok(None)
    }
  }

  pub(super) fn extract_files_list(&self) -> Result<Option<Vec<PathBuf>>, ExtractionError> {
    match format_is_valid(formats::FileList.into(), self.max_size)? {
      true => {
        let mut files_list: Vec<PathBuf> = Vec::new();
        if let Ok(_num_files) = formats::FileList.read_clipboard(&mut files_list) {
          if files_list.is_empty() {
            Err(ExtractionError::EmptyContent)
          } else {
            debug!("Found file list");
            Ok(Some(files_list))
          }
        } else {
          Ok(None)
        }
      }
      false => Ok(None),
    }
  }

  fn extract_clipboard_content(&self) -> Result<Option<Body>, ExtractionError> {
    let max_size = self.max_size;

    for (name, id) in self.custom_formats.iter() {
      if let Some(bytes) = Self::extract_clipboard_format(id.get(), max_size)? {
        debug!("Found content with custom format `{name}`");

        return Ok(Some(Body::Custom {
          name: name.clone(),
          data: bytes,
        }));
      }
    }

    if let Some(image_bytes) = self.extract_image_bytes()? {
      let image_path = if let Some(mut files_list) = self.extract_files_list()?
        && files_list.len() == 1
      {
        Some(files_list.remove(0))
      } else {
        None
      };

      Ok(Some(Body::Image(ClipboardImage {
        bytes: image_bytes,
        path: image_path,
      })))
    } else if let Some(mut files_list) = self.extract_files_list()? {
      // If there is just one file in the list and it's an image,
      // we save it directly as an image
      use crate::image::{convert_file_to_png, file_is_image};

      // We check if there is just one file
      if files_list.len() == 1
        && let Some(path) = files_list.first()

        // Then, if it's an image
        && file_is_image(path)
        // Then, if the size is within the allowed range
        && max_size.is_none_or(|max| path.metadata().is_ok_and(|metadata| max as u64 > metadata.len()))
        // Then, if the bytes are readable and the conversion to png is successful
        && let Some(png_bytes) = convert_file_to_png(path)
      //
      // Only if all of these are true, we save it as an image
      {
        debug!("Found file path with image format. Processing it as an image...");

        let image_path = files_list.remove(0);

        Ok(Some(Body::Image(ClipboardImage {
          bytes: png_bytes,
          path: Some(image_path),
        })))
      } else {
        Ok(Some(Body::FileList(files_list)))
      }
    } else {
      let mut text = String::new();

      if let Some(html_parser) = self.html_format
        && let Ok(_) = html_parser.read_clipboard(&mut text)
      {
        debug!("Extracted HTML content from clipboard");

        Ok(Some(Body::Html(text)))
      } else if let Ok(_num_bytes) = formats::Unicode.read_clipboard(&mut text) {
        debug!("Extracted plain text from clipboard");

        Ok(Some(Body::PlainText(text)))
      } else {
        Ok(None)
      }
    }
  }

  pub(super) fn get_clipboard_content(&self) -> Result<Option<Body>, ClipboardError> {
    let _clipboard =
      Clipboard::new_attempts(10).map_err(|e| ClipboardError::ReadError(e.to_string()))?;

    match self.extract_clipboard_content() {
      // Found content
      Ok(Some(content)) => Ok(Some(content)),
      // Non-fatal errors, we just return None
      Err(ExtractionError::EmptyContent) => {
        debug!("Found empty content, skipping it...");
        Ok(None)
      }
      Err(ExtractionError::SizeTooLarge) => {
        debug!("Found content beyond allowed size, skipping it...");
        Ok(None)
      }

      // Actual error, we send it
      Err(ExtractionError::ConversionError) => Err(ClipboardError::ImageConversion),
      // There was content but we could not read it
      Ok(None) => Err(ClipboardError::NoMatchingFormat),
    }
  }
}

impl Observer for WinObserver {
  fn observe(&mut self, body_senders: Arc<BodySenders>) {
    info!("Started monitoring the clipboard");

    while !self.stop.load(Ordering::Relaxed) {
      let monitor = &mut self.monitor;

      match monitor.try_recv() {
        Ok(true) => {
          match self.get_clipboard_content() {
            Ok(Some(body)) => {
              body_senders.send_all(Ok(Arc::new(body)));
            }
            Err(e) => {
              error!("{e}");

              body_senders.send_all(Err(e));
            }
            // Found content but ignored it (empty or too large)
            Ok(None) => {}
          };
        }
        Ok(false) => {
          // No event, waiting
          std::thread::sleep(self.interval);
        }
        Err(e) => {
          let error = ClipboardError::MonitorFailed(e.to_string());

          error!("{error}");

          body_senders.send_all(Err(error));

          error!("Fatal error, terminating clipboard watcher");
          break;
        }
      }
    }
  }
}
