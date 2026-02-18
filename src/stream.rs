use crate::*;

/// Asynchronous stream for the content of the system clipboard.
///
/// When the clipboard is updated, the [`ClipboardStream`] polls for the yields the new data.
#[derive(Debug)]
pub struct ClipboardStream {
  pub(crate) id: StreamId,
  pub(crate) body_rx: Pin<Box<Receiver<ClipboardResult>>>,
  pub(crate) body_senders: Arc<BodySenders>,
}

impl Stream for ClipboardStream {
  type Item = ClipboardResult;

  #[inline]
  fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
    self.body_rx.as_mut().poll_next(cx)
  }
}

impl Drop for ClipboardStream {
  fn drop(&mut self) {
    self.body_senders.unregister(&self.id);
  }
}

/// An Id to specify the [`ClipboardStream`].
#[derive(Debug, Clone, Eq, Hash, PartialEq)]
pub(crate) struct StreamId(pub(crate) usize);
