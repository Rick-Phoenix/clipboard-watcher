use crate::*;

// A wrapper for a mutex of HashMap that contains all of the registered receivers
// for a given listener.
#[derive(Debug)]
pub(crate) struct BodySenders {
	senders: Mutex<HashMap<StreamId, Sender<ClipboardResult>>>,
}

impl BodySenders {
	pub(crate) fn new() -> Self {
		Self {
			senders: Mutex::default(),
		}
	}

	/// Register Sender that was specified [`StreamId`].
	pub(crate) fn register(&self, id: StreamId, tx: Sender<ClipboardResult>) {
		let mut guard = self.senders.lock().unwrap();
		guard.insert(id, tx);
	}

	/// Close channel and unregister sender that was specified [`StreamId`]
	pub(crate) fn unregister(&self, id: &StreamId) {
		let mut guard = self.senders.lock().unwrap();
		guard.remove(id);
	}

	pub(crate) fn send_all(&self, result: ClipboardResult) {
		let mut senders = self.senders.lock().unwrap();

		for sender in senders.values_mut() {
			match sender.try_send(result.clone()) {
				Ok(()) => {}
				Err(e) => error!("Failed to send the clipboard data: {e}"),
			};
		}
	}
}
