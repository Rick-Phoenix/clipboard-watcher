use image::{DynamicImage, ImageFormat};

use crate::error::ClipboardError;

pub(crate) fn load_png(bytes: &[u8]) -> Result<DynamicImage, ClipboardError> {
  image::load_from_memory_with_format(bytes, ImageFormat::Png)
    .map_err(|e| ClipboardError::ReadError(format!("Failed to load PNG image: {e}")))
}

#[cfg(windows)]
pub(crate) fn load_dib(bytes: &[u8]) -> Result<DynamicImage, ClipboardError> {
  use std::io::Cursor;

  use image::codecs::bmp::BmpDecoder;

  let cursor = Cursor::new(bytes);

  let decoder = BmpDecoder::new_without_file_header(cursor)
    .map_err(|e| ClipboardError::ReadError(format!("Failed to load DIB image: {e}")))?;

  DynamicImage::from_decoder(decoder)
    .map_err(|e| ClipboardError::ReadError(format!("Failed to load DIB image: {e}")))
}
