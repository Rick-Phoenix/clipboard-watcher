use std::{
  collections::HashMap,
  num::NonZeroU32,
  path::PathBuf,
  sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
  },
  time::Duration,
};

use clipboard_win::{formats, Clipboard, Getter};
use image::DynamicImage;
use log::{debug, error, info, trace, warn};

use crate::{
  body::BodySenders,
  error::{ClipboardError, ErrorWrapper},
  logging::HumanBytes,
  observer::Observer,
  Body,
};

pub(crate) struct WinObserver {
  stop: Arc<AtomicBool>,
  monitor: clipboard_win::Monitor,
  html_format: clipboard_win::formats::Html,
  png_format: NonZeroU32,
  custom_formats: HashMap<Arc<str>, NonZeroU32>,
  interval: Duration,
  max_size: Option<u32>,
}

impl WinObserver {
  pub(crate) fn new(
    stop: Arc<AtomicBool>,
    monitor: clipboard_win::Monitor,
    custom_formats: Vec<Arc<str>>,
    interval: Option<Duration>,
    max_bytes: Option<u32>,
  ) -> Result<Self, String> {
    let html_format = clipboard_win::formats::Html::new()
      .ok_or("Failed to create html format identifier".to_string())?;
    let png_format = clipboard_win::register_format("PNG")
      .ok_or("Failed to create png format identifier".to_string())?;

    let custom_formats_map: Result<HashMap<Arc<str>, NonZeroU32>, String> = custom_formats
      .into_iter()
      .map(|name| {
        if let Some(id) = clipboard_win::register_format(name.as_ref()) {
          Ok((name, id))
        } else {
          Err(format!("Failed to register custom format `{name}`"))
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
    available_formats: &[u32],
    format_id: u32,
    max_bytes: Option<u32>,
  ) -> Result<Option<Vec<u8>>, ErrorWrapper> {
    use clipboard_win::formats;

    match can_access_format(available_formats, format_id, max_bytes)? {
      true => {
        let data = clipboard_win::get(formats::RawData(format_id))
          .map_err(|e| ClipboardError::ReadError(e.to_string()))?;

        if data.is_empty() {
          Err(ErrorWrapper::EmptyContent)
        } else {
          Ok(Some(data))
        }
      }
      false => Ok(None),
    }
  }

  fn extract_png(&self, available_formats: &[u32]) -> Result<Option<Vec<u8>>, ErrorWrapper> {
    Self::extract_clipboard_format(available_formats, self.png_format.get(), self.max_size)
  }

  fn extract_raw_image(
    &self,
    available_formats: &[u32],
  ) -> Result<Option<DynamicImage>, ErrorWrapper> {
    use clipboard_win::formats;

    let max_size = self.max_size;

    if let Some(bytes) =
      Self::extract_clipboard_format(available_formats, formats::CF_DIBV5, max_size)?
    {
      let image = load_dib(&bytes)?;

      Ok(Some(image))
    } else if let Some(bytes) =
      Self::extract_clipboard_format(available_formats, formats::CF_DIB, max_size)?
    {
      let image = load_dib(&bytes)?;

      Ok(Some(image))
    } else {
      Ok(None)
    }
  }

  fn extract_files_list(
    &self,
    available_formats: &[u32],
  ) -> Result<Option<Vec<PathBuf>>, ErrorWrapper> {
    match available_formats.contains(&formats::FileList.into()) {
      true => {
        let mut files_list: Vec<PathBuf> = Vec::new();
        if let Ok(_num_files) = formats::FileList.read_clipboard(&mut files_list) {
          if files_list.is_empty() {
            Err(ErrorWrapper::EmptyContent)
          } else {
            Ok(Some(files_list))
          }
        } else {
          // Technically impossible
          Ok(None)
        }
      }
      false => Ok(None),
    }
  }

  fn extract_clipboard_content(&self) -> Result<Option<Body>, ErrorWrapper> {
    let available_formats: Vec<u32> = clipboard_win::EnumFormats::new().collect();

    let max_size = self.max_size;

    for (name, id) in self.custom_formats.iter() {
      if let Some(bytes) = Self::extract_clipboard_format(&available_formats, id.get(), max_size)? {
        return Ok(Some(Body::new_custom(name.clone(), bytes)));
      }
    }

    if let Some(png_bytes) = self.extract_png(&available_formats)? {
      // Extract the image path if we have a list of files with a single item
      let image_path = self
        .extract_files_list(&available_formats)?
        .filter(|list| list.len() == 1)
        .map(|mut files| files.remove(0));

      Ok(Some(Body::new_png(png_bytes, image_path)))
    } else if let Some(image) = self.extract_raw_image(&available_formats)? {
      // Extract the image path if we have a list of files with a single item
      let image_path = self
        .extract_files_list(&available_formats)?
        .filter(|list| list.len() == 1)
        .map(|mut files| files.remove(0));

      Ok(Some(Body::new_image(image, image_path)))
    } else if let Some(files_list) = self.extract_files_list(&available_formats)? {
      Ok(Some(Body::new_file_list(files_list)))
    } else {
      let mut text = String::new();

      if self.html_format.read_clipboard(&mut text).is_ok() && content_is_not_empty(&text)? {
        Ok(Some(Body::new_html(text)))
      } else if let Ok(_num_bytes) = formats::Unicode.read_clipboard(&mut text)
        && content_is_not_empty(&text)?
      {
        Ok(Some(Body::new_text(text)))
      } else {
        Ok(None)
      }
    }
  }

  fn get_clipboard_content(&self) -> Result<Option<Body>, ClipboardError> {
    let _clipboard =
      Clipboard::new_attempts(10).map_err(|e| ClipboardError::ReadError(e.to_string()))?;

    match self.extract_clipboard_content() {
      // Found content
      Ok(Some(content)) => Ok(Some(content)),

      // Non-fatal errors, we just return None
      Err(ErrorWrapper::EmptyContent) => {
        trace!("Found empty content. Skipping it...");
        Ok(None)
      }

      Err(ErrorWrapper::SizeTooLarge) => Ok(None),

      Err(ErrorWrapper::FormatUnavailable) => Ok(None),

      // Actual error
      Err(ErrorWrapper::ReadError(e)) => Err(e),

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
              warn!("{e}");

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

// We use a result rather than a simple boolean to trigger early exits and reduce verbosity
fn content_is_not_empty(content: &str) -> Result<bool, ErrorWrapper> {
  if content.is_empty() {
    Err(ErrorWrapper::EmptyContent)
  } else {
    Ok(true)
  }
}

// We use the error wrapper to trigger early exit in case a format is present but not valid, to avoid checking other formats
fn can_access_format(
  available_formats: &[u32],
  format_id: u32,
  max_bytes: Option<u32>,
) -> Result<bool, ErrorWrapper> {
  match available_formats.contains(&format_id) {
    true => {
      match max_bytes {
        Some(max) => match clipboard_win::size(format_id) {
          Some(size) => {
            if max as usize > size.get() {
              Ok(true)
            } else if size.get() == 0 {
              Err(ErrorWrapper::EmptyContent)
            } else {
              debug!(
                "Found content with {} size, beyond maximum allowed size. Skipping it...",
                HumanBytes(size.get())
              );
              // Invalid size, we use an error to exit early later on
              Err(ErrorWrapper::SizeTooLarge)
            }
          }
          // Should be impossible given that the format
          // is already in the list, but we should trigger
          // an early exit regardless, as something went wrong
          None => Err(ErrorWrapper::FormatUnavailable),
        },
        None => Ok(true),
      }
    }
    false => Ok(false),
  }
}

fn load_dib(bytes: &[u8]) -> Result<DynamicImage, ClipboardError> {
  use std::io::Cursor;

  use image::{codecs::bmp::BmpDecoder, DynamicImage};

  let cursor = Cursor::new(bytes);

  let decoder = BmpDecoder::new_without_file_header(cursor)
    .map_err(|e| ClipboardError::ReadError(format!("Failed to load DIB image: {e}")))?;

  DynamicImage::from_decoder(decoder)
    .map_err(|e| ClipboardError::ReadError(format!("Failed to load DIB image: {e}")))
}
