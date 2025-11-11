use std::{
  path::PathBuf,
  sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
  },
  time::Duration,
};

use log::{debug, error, info, trace};
use objc2::{
  rc::{autoreleasepool, Retained},
  ClassType,
};
use objc2_app_kit::{
  NSPasteboard, NSPasteboardType, NSPasteboardTypeHTML, NSPasteboardTypePNG,
  NSPasteboardTypeString, NSPasteboardTypeTIFF, NSPasteboardURLReadingFileURLsOnlyKey,
};
use objc2_foundation::{NSArray, NSData, NSDictionary, NSNumber, NSString, NSURL};

use crate::{
  body::*,
  error::{ClipboardError, ErrorWrapper},
  image::*,
  logging::*,
  observer::Observer,
};

pub(crate) struct OSXObserver {
  stop: Arc<AtomicBool>,
  pasteboard: Retained<NSPasteboard>,
  interval: Duration,
  custom_formats: Vec<Arc<str>>,
  max_size: Option<u32>,
}

impl OSXObserver {
  pub(super) fn new(
    stop: Arc<AtomicBool>,
    interval: Option<Duration>,
    custom_formats: Vec<Arc<str>>,
    max_size: Option<u32>,
  ) -> Self {
    let pasteboard = unsafe { NSPasteboard::generalPasteboard() };

    OSXObserver {
      stop,
      pasteboard,
      interval: interval.unwrap_or_else(|| std::time::Duration::from_millis(200)),
      custom_formats,
      max_size,
    }
  }
}

impl Observer for OSXObserver {
  fn observe(&mut self, body_senders: Arc<BodySenders>) {
    let mut last_count = self.get_change_count();

    let interval = self.interval;

    info!("Started monitoring the clipboard");

    while !self.stop.load(Ordering::Relaxed) {
      std::thread::sleep(interval);

      let change_count = self.get_change_count();

      if change_count != last_count {
        last_count = change_count;

        match self.get_clipboard_content() {
          Ok(Some(content)) => body_senders.send_all(Ok(Arc::new(content))),
          Err(e) => {
            error!("{e}");
            body_senders.send_all(Err(e));
          }
          // Found content but ignored it (empty or beyond allowed size)
          Ok(None) => {}
        }
      }
    }
  }
}

impl OSXObserver {
  pub(crate) fn get_change_count(&self) -> isize {
    unsafe { self.pasteboard.changeCount() }
  }

  fn extract_clipboard_format(
    &self,
    format_type: &NSPasteboardType,
    max_size: Option<u32>,
  ) -> Result<Option<Vec<u8>>, ErrorWrapper> {
    autoreleasepool(|_| {
      let data_obj: Option<Retained<NSData>> = unsafe { self.pasteboard.dataForType(format_type) };

      match data_obj {
        Some(data) => {
          let size = data.len();
          if size == 0 {
            // Found content but it was empty, trigger early exit
            return Err(ErrorWrapper::EmptyContent);
          }

          // Check the size limit. If exceeded, return Err to signal an early exit.
          if let Some(limit) = max_size {
            if size > limit as usize {
              debug!(
                "Found content with {:.2}MB size, beyond maximum allowed size. Skipping it...",
                bytes_to_mb(size)
              );

              return Err(ErrorWrapper::SizeTooLarge);
            }
          }

          // Size is okay, copy the data to a Rust Vec.
          Ok(Some(data.to_vec()))
        }
        None => Ok(None), // Format was not present.
      }
    })
  }

  pub(crate) fn extract_files_list(&self) -> Result<Option<Vec<PathBuf>>, ErrorWrapper> {
    let files = autoreleasepool(|_| {
      let class_array = NSArray::from_slice(&[NSURL::class()]);
      let options = NSDictionary::from_slices(
        &[unsafe { NSPasteboardURLReadingFileURLsOnlyKey }],
        &[NSNumber::new_bool(true).as_ref()],
      );

      let objects = unsafe {
        self
          .pasteboard
          .readObjectsForClasses_options(&class_array, Some(&options))
      };

      objects.map(|array| {
        array
          .iter()
          .filter_map(|obj| {
            obj.downcast::<NSURL>().ok().and_then(|url| {
              if unsafe { url.isFileURL() } {
                unsafe { url.path() }.map(|p| PathBuf::from(p.to_string()))
              } else {
                None
              }
            })
          })
          .collect::<Vec<_>>()
      })
    });

    match files {
      Some(files) if !files.is_empty() => Ok(Some(files)),
      // Macos api returns an empty list if no matching objects
      // were found, but it doesn't mean the format was matched
      // so we must not trigger an early exit
      _ => Ok(None),
    }
  }

