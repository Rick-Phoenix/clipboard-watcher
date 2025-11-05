use log::error;
use std::{io::Cursor, path::Path};

use image::{DynamicImage, ImageFormat, codecs::bmp::BmpDecoder};

pub(crate) fn convert_dib_to_png(dib_bytes: &[u8]) -> Option<Vec<u8>> {
  let cursor = Cursor::new(dib_bytes);

  let decoder = BmpDecoder::new_without_file_header(cursor).ok()?;

  let dynamic_image = DynamicImage::from_decoder(decoder).ok()?;

  let mut png_buffer = Vec::new();
  if dynamic_image
    .write_to(&mut Cursor::new(&mut png_buffer), ImageFormat::Png)
    .is_ok()
  {
    Some(png_buffer)
  } else {
    error!("Failed to convert dib to png");
    None
  }
}

pub(crate) fn convert_file_to_png(path: &Path) -> Option<Vec<u8>> {
  let file_bytes = std::fs::read(path)
    .inspect_err(|e| {
      error!(
        "Failed to read the contents of file `{}`: {e}",
        path.display()
      )
    })
    .ok()?;

  let dynamic_image = image::load_from_memory(&file_bytes)
    .inspect_err(|e| {
      error!(
        "Failed to create image from contents of file `{}`: {e}",
        path.display()
      )
    })
    .ok()?;

  let mut png_buffer = Vec::new();
  dynamic_image
    .write_to(&mut Cursor::new(&mut png_buffer), ImageFormat::Png)
    .inspect_err(|e| error!("Failed to convert file `{}` to a png: {e}", path.display()))
    .ok()?;

  Some(png_buffer)
}

const IMAGE_FORMATS: [&str; 8] = ["png", "jpg", "jpeg", "gif", "bmp", "webp", "svg", "ico"];

pub(crate) fn file_is_image(path: &Path) -> bool {
  path
    .extension()
    .is_some_and(|e| IMAGE_FORMATS.contains(&e.to_string_lossy().as_ref()))
}

#[cfg(target_os = "macos")]
pub(crate) fn convert_tiff_to_png(tiff_bytes: &[u8]) -> Option<Vec<u8>> {
  match image::load_from_memory_with_format(tiff_bytes, ImageFormat::Tiff) {
    Ok(dynamic_image) => {
      let mut png_buffer = Vec::new();
      if dynamic_image
        .write_to(&mut Cursor::new(&mut png_buffer), ImageFormat::Png)
        .is_ok()
      {
        Some(png_buffer)
      } else {
        error!("Failed to convert tiff to png");

        None
      }
    }
    Err(e) => {
      error!("Failed to convert tiff to png: {e}");
      None
    }
  }
}
