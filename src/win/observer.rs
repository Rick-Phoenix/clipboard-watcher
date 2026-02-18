use clipboard_win::{
  Clipboard, EnumFormats, Getter, Monitor,
  formats::{self, Html},
  raw::format_name_big,
};
use image::DynamicImage;

use crate::*;

pub(crate) struct WinObserver<G: Gatekeeper = DefaultGatekeeper> {
  stop: Arc<AtomicBool>,
  monitor: Monitor,
  html_format: Html,
  png_format: u32,
  custom_formats: Formats,
  formats_cache: HashMap<u32, Arc<str>>,
  interval: Duration,
  max_size: Option<u32>,
  gatekeeper: G,
}

impl ClipboardContext<'_> {
  /// Attempts to extract the data for a particular [`Format`].
  #[cfg(windows)]
  #[must_use]
  #[inline]
  pub fn get_data(&self, format: &Format) -> Option<Vec<u8>> {
    clipboard_win::get(clipboard_win::formats::RawData(format.id)).ok()
  }
}

impl Formats {
  // We return None if the format is simply not present, and
  // use errors to early exit or for actual errors
  fn extract_clipboard_format(
    &self,
    format_id: u32,
    max_bytes: Option<u32>,
  ) -> Result<Option<Vec<u8>>, ErrorWrapper> {
    if self.contains_id(format_id) {
      if let Some(max) = max_bytes {
        match clipboard_win::size(format_id) {
          Some(size) => {
            if (max as usize) < size.get() {
              debug!(
                "Found content with {} size, beyond maximum allowed size. Skipping it...",
                HumanBytes(size.get())
              );
              // Invalid size, we use an error to exit early later on
              return Err(ErrorWrapper::SizeTooLarge);
            }
          }

          // Should be impossible given that the format
          // is already in the list, but we should trigger
          // an early exit regardless, as something went wrong
          None => return Err(ErrorWrapper::EmptyContent),
        };
      }

      let data = clipboard_win::get(formats::RawData(format_id))
        .map_err(|e| ClipboardError::ReadError(e.to_string()))?;

      if data.is_empty() {
        Err(ErrorWrapper::EmptyContent)
      } else {
        Ok(Some(data))
      }
    } else {
      // Format was not available at all
      Ok(None)
    }
  }

  fn extract_raw_image(&self, max_size: Option<u32>) -> Result<Option<DynamicImage>, ErrorWrapper> {
    let image_bytes =
      if let Some(bytes) = self.extract_clipboard_format(formats::CF_DIBV5, max_size)? {
        bytes
      } else if let Some(bytes) = self.extract_clipboard_format(formats::CF_DIB, max_size)? {
        bytes
      } else {
        return Ok(None);
      };

    let image = load_dib(&image_bytes)?;
    Ok(Some(image))
  }

  fn extract_files_list(&self) -> Result<Option<Vec<PathBuf>>, ErrorWrapper> {
    if self.contains_id(formats::FileList.into()) {
      let mut files_list: Vec<PathBuf> = Vec::new();
      if let Ok(_num_files) = formats::FileList.read_clipboard(&mut files_list) {
        if files_list.is_empty() {
          Err(ErrorWrapper::EmptyContent)
        } else {
          Ok(Some(files_list))
        }
      } else {
        // Can only happen if the clipboard changed in the meantime
        Ok(None)
      }
    } else {
      Ok(None)
    }
  }
}

