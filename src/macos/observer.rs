use objc2::rc::{Retained, autoreleasepool};
use objc2_app_kit::{
  NSFilenamesPboardType, NSPasteboard, NSPasteboardTypePNG, NSPasteboardTypeString,
};

use crate::Body;

pub(crate) struct OSXObserver {
  stop: Arc<AtomicBool>,
  pasteboard: Retained<NSPasteboard>,
  interval: Duration,
  custom_formats: Vec<Arc<str>>,
  max_image_bytes: Option<usize>,
  max_bytes: Option<usize>,
}

impl OSXObserver {
  pub(super) fn new(
    stop: Arc<AtomicBool>,
    interval: Option<Duration>,
    custom_formats: Vec<Arc<str>>,
    max_image_bytes: Option<usize>,
    max_bytes: Option<usize>,
  ) -> Self {
    let pasteboard = unsafe { NSPasteboard::generalPasteboard() };

    let max_image_bytes = if max_image_bytes.is_none() && max_bytes.is_some() {
      debug!("Using global size limit for images...");

      max_bytes
    } else {
      max_image_bytes
    };

    OSXObserver {
      stop,
      pasteboard,
      interval,
      custom_formats,
      max_image_bytes,
      max_bytes,
    }
  }
}

impl Observer for OSXObserver {
  fn observe(&mut self, body_senders: Arc<BodySenders>) {
    let mut last_count = self.get_change_count();

    let interval = self
      .interval
      .unwrap_or_else(|| std::time::Duration::from_millis(200));

    while !self.stop.load(Ordering::Relaxed) {
      std::thread::sleep(interval);

      let change_count = self.get_change_count();

      if change_count != last_count {
        last_count = change_count;

        body_senders.send_all(Arc::new(self.get_content()))
      }
    }
  }
}

impl OSXObserver {
  pub(crate) fn get_change_count(&self) -> isize {
    unsafe { self.pasteboard.changeCount() }
  }

  pub(crate) fn extract_file_list(self) -> Option<Vec<PathBuf>> {
    autoreleasepool(|_| {
      let class_array = NSArray::from_slice(&[NSURL::class()]);
      let options = NSDictionary::from_slices(
        &[unsafe { NSPasteboardURLReadingFileURLsOnlyKey }],
        &[NSNumber::new_bool(true).as_ref()],
      );
      let objects = unsafe {
        self
          .clipboard
          .pasteboard
          .readObjectsForClasses_options(&class_array, Some(&options))
      };

      objects
        .map(|array| {
          array
            .iter()
            .filter_map(|obj| {
              obj
                .downcast::<NSURL>()
                .ok()
                .and_then(|url| unsafe { url.path() }.map(|p| PathBuf::from(p.to_string())))
            })
            .collect::<Vec<_>>()
        })
        .filter(|file_list| !file_list.is_empty())
    })
  }

  fn extract_image_bytes(&self) -> Option<Vec<u8>> {
    if let Some(png_bytes) = self.get_data_for_type(NSPasteboardTypePNG, self.max_image_bytes) {
      debug!("Found image in png format");

      Some(png_bytes)
    } else if let Some(tiff_bytes) =
      self.get_data_for_type(NSPasteboardTypeTIFF, self.max_image_bytes)
    {
      debug!("Found raw TIFF data. Normalizing to PNG...");

      if let Some(png_bytes) = convert_tiff_to_png(&tiff_bytes) {
        Some(png_bytes)
      } else {
        None
      }
    }

    None
  }

  fn get_raw_data(
    &self,
    format_type: &NSPasteboardType,
    max_bytes: Option<usize>,
  ) -> Option<Vec<u8>> {
    max_bytes
      .is_none_or(|max| {
        self
          .get_data_for_type(format_type)
          .is_some_and(|size| max > size)
      })
      .then_some(|| {
        let data = unsafe { self.pasteboard.dataForType(format_type) }?;
        data.to_vec()
      })
      .filter(|data| !data.is_empty())
  }

  fn string_from_type(&self, type_: &'static NSString) -> Option<String> {
    // XXX: We explicitly use `pasteboardItems` and not `stringForType` since the latter will concat
    // multiple strings, if present, into one and return it instead of reading just the first which is `arboard`'s
    // historical behavior.
    let contents = unsafe { self.pasteboard.pasteboardItems() }
      .ok_or_else(|| Error::unknown("NSPasteboard#pasteboardItems errored"))?;

    for item in contents {
      if let Some(string) = unsafe { item.stringForType(type_) } {
        return Some(string.to_string());
      }
    }

    None
  }

  fn get_data_size_for_type(&self, format_type: &NSPasteboardType) -> Option<usize> {
    // Get cheap reference first
    let data_obj: Option<Retained<NSData>> = unsafe { self.pasteboard.dataForType(format_type) };

    data_obj.map(|data| {
      // Get size of buffer
      let size = data.len();
      size
    })
  }

  pub(crate) fn get_content(&self) -> Result<Body, ClipboardError> {
    autoreleasepool(|_| {
      let max_bytes = self.max_bytes;

      for format in self.custom_formats.iter() {
        let format_nsstring = NSString::from_str(format.as_ref());

        if let Some(bytes) = get_data_size_for_type(&format_nsstring)
          .filter(|size| max_bytes.is_none_or(|max| max > size))
          .and_then(|_| get_data_for_type(&format_nsstring))
        {
          return Ok(Body::Custom {
            name: format.clone(),
            data: bytes,
          });
        }
      }

      if let Some(image_bytes) = self.extract_image_bytes() {
        let image_path = if let Some(mut files_list) = self.extract_file_list()
          && files_list.len() == 1
        {
          Some(files_list.remove(0))
        } else {
          None
        };

        Ok(Body::Image(ClipboardImage {
          path: image_path,
          bytes: image_bytes,
        }))
      } else if let Some(mut files_list) = self.extract_file_list() {
        // If there is just one file in the list and it's an image,
        // we save it directly as an image
        use crate::image::{convert_file_to_png, file_is_image};

        // We check if there is just one file
        if files_list.len() == 1
        && let Some(path) = files_list.first()

        // Then, if it's an image
        && file_is_image(path)
        // Then, if the size is within the allowed range
        && max_bytes.is_none_or(|max| path.metadata().is_ok_and(|metadata| max as u64 > metadata.len()))
        // Then, if the bytes are readable and the conversion to png is successful
        && let Some(png_bytes) = convert_file_to_png(path)
        //
        // Only if all of these are true, we save it as an image
        {
          debug!("Found file path with image format. Processing it as an image...");

          let image_path = files_list.remove(0);

          Ok(Body::Image(ClipboardImage {
            bytes: png_bytes,
            path: Some(image_path),
          }))
        } else {
          Ok(Body::FileList(files_list))
        }
      } else if let Some(html) = self.string_from_type(NSPasteboardTypeHTML) {
        Ok(Body::Html(html))
      } else if let Some(rich_text) = self.string_from_type(NSPasteboardTypeRTF) {
        Ok(Body::RichText(rich_text))
      } else if let Some(text) = self.string_from_type(NSPasteboardTypeString) {
        Ok(Body::PlainText(text))
      }
    })
  }
}
