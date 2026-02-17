use crate::{win::observer::WinObserver, *};

impl Driver {
  #[inline(never)]
  #[cold]
  /// Construct [`Driver`] and spawn a thread for monitoring clipboard events
  pub(crate) fn new(
    body_senders: Arc<BodySenders>,
    interval: Option<Duration>,
    custom_formats: Vec<Arc<str>>,
    max_bytes: Option<u32>,
    gatekeeper: Option<Gatekeeper>,
  ) -> Result<Self, InitializationError> {
    use std::sync::mpsc;

    let stop = Arc::new(AtomicBool::new(false));

    let stop_cl = stop.clone();

    let (init_tx, init_rx) = mpsc::sync_channel(0);

    // spawn OS thread
    // observe clipboard change event and send item
    let handle = std::thread::spawn(move || {
      match clipboard_win::Monitor::new() {
        Ok(monitor) => {
          match WinObserver::new(
            stop_cl,
            monitor,
            custom_formats,
            interval,
            max_bytes,
            gatekeeper,
          ) {
            Ok(mut observer) => {
              init_tx.send(Ok(())).unwrap();
              observer.observe(body_senders);
            }
            Err(e) => init_tx.send(Err(e)).unwrap(),
          };
        }
        Err(e) => {
          init_tx.send(Err(e.to_string())).unwrap();
        }
      };
    });

    // Block until we get an init signal
    match init_rx.recv() {
      Ok(Ok(())) => Ok(Driver {
        stop,
        handle: Some(handle),
      }),
      Ok(Err(e)) => Err(InitializationError(e)),
      Err(e) => Err(InitializationError(e.to_string())),
    }
  }
}
