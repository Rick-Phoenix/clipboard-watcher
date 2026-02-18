use crate::*;
use percent_encoding::percent_decode;
use std::time::Instant;
use x11rb::{
  CURRENT_TIME,
  connection::Connection,
  protocol::{
    Event, xfixes,
    xproto::{Atom, ConnectionExt, CreateWindowAux, EventMask, Property, WindowClass},
  },
  rust_connection::RustConnection,
};

pub(crate) struct LinuxObserver<G: Gatekeeper = DefaultGatekeeper> {
  stop_signal: Arc<AtomicBool>,
  interval: Duration,
  max_size: Option<u32>,
  custom_formats: Formats,
  x11: X11Context,
  atoms_cache: HashMap<Atom, Arc<str>>,
  gatekeeper: G,
}

pub(crate) struct X11Context {
  conn: RustConnection,
  win_id: u32,
  atoms: Atoms,
}

impl ClipboardContext<'_> {
  /// Attempts to extract the data for a particular [`Format`].
  #[must_use]
  #[inline]
  pub fn get_data(&self, format: &Format) -> Option<Vec<u8>> {
    self
      .x11
      .request_and_read_property(format.id, self.x11.atoms.DATA)
      .ok()
  }
}

const DEFAULT_TIMEOUT: Duration = Duration::from_secs(3);

impl<G: Gatekeeper> LinuxObserver<G> {
  #[inline(never)]
  #[cold]
  pub(crate) fn new(
    stop: Arc<AtomicBool>,
    interval: Option<Duration>,
    max_size: Option<u32>,
    custom_formats: Vec<Arc<str>>,
    gatekeeper: G,
  ) -> Result<Self, String> {
    let (conn, screen_id) = x11rb::connect(None).context("Failed to connect to the x11 server")?;

    let win_id = conn
      .generate_id()
      .context("Failed to generate a window id")?;

    {
      let screen = conn
        .setup()
        .roots
        .get(screen_id)
        .context("Failed to get the root window")?;

      conn
        .create_window(
          0,
          win_id,
          screen.root,
          0,
          0,
          1,
          1,
          0,
          WindowClass::INPUT_OUTPUT,
          screen.root_visual,
          &CreateWindowAux::new()
            .event_mask(EventMask::STRUCTURE_NOTIFY | EventMask::PROPERTY_CHANGE),
        )
        .context("Failed to create a new x11 window")?
        .check()
        .context("Failed to create a new x11 window")?;
    }

    let atoms = Atoms::new(&conn)
      .context("Failed to get the atoms identifiers")?
      .reply()
      .context("Failed to get the atoms identifiers")?;

    let custom_formats = register_custom_formats(&conn, custom_formats)?;
    let mut atoms_cache: HashMap<u32, Arc<str>> = HashMap::new();

    for format in &custom_formats {
      atoms_cache.insert(format.id, format.name.clone());
    }

    let screen = conn
      .setup()
      .roots
      .get(screen_id)
      .context("Failed to connect to the root window")?;

    // Check xfixes presence
    xfixes::query_version(&conn, 5, 0).context("Failed to query xfixes version")?;

    // Watch for events on the clipboard
    // Cookie = request id
    let cookie = xfixes::select_selection_input(
      &conn,
      screen.root,
      atoms.CLIPBOARD,
      xfixes::SelectionEventMask::SET_SELECTION_OWNER,
    )
    .context("Failed to select selection input with xfixes")?;

    cookie
      .check()
      .context("Failed to get response from the X11 server")?;

    Ok(Self {
      stop_signal: stop,
      interval: interval.unwrap_or_else(|| std::time::Duration::from_millis(200)),
      max_size,
      custom_formats,
      atoms_cache,
      x11: X11Context {
        conn,
        win_id,
        atoms,
      },
      gatekeeper,
    })
  }
}

