use std::{
  pin::Pin,
  task::{Context, Poll},
};

use futures::{channel::mpsc::Receiver, Stream};

use crate::{body::BodySendersDropHandle, error::ClipboardResult};

/// Asynchronous stream for fetching clipboard item.
///
/// When the clipboard is updated, the [`ClipboardStream`] polls for the yields the new data.
#[derive(Debug)]
pub struct ClipboardStream {
  pub(crate) id: StreamId,
  pub(crate) body_rx: Pin<Box<Receiver<ClipboardResult>>>,
  pub(crate) drop_handle: BodySendersDropHandle,
}

impl ClipboardStream {
  pub fn id(&self) -> &StreamId {
    &self.id
  }
}

impl Stream for ClipboardStream {
  type Item = ClipboardResult;

  fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
    self.body_rx.as_mut().poll_next(cx)
  }
}

impl Drop for ClipboardStream {
  fn drop(&mut self) {
    self.body_rx.close();
    // drain messages inner channel
    loop {
      match self.body_rx.try_next() {
        Ok(Some(_)) => {}
        Ok(None) => break,
        Err(_) => continue,
      }
    }

    // remove Sender from HashMap
    self.drop_handle.drop(&self.id);
  }
}

/// An Id to specify the [`ClipboardStream`].
#[derive(Debug, Clone, Eq, Hash, PartialEq)]
pub struct StreamId(pub(crate) usize);
