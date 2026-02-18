use crate::{linux::observer::LinuxObserver, *};

impl Driver {
  #[inline(never)]
  #[cold]
  /// Construct [`Driver`] and spawn a thread for monitoring clipboard events
  pub(crate) fn new<G: Gatekeeper>(
    body_senders: Arc<BodySenders>,
    interval: Option<Duration>,
    custom_formats: Vec<Arc<str>>,
    max_bytes: Option<u32>,
    gatekeeper: G,
  ) -> Result<Self, InitializationError> {
    let stop = Arc::new(AtomicBool::new(false));

    let stop_cl = stop.clone();

    let (init_tx, init_rx) = sync_channel(0);

    let handle = std::thread::spawn(move || {
      match LinuxObserver::new(stop_cl, interval, max_bytes, custom_formats, gatekeeper) {
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
      Ok(Ok(())) => Ok(Self {
        stop,
        handle: Some(handle),
      }),
      Ok(Err(e)) => Err(InitializationError(e)),
      Err(e) => Err(InitializationError(e.to_string())),
    }
  }
}
