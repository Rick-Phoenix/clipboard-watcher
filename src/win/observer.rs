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
use log::{debug, error, info, trace};

use crate::{
  Body,
  body::BodySenders,
  error::{ClipboardError, ErrorWrapper},
  logging::bytes_to_mb,
  observer::Observer,
};

pub(super) struct WinObserver {
  stop: Arc<AtomicBool>,
  monitor: clipboard_win::Monitor,
  html_format: clipboard_win::formats::Html,
  png_format: NonZeroU32,
  custom_formats: HashMap<Arc<str>, NonZeroU32>,
  interval: Duration,
  max_size: Option<u32>,
}

// We use the error wrapper to trigger early exit in case a format is present but not valid, to avoid checking other formats
fn access_format(
  available_formats: &[u32],
  format_id: u32,
  max_bytes: Option<u32>,
) -> Result<(), ErrorWrapper> {
  match available_formats.contains(&format_id) {
    true => {
      match max_bytes {
        Some(max) => match clipboard_win::size(format_id) {
          Some(size) => {
            if max as usize > size.get() {
              Ok(())
            } else if size.get() == 0 {
              Err(ErrorWrapper::EmptyContent)
            } else {
              debug!(
                "Found content with {:.2}MB size, beyond maximum allowed size. Skipping it...",
                bytes_to_mb(size.get())
              );
              // Invalid side, we use an error to exit early later on
              Err(ErrorWrapper::SizeTooLarge)
            }
          }
          // Should be impossible
          None => Err(ErrorWrapper::FormatUnavailable),
        },
        None => Ok(()),
      }
    }
    false => Err(ErrorWrapper::FormatUnavailable),
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
    available_formats: &[u32],
    format_id: u32,
    max_bytes: Option<u32>,
  ) -> Result<Option<Vec<u8>>, ErrorWrapper> {
    use clipboard_win::formats;

    match access_format(available_formats, format_id, max_bytes)
      .is_ok()
      .then(|| clipboard_win::get(formats::RawData(format_id)).ok())
      .flatten()
    {
      Some(data) => Ok(Some(data)),
      None => Ok(None),
    }
  }

  pub(super) fn extract_image_bytes(
    &self,
    available_formats: &[u32],
  ) -> Result<Option<Vec<u8>>, ErrorWrapper> {
    use clipboard_win::formats;

    use crate::image::convert_dib_to_png;

    let max_size = self.max_size;

    if let Some(png_bytes) =
      Self::extract_clipboard_format(available_formats, self.png_format.get(), max_size)?
    {
      trace!("Loaded PNG from clipboard");

      Ok(Some(png_bytes))
    } else if let Some(bytes) =
      Self::extract_clipboard_format(available_formats, formats::CF_DIBV5, max_size)?
      && let Ok(png_bytes) =
        convert_dib_to_png(&bytes).ok_or(ErrorWrapper::ReadError(ClipboardError::ImageConversion))
    {
      trace!("Loaded DIBV5 from clipboard. Converting to PNG...");

      Ok(Some(png_bytes))
    } else if let Some(bytes) =
      Self::extract_clipboard_format(available_formats, formats::CF_DIB, max_size)?
      && let Ok(png_bytes) =
        convert_dib_to_png(&bytes).ok_or(ErrorWrapper::ReadError(ClipboardError::ImageConversion))
    {
      trace!("Loaded DIB from clipboard. Converting to PNG...");

      Ok(Some(png_bytes))
    } else {
      Ok(None)
    }
  }

  pub(super) fn extract_files_list(
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

    if let Some(image_bytes) = self.extract_image_bytes(&available_formats)? {
      let image_path = if let Some(mut files_list) = self.extract_files_list(&available_formats)?
        && files_list.len() == 1
      {
        let img_path = files_list.remove(0);

        Some(img_path)
      } else {
        None
      };

      Ok(Some(Body::new_image(image_bytes, image_path)))
    } else if let Some(mut files_list) = self.extract_files_list(&available_formats)? {
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
        let image_path = files_list.remove(0);

        Ok(Some(Body::new_image(png_bytes, Some(image_path))))
      } else {
        Ok(Some(Body::new_file_list(files_list)))
      }
    } else {
      let mut text = String::new();

      if self.html_format.read_clipboard(&mut text).is_ok() {
        Ok(Some(Body::new_html(text)))
      } else if let Ok(_num_bytes) = formats::Unicode.read_clipboard(&mut text) {
        Ok(Some(Body::new_text(text)))
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