impl<G: Gatekeeper> Observer for LinuxObserver<G> {
  fn observe(&mut self, body_senders: Arc<BodySenders>) {
    info!("Started monitoring the clipboard");

    while !self.stop_signal.load(Ordering::Relaxed) {
      match self.x11.conn.poll_for_event() {
        Ok(event) => {
          if let Some(Event::XfixesSelectionNotify(notify_event)) = event
            && notify_event.selection == self.x11.atoms.CLIPBOARD
          {
            match self.poll_clipboard() {
              Ok(Some(content)) => body_senders.send_all(&Ok(Arc::new(content))),

              // Skipped content (size too large, empty, etc)
              Ok(None) => {}

              // Read error
              Err(e) => {
                warn!("{e}");

                body_senders.send_all(&Err(e));
              }
            }
          }
        }
        Err(e) => {
          error!("{e}");

          body_senders.send_all(&Err(ClipboardError::MonitorFailed(e.to_string())));

          error!("Fatal error, terminating clipboard watcher");
          break;
        }
      };

      std::thread::sleep(self.interval);
    }
  }
}

impl<G: Gatekeeper> LinuxObserver<G> {
  // Calls the extractor and unwraps the error
  fn poll_clipboard(&mut self) -> Result<Option<Body>, ClipboardError> {
    match self.extract_clipboard_content() {
      Ok(Some(content)) => Ok(Some(content)),

      // No content or non-fatal errors
      Ok(None) | Err(ErrorWrapper::SizeTooLarge | ErrorWrapper::UserSkipped) => Ok(None),

      Err(ErrorWrapper::EmptyContent) => {
        trace!("Found empty content. Skipping it...");
        Ok(None)
      }

      Err(ErrorWrapper::ReadError(e)) => Err(e),
    }
  }

  // Tries to extract the contents of the clipboard, and returns an error
  // wrapper that can indicate a normal early exit or an actual error
  fn extract_clipboard_content(&mut self) -> Result<Option<Body>, ErrorWrapper> {
    let formats = self.get_available_formats()?;

    let ctx = ClipboardContext {
      formats: &formats,
      x11: &self.x11,
    };

    if !self.gatekeeper.check(ctx) {
      return Err(ErrorWrapper::UserSkipped);
    }

    for format in self.custom_formats.iter() {
      if formats.contains_id(format.id) {
        let data = self
          .x11
          .read_format_with_size_check(format.id, &formats, self.max_size)?;

        return Ok(Some(Body::new_custom(format.name.clone(), data)));
      }
    }

    if formats.contains_id(self.x11.atoms.PNG_MIME) {
      let bytes =
        self
          .x11
          .read_format_with_size_check(self.x11.atoms.PNG_MIME, &formats, self.max_size)?;

      let path = if formats.contains_id(self.x11.atoms.FILE_LIST)
        && let Ok(mut files) = self.x11.extract_file_list()
        && files.len() == 1
      {
        Some(files.remove(0))
      } else {
        None
      };

      Ok(Some(Body::new_png(bytes, path)))
    } else if formats.contains_id(self.x11.atoms.FILE_LIST) {
      let files = self.x11.extract_file_list()?;

      Ok(Some(Body::new_file_list(files)))
    } else if formats.contains_id(self.x11.atoms.HTML) {
      let bytes = self
        .x11
        .request_and_read_property(self.x11.atoms.HTML, self.x11.atoms.DATA)?;

      let html = String::from_utf8_lossy(&bytes);

      Ok(Some(Body::new_html(html.into_owned())))
    } else if let Some(format) = self.x11.available_text_format(&formats) {
      let bytes = self
        .x11
        .request_and_read_property(format, self.x11.atoms.DATA)?;

      let text = String::from_utf8_lossy(&bytes);

      Ok(Some(Body::new_text(text.into_owned())))
    } else {
      Err(ErrorWrapper::ReadError(ClipboardError::NoMatchingFormat))
    }
  }

