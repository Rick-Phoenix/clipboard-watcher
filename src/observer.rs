use std::sync::Arc;

use crate::BodySenders;

/// A trait for observing clipboard changes and report any new events to a list of subscribers.
pub(super) trait Observer {
	fn observe(&mut self, body_senders: Arc<BodySenders>);
}
