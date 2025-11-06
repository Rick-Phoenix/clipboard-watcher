use std::{convert::Infallible, sync::Arc};

use thiserror::Error;

use crate::Body;

/// Various kinds of errors that can occur while monitoring or reading the clipboard.
#[derive(Clone, Debug, Error)]
#[non_exhaustive]
pub enum ClipboardError {
  #[error("Failed to start clipboard monitor: {0}")]
  InitializationError(String),

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

  #[error("The selected clipboard is not supported with the current system configuration.")]
  ClipboardNotSupported,

  #[error("The native clipboard is not accessible due to being held by another party.")]
  ClipboardOccupied,
}

impl From<Infallible> for ClipboardError {
  fn from(value: Infallible) -> Self {
    match value {}
  }
}

pub(crate) enum ExtractionError {
  EmptyContent,
  SizeTooLarge,
  ConversionError,
}

pub type ClipboardResult = Result<Arc<Body>, ClipboardError>;
