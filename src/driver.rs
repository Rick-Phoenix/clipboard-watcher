use std::{
  sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
  },
  thread::JoinHandle,
};

/// The struct that is responsible for starting and stopping the Observer.
#[derive(Debug)]
pub(crate) struct Driver {
  /// This is cloned and passed to the Observer threads to give them the interruption signal
  pub(crate) stop: Arc<AtomicBool>,

  /// This is the handle of the spawned Observer thread.
  pub(crate) handle: Option<JoinHandle<()>>,
}

impl Drop for Driver {
  fn drop(&mut self) {
    // Change the AtomicBool, stop the observers
    self.stop.store(true, Ordering::Relaxed);

    // Wait for the thread to finish
    // We use option + take here because join consumes the value
    if let Some(handle) = self.handle.take() {
      handle.join().unwrap();
    }
  }
}
