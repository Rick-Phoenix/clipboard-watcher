use std::sync::Arc;

use crate::body::BodySenders;

/// A trait observing clipboard change event and send data to receiver([`ClipboardStream`])
pub(super) trait Observer {
  fn observe(&mut self, body_senders: Arc<BodySenders>);
}
