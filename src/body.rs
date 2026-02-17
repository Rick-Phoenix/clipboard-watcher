use crate::*;

use crate::logging::HumanBytes;

/// The content extracted from the clipboard.
///
/// To avoid extracting all types of content each time, only one of them is chosen, in the following order of priority:
///
/// - Custom formats (in the order they are given, if present)
/// - Png Image
/// - Raw Image (normalized to rgb8)
/// - File list
/// - HTML
/// - Plain text
///
/// When a clipboard item can fit more than one of these formats, only the one with the highest priority will be chosen.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(untagged))]
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Body {
	/// HTML content.
	Html(String),
	/// Plaintext content.
	PlainText(String),
	/// An raw image taken from the clipboard (in bmp or tiff format)
	/// and converted to raw rgb8 bytes.
	RawImage(RawImage),
	/// An image in png format.
	PngImage {
		bytes: Vec<u8>,
		path: Option<PathBuf>,
	},
	/// A list of files.
	FileList(Vec<PathBuf>),
	/// A custom format.
	Custom { name: Arc<str>, data: Vec<u8> },
}

impl Body {
	/// Checks whether this instance contains an image.
	#[must_use]
	pub const fn is_image(&self) -> bool {
		matches!(self, Self::RawImage(_) | Self::PngImage { .. })
	}

	pub(crate) fn new_png(bytes: Vec<u8>, path: Option<PathBuf>) -> Self {
		if log::log_enabled!(log::Level::Debug) {
			if let Some(path) = &path {
				debug!(
					"Found PNG image. Size: {}, Path: {}",
					HumanBytes(bytes.len()),
					path.display()
				);
			} else {
				debug!(
					"Found PNG image. Size: {}, Path: None",
					HumanBytes(bytes.len())
				);
			};
		}

		Self::PngImage { bytes, path }
	}

	#[cfg(not(target_os = "linux"))]
	pub(crate) fn new_image(image: image::DynamicImage, path: Option<PathBuf>) -> Self {
		let rgb = image.into_rgb8();

		let (width, height) = rgb.dimensions();
		let image = RawImage {
			bytes: rgb.into_raw(),
			path,
			width,
			height,
		};

		if log::log_enabled!(log::Level::Debug) {
			image.log_info();
		}

		Self::RawImage(image)
	}

	pub(crate) fn new_custom(name: Arc<str>, data: Vec<u8>) -> Self {
		if log::log_enabled!(log::Level::Debug) {
			debug!(
				"Found content with custom format `{name}`. Size: {}",
				HumanBytes(data.len())
			);
		}

		Self::Custom { name, data }
	}

	pub(crate) fn new_file_list(files: Vec<PathBuf>) -> Self {
		if log::log_enabled!(log::Level::Debug) {
			debug!("Found file list with {} elements: {files:?}", files.len());
		}

		Self::FileList(files)
	}

	pub(crate) fn new_html(html: String) -> Self {
		if log::log_enabled!(log::Level::Debug) {
			debug!("Found html content");
		}

		Self::Html(html)
	}

	pub(crate) fn new_text(text: String) -> Self {
		if log::log_enabled!(log::Level::Debug) {
			debug!("Found text content");
		}

		Self::PlainText(text)
	}
}

/// An image from the clipboard, normalized to raw rgb8 bytes.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct RawImage {
	/// The rgb8 bytes of the image.
	pub bytes: Vec<u8>,
	/// The width of the image
	pub width: u32,
	/// The height of the image
	pub height: u32,
	/// The path to the image's file (if one can be detected).
	pub path: Option<PathBuf>,
}

impl RawImage {
	/// Checks whether the clipboard has a file path attached to it.
	#[must_use]
	pub const fn has_path(&self) -> bool {
		self.path.is_some()
	}

	#[cfg(not(target_os = "linux"))]
	pub(crate) fn log_info(&self) {
		if let Some(path) = &self.path {
			debug!(
				"Found raw image. Size: {}, Path: {}",
				HumanBytes(self.bytes.len()),
				path.display()
			);
		} else {
			debug!(
				"Found raw image. Size: {}, Path: None",
				HumanBytes(self.bytes.len())
			);
		}
	}
}