  fn get_available_formats(&mut self) -> Result<Formats, ErrorWrapper> {
    let prop_reply = self
      .x11
      .request_and_read_property(self.x11.atoms.TARGETS, self.x11.atoms.METADATA)?;

    let ignored_formats = [
      self.x11.atoms.TIMESTAMP,
      self.x11.atoms.MULTIPLE,
      self.x11.atoms.TARGETS,
      self.x11.atoms.SAVE_TARGETS,
    ];

    // Convert the Vec<u8> into a Vec<Atom>
    let available_formats: Vec<Atom> = prop_reply
      // Split in chunks of 4 bytes
      .chunks_exact(4)
      .map(|chunk| u32::from_ne_bytes(chunk.try_into().unwrap()))
      .filter(|atom| !ignored_formats.contains(atom))
      .collect();

    self.resolve_atom_names(&available_formats)
  }

  fn resolve_atom_names(&mut self, atoms: &[Atom]) -> Result<Formats, ErrorWrapper> {
    let mut formats: Vec<Format> = Vec::new();
    let mut missing_atoms: Vec<Atom> = Vec::new();

    for atom in atoms {
      if let Some(name) = self.atoms_cache.get(atom) {
        formats.push(Format {
          id: *atom,
          name: name.clone(),
        });
      } else {
        missing_atoms.push(*atom);
      }
    }

    let mut cookies = Vec::with_capacity(missing_atoms.len());

    // Send all requests at once
    // This is non-blocking. It just fills the outgoing buffer.
    for atom in missing_atoms {
      // .get_atom_name() returns a Cookie immediately
      let Ok(cookie) = self.x11.conn.get_atom_name(atom) else {
        continue;
      };

      cookies.push((atom, cookie));
    }

    // Collect all replies
    // The X Server processes requests in order.
    for (atom, cookie) in cookies {
      // .reply() blocks until THIS specific answer arrives.
      // Since we sent them all first, the network latency is amortized.
      let Ok(reply) = cookie.reply() else {
        continue;
      };

      // X11 returns raw bytes (usually ISO-8859-1 or UTF-8 depending on age)
      // String::from_utf8_lossy is usually safe enough for clipboard atom names
      let name: Arc<str> = String::from_utf8_lossy(&reply.name).into_owned().into();

      self.atoms_cache.insert(atom, name.clone());

      formats.push(Format { id: atom, name });
    }

    Ok(Formats { data: formats })
  }
}

x11rb::atom_manager! {
  pub Atoms: AtomCookies {
  // Atom to select the clipboard as a whole
  CLIPBOARD,

  // Ignored formats
  MULTIPLE,
  SAVE_TARGETS,
  TIMESTAMP,

  // Property slot names (arbitrary, just for organization)
  //
  // For requesting metadata such as length
  METADATA,
  // For requesting actual clipboard content
  DATA,

  // Metadata formats
  //
  // Available formats
  TARGETS,
  // Length of content
  LENGTH,
  // Information about an atom
  ATOM,
  // Type of response
  INCR,

  // Content formats
  //
  UTF8_STRING,
  UTF8_MIME_0: b"text/plain;charset=utf-8",
  UTF8_MIME_1: b"text/plain;charset=UTF-8",

  HTML: b"text/html",
  PNG_MIME: b"image/png",
  FILE_LIST: b"text/uri-list",
  }
}

fn to_read_error<T: Display>(error: T) -> ErrorWrapper {
  ErrorWrapper::ReadError(ClipboardError::ReadError(error.to_string()))
}

// Needs to be a pure fn because it's used in the constructor
fn register_custom_formats(
  conn: &RustConnection,
  format_names: Vec<Arc<str>>,
) -> Result<Formats, String> {
  let mut data: Vec<Format> = Vec::with_capacity(format_names.len());

  for name in format_names {
    // Send the request to the server to get the Atom for this string.
    let cookie = conn
      // false` means "create `it if it doesn't exist"
      .intern_atom(false, name.as_bytes())
      .map_err(|e| format!("Failed to register custom format `{name}`: {e}"))?;

    let reply = cookie
      .reply()
      .map_err(|e| format!("Failed to register custom format `{name}`: {e}"))?;

    data.push(Format {
      id: reply.atom,
      name,
    });
  }

  Ok(Formats { data })
}

