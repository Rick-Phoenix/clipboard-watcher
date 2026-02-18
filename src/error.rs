use std::{convert::Infallible, fmt::Display, sync::Arc};

use thiserror::Error;

use crate::Body;

pub(crate) trait WithContext<T> {
  fn context(self, msg: &str) -> Result<T, String>;
}

impl<T, E: Display> WithContext<T> for Result<T, E> {
  fn context(self, msg: &str) -> Result<T, String> {
    self.map_err(|e| format!("{msg}: {e}"))
  }
}

impl<T> WithContext<T> for Option<T> {
  fn context(self, msg: &str) -> Result<T, String> {
    self.ok_or_else(|| msg.to_string())
  }
}

/// An error encountered while initializing the clipboard watcher
#[derive(Clone, Debug, Error)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(transparent))]
#[error("Failed to start clipboard monitor: {0}")]
pub struct InitializationError(pub String);

impl From<Infallible> for InitializationError {
  #[inline(never)]
  #[cold]
  fn from(value: Infallible) -> Self {
    match value {}
  }
}

/// Various kinds of errors that can occur while monitoring or reading the clipboard.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Debug, Error)]
#[non_exhaustive]
pub enum ClipboardError {
  #[error("Failed to monitor the clipboard: {0}")]
  MonitorFailed(String),

  #[error("Failed to read the clipboard: {0}")]
  ReadError(String),

  #[error("The content of the clipboard did not match any supported format")]
  NoMatchingFormat,
}

impl From<Infallible> for ClipboardError {
  fn from(value: Infallible) -> Self {
    match value {}
  }
}

pub(crate) enum ErrorWrapper {
  EmptyContent,
  SizeTooLarge,
  ReadError(ClipboardError),
  UserSkipped,
}

impl From<ClipboardError> for ErrorWrapper {
  #[inline]
  fn from(value: ClipboardError) -> Self {
    Self::ReadError(value)
  }
}

pub type ClipboardResult = Result<Arc<Body>, ClipboardError>;