impl<G: Gatekeeper> Observer for WinObserver<G> {
  fn observe(&mut self, body_senders: Arc<BodySenders>) {
    info!("Started monitoring the clipboard");

    while !self.stop.load(Ordering::Relaxed) {
      let monitor = &mut self.monitor;

      match monitor.try_recv() {
        Ok(true) => {
          match self.poll_clipboard() {
            Ok(Some(body)) => {
              body_senders.send_all(&Ok(Arc::new(body)));
            }
            Err(e) => {
              warn!("{e}");

              body_senders.send_all(&Err(e));
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

          body_senders.send_all(&Err(error));

          error!("Fatal error, terminating clipboard watcher");
          break;
        }
      }
    }
  }
}

impl<G: Gatekeeper> WinObserver<G> {
  #[inline(never)]
  #[cold]
  pub(crate) fn new(
    stop: Arc<AtomicBool>,
    monitor: Monitor,
    custom_format_names: Vec<Arc<str>>,
    interval: Option<Duration>,
    max_bytes: Option<u32>,
    gatekeeper: G,
  ) -> Result<Self, String> {
    let html_format = Html::new().ok_or("Failed to create html format identifier".to_string())?;

    let png_format = clipboard_win::register_format("PNG")
      .ok_or("Failed to create png format identifier".to_string())?;

    let mut custom_formats = Formats::default();
    let mut formats_cache: HashMap<u32, Arc<str>> = HashMap::new();

    for name in custom_format_names {
      if let Some(id) = clipboard_win::register_format(name.as_ref()) {
        formats_cache.insert(id.get(), name.clone());
        custom_formats.data.push(Format { id: id.get(), name });
      } else {
        return Err(format!("Failed to register custom format `{name}`"));
      }
    }

    Ok(Self {
      stop,
      monitor,
      html_format,
      png_format: png_format.get(),
      custom_formats,
      formats_cache,
      interval: interval.unwrap_or_else(|| Duration::from_millis(200)),
      max_size: max_bytes,
      gatekeeper,
    })
  }

  // Reads the clipboard and extracts the first matching format, following the priority list
  // Here we return None if we weren't able to read any format
  fn extract_clipboard_content(&mut self) -> Result<Option<Body>, ErrorWrapper> {
    let formats: Formats = EnumFormats::new()
      .filter_map(|id| {
        if let Some(name) = self.formats_cache.get(&id) {
          Some(Format {
            name: name.clone(),
            id,
          })
        } else {
          format_name_big(id).map(|name| {
            let name: Arc<str> = name.into();

            self.formats_cache.insert(id, name.clone());

            Format { name, id }
          })
        }
      })
      .collect();

    let ctx = ClipboardContext { formats: &formats };

    if !self.gatekeeper.check(ctx) {
      return Err(ErrorWrapper::UserSkipped);
    }

    let max_size = self.max_size;

    for format in self.custom_formats.iter() {
      if let Some(bytes) = formats.extract_clipboard_format(format.id, max_size)? {
        return Ok(Some(Body::new_custom(format.name.clone(), bytes)));
      }
    }

    if let Some(png_bytes) = formats.extract_clipboard_format(self.png_format, max_size)? {
      // Extract the image path if we have a list of files with a single item
      let image_path = formats
        .extract_files_list()?
        .filter(|list| list.len() == 1)
        .map(|mut files| files.remove(0));

      Ok(Some(Body::new_png(png_bytes, image_path)))
    } else if let Some(image) = formats.extract_raw_image(max_size)? {
      // Extract the image path if we have a list of files with a single item
      let image_path = formats
        .extract_files_list()?
        .filter(|list| list.len() == 1)
        .map(|mut files| files.remove(0));

      Ok(Some(Body::new_image(image, image_path)))
    } else if let Some(files_list) = formats.extract_files_list()? {
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

  // Opens the clipboard and calls the extractor, then handles the result
  fn poll_clipboard(&mut self) -> Result<Option<Body>, ClipboardError> {
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

      Err(ErrorWrapper::SizeTooLarge | ErrorWrapper::UserSkipped) => Ok(None),

      // Actual error
      Err(ErrorWrapper::ReadError(e)) => Err(e),

      // There was content but we could not read it
      Ok(None) => Err(ClipboardError::NoMatchingFormat),
    }
  }
}

// We use a result rather than a simple boolean to trigger early exits and reduce verbosity
const fn content_is_not_empty(content: &str) -> Result<bool, ErrorWrapper> {
  if content.is_empty() {
    Err(ErrorWrapper::EmptyContent)
  } else {
    Ok(true)
  }
}

fn load_dib(bytes: &[u8]) -> Result<DynamicImage, ClipboardError> {
  use std::io::Cursor;

  use image::{DynamicImage, codecs::bmp::BmpDecoder};

  let cursor = Cursor::new(bytes);

  let decoder = BmpDecoder::new_without_file_header(cursor)
    .map_err(|e| ClipboardError::ReadError(format!("Failed to load DIB image: {e}")))?;

  DynamicImage::from_decoder(decoder)
    .map_err(|e| ClipboardError::ReadError(format!("Failed to load DIB image: {e}")))
}