impl X11Context {
  fn extract_file_list(&self) -> Result<Vec<PathBuf>, ErrorWrapper> {
    let raw_data = self.request_and_read_property(self.atoms.FILE_LIST, self.atoms.DATA)?;

    Ok(paths_from_uri_list(&raw_data))
  }

  // Gets the first available plain text format
  fn available_text_format(&self, available_formats: &Formats) -> Option<Atom> {
    [
      self.atoms.UTF8_MIME_0,
      self.atoms.UTF8_MIME_1,
      self.atoms.UTF8_STRING,
    ]
    .into_iter()
    .find(|&format| available_formats.contains_id(format))
  }

  // Reads the actual data of a property
  fn read_property_data(&self, property_atom: Atom) -> Result<Vec<u8>, ErrorWrapper> {
    let start_time = Instant::now();
    let mut buffer = Vec::new();

    // First, peek to see if this is an INCR transfer.
    let initial_reply = self
      .conn
      .get_property(false, self.win_id, property_atom, x11rb::NONE, 0, u32::MAX)
      .map_err(to_read_error)?
      .reply()
      .map_err(to_read_error)?;

    if initial_reply.type_ == self.atoms.INCR {
      // --- INCR Path ---
      // We must delete the INCR marker to start the transfer.
      self
        .conn
        .delete_property(self.win_id, property_atom)
        .map_err(to_read_error)?
        .check()
        .map_err(to_read_error)?;

      loop {
        if start_time.elapsed() > DEFAULT_TIMEOUT {
          return Err(to_read_error("Timeout during INCR transfer"));
        }

        let event = self.conn.poll_for_event().map_err(to_read_error)?; // Don't need sequence number here
        if let Some(Event::PropertyNotify(ev)) = event {
          if ev.atom == property_atom && ev.state == Property::NEW_VALUE {
            let chunk_reply = self
              .conn
              .get_property(true, self.win_id, property_atom, x11rb::NONE, 0, u32::MAX)
              .map_err(to_read_error)?
              .reply()
              .map_err(to_read_error)?;
            if chunk_reply.value.is_empty() {
              break; // End of transfer
            }
            buffer.extend_from_slice(&chunk_reply.value);
          }
        } else {
          std::thread::sleep(Duration::from_millis(20));
        }
      }
    } else {
      // --- Normal Path ---
      // The data is all in the property we already peeked at.
      buffer.extend_from_slice(&initial_reply.value);
      // We now must clean up the property.
      self
        .conn
        .delete_property(self.win_id, property_atom)
        .map_err(to_read_error)?
        .check()
        .map_err(to_read_error)?;
    }

    Ok(buffer)
  }

  // Attempts to extract a specific format from the clipboard while checking for the max size
  fn read_format_with_size_check(
    &self,
    format_to_read: Atom,
    available_formats: &Formats,
    max_size: Option<u32>,
  ) -> Result<Vec<u8>, ErrorWrapper> {
    // 1. Try the cheap size verification first
    if let Some(max_size) = max_size
      && available_formats.contains_id(self.atoms.LENGTH)
    {
      let size_bytes = self.request_and_read_property(self.atoms.LENGTH, self.atoms.METADATA)?;

      if size_bytes.len() >= 4 {
        let size = u32::from_ne_bytes(size_bytes[0..4].try_into().unwrap());

        if size == 0 {
          return Err(ErrorWrapper::EmptyContent);
        }

        if size > max_size {
          debug!(
            "Found content with {} size, beyond maximum allowed size. Skipping it...",
            HumanBytes(size as usize)
          );

          return Err(ErrorWrapper::SizeTooLarge);
        }
        // Size is OK, now we must do a *second* request for the actual data.
        return self.request_and_read_property(format_to_read, self.atoms.DATA);
      }
    }

    // 2. If unsuccessful, use the more inefficient method to try and read the size.
    // Make the request, but don't read the data yet.
    let data_prop = self.request_property(format_to_read, self.atoms.DATA)?;

    if let Some(max_size) = max_size {
      // 3. Use the size helper to "peek" at the size.
      let size = self.get_property_size(data_prop)?;

      if size == 0 {
        return Err(ErrorWrapper::EmptyContent);
      }

      // 4. Make a decision based on the size.
      if size > max_size {
        debug!(
          "Found content with {} size, beyond maximum allowed size. Skipping it...",
          HumanBytes(size as usize)
        );

        // Size is too large. We MUST clean up the property we created.
        self
          .conn
          .delete_property(self.win_id, data_prop)
          .map_err(to_read_error)?
          .check()
          .map_err(to_read_error)?;
        return Err(ErrorWrapper::SizeTooLarge);
      }
    }

    // Size is OK! Proceed to read the full data from the waiting property.
    self.read_property_data(data_prop)
  }

