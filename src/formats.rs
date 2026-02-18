use crate::*;

/// A struct that represents a clipboard format.
#[derive(Debug, Clone)]
pub struct Format {
  pub(crate) name: Arc<str>,
  #[cfg(not(target_os = "macos"))]
  pub(crate) id: u32,
  #[cfg(target_os = "macos")]
  pub(crate) id: objc2::rc::Retained<objc2_foundation::NSString>,
}

impl Format {
  /// Returns the name of the format
  #[must_use]
  #[inline]
  pub fn name(&self) -> &str {
    &self.name
  }
}

/// A struct that represents the list of formats currently available on the clipboard.
#[derive(Default, Debug)]
pub struct Formats {
  pub(crate) data: Vec<Format>,
}

impl FromIterator<Format> for Formats {
  fn from_iter<T: IntoIterator<Item = Format>>(iter: T) -> Self {
    Self {
      data: iter.into_iter().collect(),
    }
  }
}

impl IntoIterator for Formats {
  type Item = Format;
  type IntoIter = std::vec::IntoIter<Format>;

  #[inline]
  fn into_iter(self) -> Self::IntoIter {
    self.data.into_iter()
  }
}

impl<'a> IntoIterator for &'a Formats {
  type Item = &'a Format;
  type IntoIter = std::slice::Iter<'a, Format>;

  #[inline]
  fn into_iter(self) -> Self::IntoIter {
    self.data.iter()
  }
}

impl Formats {
  #[inline]
  pub fn iter(&self) -> std::slice::Iter<'_, Format> {
    self.data.iter()
  }

  #[cfg(not(target_os = "macos"))]
  #[must_use]
  #[inline]
  pub(crate) fn contains_id(&self, id: u32) -> bool {
    self.data.iter().any(|d| d.id == id)
  }
}
