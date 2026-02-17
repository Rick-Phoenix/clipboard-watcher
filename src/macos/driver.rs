use std::{
  convert::Infallible,
  sync::{Arc, atomic::AtomicBool},
  time::Duration,
};

use crate::{BodySenders, driver::Driver, macos::observer::OSXObserver, observer::Observer};

impl Driver {
  /// Construct [`Driver`] and spawn a thread for monitoring clipboard events
  pub(crate) fn new(
    body_senders: Arc<BodySenders>,
    interval: Option<Duration>,
    custom_formats: Vec<Arc<str>>,
    max_bytes: Option<u32>,
  ) -> Result<Self, Infallible> {
    let stop = Arc::new(AtomicBool::new(false));

    let stop_cl = stop.clone();

    // spawn OS thread
    // observe clipboard change event and send item
    let handle = std::thread::spawn(move || {
      // construct Observer in thread
      // OSXSys is **not** implemented Send + Sync
      // in order to send Observer, construct it
      let mut observer = OSXObserver::new(stop_cl, interval, custom_formats, max_bytes);

      // event change observe loop
      observer.observe(body_senders);
    });

    Ok(Driver {
      stop,
      handle: Some(handle),
    })
  }
}
