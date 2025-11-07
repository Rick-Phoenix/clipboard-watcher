use std::{
  sync::{atomic::AtomicBool, Arc},
  time::Duration,
};

use crate::{
  body::BodySenders, driver::Driver, error::InitializationError, linux::observer::LinuxObserver,
  observer::Observer,
};

impl Driver {
  /// Construct [`Driver`] and spawn a thread for monitoring clipboard events
  pub(crate) fn new(
    body_senders: Arc<BodySenders>,
    interval: Option<Duration>,
    custom_formats: Vec<impl AsRef<str>>,
    max_bytes: Option<u32>,
  ) -> Result<Self, InitializationError> {
    use std::sync::mpsc;

    let stop = Arc::new(AtomicBool::new(false));

    let stop_cl = stop.clone();

    let (init_tx, init_rx) = mpsc::sync_channel(0);

    let thread_safe_formats_list: Vec<Arc<str>> = custom_formats
      .into_iter()
      .map(|f| f.as_ref().into())
      .collect();

    let handle = std::thread::spawn(move || {
      match LinuxObserver::new(stop_cl, interval, max_bytes, thread_safe_formats_list) {
        Ok(mut observer) => {
          init_tx.send(Ok(())).unwrap();

          observer.observe(body_senders);
        }
        Err(e) => {
          init_tx.send(Err(e)).unwrap();
        }
      };
    });

    // Block until we get an init signal
    match init_rx.recv() {
      Ok(Ok(())) => Ok(Driver {
        stop,
        handle: Some(handle),
      }),
      Ok(Err(e)) => Err(InitializationError(e.to_string())),
      Err(e) => Err(InitializationError(e.to_string())),
    }
  }
}