  pub(super) fn extract_image_bytes(&self) -> Result<Option<Vec<u8>>, ErrorWrapper> {
    let max_size = self.max_size;

    if let Some(png_bytes) =
      unsafe { self.extract_clipboard_format(NSPasteboardTypePNG, max_size)? }
    {
      trace!("Loaded PNG from clipboard");

      Ok(Some(png_bytes))
    } else if let Some(tiff_bytes) =
      unsafe { self.extract_clipboard_format(NSPasteboardTypeTIFF, max_size)? }
    {
      trace!("Loaded TIFF from clipboard. Converting to PNG...");

      if let Some(png_bytes) = convert_tiff_to_png(&tiff_bytes) {
        Ok(Some(png_bytes))
      } else {
        // We got the content but failed to extract it, trigger early exit
        Err(ErrorWrapper::ReadError(ClipboardError::ImageConversion))
      }
    } else {
      Ok(None)
    }
  }

  fn string_from_type(&self, type_: &'static NSString) -> Result<Option<String>, ErrorWrapper> {
    // XXX: We explicitly use `pasteboardItems` and not `stringForType` since the latter will concat
    // multiple strings, if present, into one and return it instead of reading just the first which is `arboard`'s
    // historical behavior.
    autoreleasepool(|_| {
      // If no pasteboard items are found, we trigger the early exit
      let contents =
        unsafe { self.pasteboard.pasteboardItems() }.ok_or(ErrorWrapper::EmptyContent)?;

      for item in contents {
        if let Some(string) = unsafe { item.stringForType(type_) } {
          if !string.is_empty() {
            return Ok(Some(string.to_string()));
          } else {
            return Err(ErrorWrapper::EmptyContent);
          }
        }
      }

      Ok(None)
    })
  }

  fn extract_content(&self) -> Result<Option<Body>, ErrorWrapper> {
    autoreleasepool(|_| {
      let max_size = self.max_size;

      for name in self.custom_formats.iter() {
        let format_nsstring = NSString::from_str(name.as_ref());
        // For custom formats, we check the size as well as the presence
        if let Some(bytes) = self.extract_clipboard_format(&format_nsstring, max_size)? {
          return Ok(Some(Body::new_custom(name.clone(), bytes)));
        }
      }

      if let Some(image_bytes) = self.extract_image_bytes()? {
        // If there is only one path in the file list, which is sometimes emitted by the OS
        // when copying an image, we assign it to the image
        let image_path = if let Some(mut files_list) = self.extract_files_list()? {
          if files_list.len() == 1 {
            Some(files_list.remove(0))
          } else {
            None
          }
        } else {
          None
        };

        Ok(Some(Body::new_image(image_bytes, image_path)))
      } else if let Some(mut files_list) = self.extract_files_list()? {
        Ok(Some(Body::new_file_list(files_list)))
      } else {
        if let Some(html) = unsafe { self.string_from_type(NSPasteboardTypeHTML)? } {
          return Ok(Some(Body::new_html(html)));
        }
        if let Some(plain_text) = unsafe { self.string_from_type(NSPasteboardTypeString)? } {
          return Ok(Some(Body::new_text(plain_text)));
        }

        Ok(None)
      }
    })
  }

  fn get_clipboard_content(&self) -> Result<Option<Body>, ClipboardError> {
    match self.extract_content() {
      // Found content
      Ok(Some(content)) => Ok(Some(content)),

      // Non-fatal errors, we just return None
      Err(ErrorWrapper::EmptyContent) => {
        debug!("Found empty content. Skipping it...");
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
