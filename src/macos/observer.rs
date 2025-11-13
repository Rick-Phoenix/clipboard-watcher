use std::{
  path::PathBuf,
  sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
  },
  time::Duration,
};

use image::ImageFormat;
use log::{debug, info, trace, warn};
use objc2::{
  rc::{autoreleasepool, Retained},
  ClassType,
};
use objc2_app_kit::{
  NSPasteboard, NSPasteboardType, NSPasteboardTypeFileURL, NSPasteboardTypeHTML,
  NSPasteboardTypePNG, NSPasteboardTypeString, NSPasteboardTypeTIFF,
  NSPasteboardURLReadingFileURLsOnlyKey,
};
use objc2_foundation::{NSArray, NSData, NSDictionary, NSNumber, NSString, NSURL};

use crate::{
  body::*,
  error::{ClipboardError, ErrorWrapper},
  logging::*,
  observer::Observer,
};

struct AvailableTypes {
  inner: Retained<NSArray<NSPasteboardType>>,
}

impl AvailableTypes {
  pub fn new(inner: Retained<NSArray<NSPasteboardType>>) -> Self {
    Self { inner }
  }

  pub fn contains(&self, format_type: &NSPasteboardType) -> bool {
    unsafe { self.inner.containsObject(&format_type) }
  }
}

// Caches the custom format in both formats used for retrieval/storage
pub(crate) struct CustomFormat {
  pub(crate) ns_string: Retained<NSString>,
  pub(crate) rust_string: Arc<str>,
}

impl CustomFormat {
  pub(crate) fn new(str: Arc<str>) -> Self {
    Self {
      ns_string: NSString::from_str(str.as_ref()),
      rust_string: str,
    }
  }
}

pub(crate) struct OSXObserver {
  stop: Arc<AtomicBool>,
  pasteboard: Retained<NSPasteboard>,
  interval: Duration,
  custom_formats: Vec<CustomFormat>,
  max_size: Option<u32>,
}

impl OSXObserver {
  pub(crate) fn new(
    stop: Arc<AtomicBool>,
    interval: Option<Duration>,
    custom_formats: Vec<Arc<str>>,
    max_size: Option<u32>,
  ) -> Self {
    let pasteboard = unsafe { NSPasteboard::generalPasteboard() };
    let custom_formats: Vec<CustomFormat> = custom_formats
      .into_iter()
      .map(|str| CustomFormat::new(str))
      .collect();

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

    info!("Started monitoring the clipboard");

    while !self.stop.load(Ordering::Relaxed) {
      let change_count = self.get_change_count();

      if change_count != last_count {
        last_count = change_count;

        match self.get_clipboard_content() {
          Ok(Some(content)) => body_senders.send_all(Ok(Arc::new(content))),
          Err(e) => {
            warn!("{e}");
            body_senders.send_all(Err(e));
          }
          // Found content but ignored it (empty or beyond allowed size)
          Ok(None) => {}
        }
      }

      std::thread::sleep(self.interval);
    }
  }
}

impl OSXObserver {
  fn get_change_count(&self) -> isize {
    unsafe { self.pasteboard.changeCount() }
  }

  // Attempts to extract a specific format from the clipboard
  fn extract_clipboard_format(
    &self,
    available_types: &AvailableTypes,
    format_type: &NSPasteboardType,
    max_size: Option<u32>,
  ) -> Result<Option<Vec<u8>>, ErrorWrapper> {
    if !available_types.contains(format_type) {
      return Ok(None);
    }

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
                "Found content with {} size, beyond maximum allowed size. Skipping it...",
                HumanBytes(size)
              );

