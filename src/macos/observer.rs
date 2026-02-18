use crate::*;

use image::ImageFormat;
use objc2::{
  ClassType,
  rc::{Retained, autoreleasepool},
};
use objc2_app_kit::{
  NSPasteboard, NSPasteboardType, NSPasteboardTypeFileURL, NSPasteboardTypeHTML,
  NSPasteboardTypePNG, NSPasteboardTypeString, NSPasteboardTypeTIFF,
  NSPasteboardURLReadingFileURLsOnlyKey,
};
use objc2_foundation::{NSArray, NSData, NSDictionary, NSNumber, NSString, NSURL};

pub(crate) struct OSXObserver<G: Gatekeeper = DefaultGatekeeper> {
  stop_signal: Arc<AtomicBool>,
  pasteboard: Retained<NSPasteboard>,
  interval: Duration,
  custom_formats: Formats,
  max_size: Option<u32>,
  gatekeeper: G,
}

impl ClipboardContext<'_> {
  #[must_use]
  pub fn get_data(&self, format: &Format) -> Option<Vec<u8>> {
    extract_clipboard_format_macos(&self.pasteboard, self.formats, &format.id, None).ok()?
  }
}

impl Formats {
  pub(crate) fn contains_format(&self, target_type: &NSPasteboardType) -> bool {
    self
      .iter()
      .any(|f| <Retained<NSString> as AsRef<NSPasteboardType>>::as_ref(&f.id) == target_type)
  }
}

impl<G: Gatekeeper> OSXObserver<G> {
  #[inline(never)]
  #[cold]
  pub(crate) fn new(
    stop_signal: Arc<AtomicBool>,
    interval: Option<Duration>,
    custom_format_names: Vec<Arc<str>>,
    max_size: Option<u32>,
    gatekeeper: G,
  ) -> Self {
    let pasteboard = unsafe { NSPasteboard::generalPasteboard() };
    let custom_formats: Formats = custom_format_names
      .into_iter()
      .map(|str| Format {
        id: NSString::from_str(str.as_ref()),
        name: str,
      })
      .collect();

    OSXObserver {
      stop_signal,
      pasteboard,
      interval: interval.unwrap_or_else(|| std::time::Duration::from_millis(200)),
      custom_formats,
      max_size,
      gatekeeper,
    }
  }
}

impl<G: Gatekeeper> Observer for OSXObserver<G> {
  fn observe(&mut self, body_senders: Arc<BodySenders>) {
    let mut last_count = self.get_change_count();

    info!("Started monitoring the clipboard");

    while !self.stop_signal.load(Ordering::Relaxed) {
      let change_count = self.get_change_count();

      if change_count != last_count {
        last_count = change_count;

        match self.get_clipboard_content() {
          Ok(Some(content)) => body_senders.send_all(&Ok(Arc::new(content))),
          Err(e) => {
            warn!("{e}");
            body_senders.send_all(&Err(e));
          }
          // Found content but ignored it (empty or beyond allowed size)
          Ok(None) => {}
        }
      }

      std::thread::sleep(self.interval);
    }
  }
}

impl<G: Gatekeeper> OSXObserver<G> {
  fn get_available_formats(&self) -> Result<Formats, ErrorWrapper> {
    unsafe {
      // 1. Get the NSArray of types
      // types() returns Option<Retained<NSArray<NSPasteboardType>>>
      let types_array =
        self
          .pasteboard
          .types()
          .ok_or(ErrorWrapper::ReadError(ClipboardError::ReadError(
            "Could not get types".to_string(),
          )))?;

      // 2. Map NSArray -> Vec<Format>
      let data: Vec<Format> = types_array
        .iter()
        .map(|ns_string| {
          // Convert NSString to Rust String
          let rust_string = ns_string.to_string();

          Format {
            name: rust_string.into(), // Arc<str>
            id: ns_string,            // Retained<NSString>
          }
        })
        .collect();

      Ok(Formats { data })
    }
  }

  fn get_change_count(&self) -> isize {
    unsafe { self.pasteboard.changeCount() }
  }

  fn extract_files_list(
    &self,
    available_types: &Formats,
  ) -> Result<Option<Vec<PathBuf>>, ErrorWrapper> {
    if unsafe { !available_types.contains_format(&NSPasteboardTypeFileURL) } {
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

  fn extract_png(&self, available_types: &Formats) -> Result<Option<Vec<u8>>, ErrorWrapper> {
    unsafe {
      extract_clipboard_format_macos(
        &self.pasteboard,
        available_types,
        NSPasteboardTypePNG,
        self.max_size,
      )
    }
  }

  fn extract_raw_image(
    &self,
    available_types: &Formats,
  ) -> Result<Option<image::DynamicImage>, ErrorWrapper> {
    if let Some(tiff_bytes) = unsafe {
      extract_clipboard_format_macos(
        &self.pasteboard,
        available_types,
        NSPasteboardTypeTIFF,
        self.max_size,
      )?
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
    available_types: &Formats,
    type_: &'static NSString,
  ) -> Result<Option<String>, ErrorWrapper> {
    if !available_types.contains_format(type_) {
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

      let formats = self.get_available_formats()?;

      let ctx = ClipboardContext {
        formats: &formats,
        pasteboard: &self.pasteboard,
      };

      if !self.gatekeeper.check(ctx) {
        return Err(ErrorWrapper::UserSkipped);
      }

      for format in self.custom_formats.iter() {
        // For custom formats, we check the size as well as the presence
        if let Some(bytes) =
          extract_clipboard_format_macos(&self.pasteboard, &formats, &format.id, max_size)?
        {
          return Ok(Some(Body::new_custom(format.name.clone(), bytes)));
        }
      }

      if let Some(png_bytes) = self.extract_png(&formats)? {
        // Extract the image path if we have a list of files with a single item
        let image_path = self
          .extract_files_list(&formats)?
          .filter(|list| list.len() == 1)
          .map(|mut files| files.remove(0));

        Ok(Some(Body::new_png(png_bytes, image_path)))
      } else if let Some(image) = self.extract_raw_image(&formats)? {
        // Extract the image path if we have a list of files with a single item
        let image_path = self
          .extract_files_list(&formats)?
          .filter(|list| list.len() == 1)
          .map(|mut files| files.remove(0));

        Ok(Some(Body::new_image(image, image_path)))
      } else if let Some(files_list) = self.extract_files_list(&formats)? {
        Ok(Some(Body::new_file_list(files_list)))
      } else {
        if let Some(html) = unsafe { self.string_from_type(&formats, NSPasteboardTypeHTML)? } {
          return Ok(Some(Body::new_html(html)));
        }
        if let Some(plain_text) =
          unsafe { self.string_from_type(&formats, NSPasteboardTypeString)? }
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

      Err(ErrorWrapper::SizeTooLarge | ErrorWrapper::UserSkipped) => Ok(None),

      // Actual error
      Err(ErrorWrapper::ReadError(e)) => Err(e),

      // There was content but we could not read it
      Ok(None) => Err(ClipboardError::NoMatchingFormat),
    }
  }
}

// Attempts to extract a specific format from the clipboard
pub(crate) fn extract_clipboard_format_macos(
  pasteboard: &NSPasteboard,
  available_types: &Formats,
  format_type: &NSPasteboardType,
  max_size: Option<u32>,
) -> Result<Option<Vec<u8>>, ErrorWrapper> {
  if !available_types.contains_format(format_type) {
    return Ok(None);
  }

  autoreleasepool(|_| {
    let data_obj: Option<Retained<NSData>> = unsafe { pasteboard.dataForType(format_type) };

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