  // Requests the property without reading it (useful for checking the size
  // in case the LENGTH atom is not supported by the clipboard owner)
  fn request_property(
    &self,
    format_to_request: Atom,
    property_name: Atom,
  ) -> Result<Atom, ErrorWrapper> {
    let start_time = Instant::now();
    let cookie = self
      .conn
      .convert_selection(
        self.win_id,
        self.atoms.CLIPBOARD,
        format_to_request,
        property_name,
        CURRENT_TIME,
      )
      .map_err(to_read_error)?;

    let sequence_number = cookie.sequence_number();

    // Flush requests before checking for the response
    self.conn.flush().map_err(to_read_error)?;

    loop {
      if start_time.elapsed() > DEFAULT_TIMEOUT {
        return Err(to_read_error("Timeout waiting for SelectionNotify event"));
      }

      let event_with_seq = self
        .conn
        .poll_for_event_with_sequence()
        .map_err(to_read_error)?;

      if let Some((event, seq)) = event_with_seq {
        if seq < sequence_number {
          continue;
        }

        if let Event::SelectionNotify(ev) = event
          && ev.requestor == self.win_id
          && ev.selection == self.atoms.CLIPBOARD
        {
          if ev.property == x11rb::NONE {
            return Err(to_read_error("Clipboard owner failed to convert selection"));
          }
          // Success! The data is on the server. Return the property's name,
          // which can later be used to inspect or get the data
          return Ok(ev.property);
        }
      } else {
        std::thread::sleep(Duration::from_millis(20));
      }
    }
  }

  // Fallback method to check for the size of an item when the LENGTH
  // request was unsuccessful
  fn get_property_size(&self, property_atom: Atom) -> Result<u32, ErrorWrapper> {
    let prop_reply = self
      .conn
      .get_property(
        false, // `false` is critical: do not delete the property.
        self.win_id,
        property_atom,
        x11rb::NONE,
        0,
        0, // Ask for zero bytes.
      )
      .map_err(to_read_error)?
      .reply()
      .map_err(to_read_error)?;

    // The total size is in the `bytes_after` field.
    Ok(prop_reply.bytes_after)
  }

  fn request_and_read_property(
    &self,
    format_to_read: Atom,
    property_name: Atom,
  ) -> Result<Vec<u8>, ErrorWrapper> {
    let property_atom = self.request_property(format_to_read, property_name)?;

    self.read_property_data(property_atom)
  }
}

// From [arboard](https://github.com/1Password/arboard), with modifications
fn paths_from_uri_list(uri_list: &[u8]) -> Vec<PathBuf> {
  uri_list
    .split(|char| *char == b'\n')
    // Removing any trailing \r that might be captured
    .map(|line| line.strip_suffix(b"\r").unwrap_or(line))
    .filter_map(|line| line.strip_prefix(b"file://"))
    .filter_map(|s| percent_decode(s).decode_utf8().ok())
    .map(|decoded| PathBuf::from(decoded.as_ref()))
    .collect()
}
