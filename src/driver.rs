use std::{
  sync::{Arc, atomic::AtomicBool},
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
