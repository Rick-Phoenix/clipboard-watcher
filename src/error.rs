use std::{convert::Infallible, sync::Arc};

use thiserror::Error;

use crate::Body;

#[derive(Clone, Debug, Error)]
#[error("Failed to start clipboard monitor: {0}")]
pub struct InitializationError(pub String);

impl From<Infallible> for InitializationError {
  fn from(value: Infallible) -> Self {
    match value {}
  }
}

/// Various kinds of errors that can occur while monitoring or reading the clipboard.
#[derive(Clone, Debug, Error)]
#[non_exhaustive]
pub enum ClipboardError {
  #[error("Failed to monitor the clipboard: {0}")]
  MonitorFailed(String),

  #[error("Failed to receive data from channel: {0}")]
  TryRecvError(String),

  #[error("Failed to read the clipboard: {0}")]
  ReadError(String),

  #[error("The content of the clipboard did not match any supported format")]
  NoMatchingFormat,

  #[error("Could not convert clipboard image to png format")]
  ImageConversion,
}

impl From<Infallible> for ClipboardError {
  fn from(value: Infallible) -> Self {
    match value {}
  }
}

pub(crate) enum ErrorWrapper {
  EmptyContent,
  SizeTooLarge,
  FormatUnavailable,
  ReadError(ClipboardError),
}

pub(crate) enum ExtractionError {
  EmptyContent,
  SizeTooLarge,
  ConversionError,
}

pub type ClipboardResult = Result<Arc<Body>, ClipboardError>;