              return Err(ErrorWrapper::SizeTooLarge);
            }
          }

          // Size is okay, copy the data to a Rust Vec.
          Ok(Some(data.to_vec()))
        }
        // Format was not present (technically it should not happen
        // since the format was in the list already)
        None => Ok(None),
      }
    })
  }

  fn extract_files_list(
    &self,
    available_types: &AvailableTypes,
  ) -> Result<Option<Vec<PathBuf>>, ErrorWrapper> {
    if unsafe { !available_types.contains(&NSPasteboardTypeFileURL) } {
      return Ok(None);
    }

    let files = autoreleasepool(|_| {
      // The readObjects(classes, options) receives two arguments:
      //
      // 1. The list of classes to read (in this case, just NSURL)
      let class_array = NSArray::from_slice(&[NSURL::class()]);

      // 2. The options for the query (in our case, only read the URLs that are FileURLs)
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
      // were found. Theoretically impossible since the format is already in the list.
      _ => Ok(None),
    }
  }

  fn extract_png(&self, available_types: &AvailableTypes) -> Result<Option<Vec<u8>>, ErrorWrapper> {
    unsafe { self.extract_clipboard_format(available_types, NSPasteboardTypePNG, self.max_size) }
  }

  fn extract_raw_image(
    &self,
    available_types: &AvailableTypes,
  ) -> Result<Option<image::DynamicImage>, ErrorWrapper> {
    if let Some(tiff_bytes) = unsafe {
      self.extract_clipboard_format(available_types, NSPasteboardTypeTIFF, self.max_size)?
    } {
      trace!("Found image in TIFF format");

      let image = image::load_from_memory_with_format(&tiff_bytes, ImageFormat::Tiff)
        .map_err(|e| ClipboardError::ReadError(format!("Failed to load TIFF image: {e}")))?;

      Ok(Some(image))
    } else {
      Ok(None)
    }
  }

  // From [arboard](https://github.com/1Password/arboard), with modifications
  fn string_from_type(
    &self,
    available_types: &AvailableTypes,
    type_: &'static NSString,
  ) -> Result<Option<String>, ErrorWrapper> {
    if !available_types.contains(type_) {
      return Ok(None);
    }

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

  // Reads the clipboard and extract the first kind of format available, following the priority list
  fn extract_content(&self) -> Result<Option<Body>, ErrorWrapper> {
    autoreleasepool(|_| {
      let max_size = self.max_size;

      let available_types = if let Some(types) = unsafe { self.pasteboard.types() } && !types.is_empty() {
        AvailableTypes::new(types)
      } else {
        return Ok(None)
      };

      for format in self.custom_formats.iter() {
        // For custom formats, we check the size as well as the presence
        if let Some(bytes) =
          self.extract_clipboard_format(&available_types, &format.ns_string, max_size)?
        {
          return Ok(Some(Body::new_custom(format.rust_string.clone(), bytes)));
        }
      }

      if let Some(png_bytes) = self.extract_png(&available_types)? {
        // Extract the image path if we have a list of files with a single item
        let image_path = self
          .extract_files_list(&available_types)?
          .filter(|list| list.len() == 1)
          .map(|mut files| files.remove(0));

        Ok(Some(Body::new_png(png_bytes, image_path)))
      } else if let Some(image) = self.extract_raw_image(&available_types)? {
        // Extract the image path if we have a list of files with a single item
        let image_path = self
          .extract_files_list(&available_types)?
          .filter(|list| list.len() == 1)
          .map(|mut files| files.remove(0));

        Ok(Some(Body::new_image(image, image_path)))
      } else if let Some(files_list) = self.extract_files_list(&available_types)? {
        Ok(Some(Body::new_file_list(files_list)))
      } else {
        if let Some(html) =
          unsafe { self.string_from_type(&available_types, NSPasteboardTypeHTML)? }
        {
          return Ok(Some(Body::new_html(html)));
        }
        if let Some(plain_text) =
          unsafe { self.string_from_type(&available_types, NSPasteboardTypeString)? }
        {
          return Ok(Some(Body::new_text(plain_text)));
        }

        Ok(None)
      }
    })
  }

  // Tries to read the clipboard and unwraps the error, if one was encountered
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

      // Actual error
      Err(ErrorWrapper::ReadError(e)) => Err(e),

      // There was content but we could not read it
      Ok(None) => Err(ClipboardError::NoMatchingFormat),
    }
  }
}
