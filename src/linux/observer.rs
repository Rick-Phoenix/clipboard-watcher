use std::{
  collections::HashMap,
  fmt::Display,
  path::PathBuf,
  sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
  },
  time::{Duration, Instant},
};

use log::{debug, error, info, trace};
use percent_encoding::percent_decode;
use x11rb::{
  connection::Connection,
  protocol::{
    xfixes,
    xproto::{Atom, ConnectionExt, CreateWindowAux, EventMask, Property, WindowClass},
    Event,
  },
  rust_connection::RustConnection,
  CURRENT_TIME,
};

use crate::{
  body::BodySenders,
  error::{ClipboardError, ErrorWrapper},
  logging::bytes_to_mb,
  observer::Observer,
  Body,
};

pub(crate) struct LinuxObserver {
  stop: Arc<AtomicBool>,
  interval: Duration,
  max_size: Option<u32>,
  server_context: XServerContext,
  custom_formats: HashMap<Arc<str>, Atom>,
}

struct XServerContext {
  conn: RustConnection,
  win_id: u32,
  screen: usize,
  atoms: Atoms,
}

const DEFAULT_TIMEOUT: Duration = Duration::from_secs(3);

impl LinuxObserver {
  pub(super) fn new(
    stop: Arc<AtomicBool>,
    interval: Option<Duration>,
    max_size: Option<u32>,
    custom_formats: Vec<Arc<str>>,
  ) -> Result<Self, String> {
    let server_context = XServerContext::new()?;

    let custom_formats = server_context.intern_custom_formats(custom_formats)?;

    let screen = server_context
      .conn
      .setup()
      .roots
      .get(server_context.screen)
      .ok_or_else(|| "Failed to connect to the root window".to_string())?;

    // Check xfixes presence
    xfixes::query_version(&server_context.conn, 5, 0)
      .map_err(|e| format!("Failed to query xfixes version: {e}"))?;

    // Watch for events on the clipboard
    // Cookie = request id
    let cookie = xfixes::select_selection_input(
      &server_context.conn,
      screen.root,
      server_context.atoms.CLIPBOARD,
      xfixes::SelectionEventMask::SET_SELECTION_OWNER,
    )
    .map_err(|e| format!("Failed to select selection input with xfixes: {e}"))?;

    cookie
      .check()
      .map_err(|e| format!("Failed to get response from the X11 server: {e}"))?;

    Ok(LinuxObserver {
      stop,
      interval: interval.unwrap_or_else(|| std::time::Duration::from_millis(200)),
      max_size,
      server_context,
      custom_formats,
    })
  }
}

impl Observer for LinuxObserver {
  fn observe(&mut self, body_senders: Arc<BodySenders>) {
    let interval = self.interval;

    info!("Started monitoring the clipboard");

    while !self.stop.load(Ordering::Relaxed) {
      std::thread::sleep(interval);

      match self.server_context.conn.poll_for_event() {
        Ok(event) => {
          if let Some(Event::XfixesSelectionNotify(notify_event)) = event
            && notify_event.selection == self.server_context.atoms.CLIPBOARD
          {
            match self.poll_clipboard() {
              Ok(Some(content)) => body_senders.send_all(Ok(Arc::new(content))),

              // Skipped content (size too large, empty, etc)
              Ok(None)  => {}

              // Read error
              Err(e) => {
                error!("{e}");

                body_senders.send_all(Err(e));
              }
            }
          }
        }
        Err(e) => {
          error!("{e}");

          body_senders.send_all(Err(ClipboardError::MonitorFailed(e.to_string())));

          error!("Fatal error, terminating clipboard watcher");
          break;
        }
      };
    }
  }
}

impl LinuxObserver {
  pub(super) fn poll_clipboard(&self) -> Result<Option<Body>, ClipboardError> {
    match self.get_clipboard_content() {
      Ok(Some(content)) => Ok(Some(content)),

      // No content or non-fatal errors
      Ok(None) | Err(ErrorWrapper::SizeTooLarge) | Err(ErrorWrapper::FormatUnavailable) => Ok(None),
      Err(ErrorWrapper::EmptyContent) => {
        trace!("Found empty content. Skipping it...");
        Ok(None)
      }

      Err(ErrorWrapper::ReadError(e)) => Err(e),
    }
  }

  fn get_clipboard_content(&self) -> Result<Option<Body>, ErrorWrapper> {
    let available_formats = self.server_context.get_available_formats()?;

    for (name, atom) in self.custom_formats.iter() {
      if available_formats.contains(atom) {
        let data = self.server_context.extract_clipboard_content(
          *atom,
          &available_formats,
          self.max_size,
        )?;

        return Ok(Some(Body::new_custom(name.clone(), data)));
      }
    }

    if available_formats.contains(&self.server_context.atoms.PNG_MIME) {
      let bytes = self.server_context.extract_clipboard_content(
        self.server_context.atoms.PNG_MIME,
        &available_formats,
        self.max_size,
      )?;

      let path = if let Ok(mut files) = self.server_context.extract_file_list(&available_formats) && files.len() == 1 {
        Some(files.remove(0))
      } else{
        None
      };

      Ok(Some(Body::new_image(bytes, path)))
    } else if available_formats.contains(&self.server_context.atoms.FILE_LIST) {
      let files = self.server_context.extract_file_list(&available_formats)?;

      Ok(Some(Body::new_file_list(files)))
    } else if available_formats.contains(&self.server_context.atoms.HTML) {
      let bytes = self.server_context.extract_clipboard_content(
        self.server_context.atoms.HTML,
        &available_formats,
        None,
      )?;

      let html = String::from_utf8_lossy(&bytes);

      Ok(Some(Body::new_html(html.into_owned())))
    } else if let Some(format) = self
      .server_context
      .available_text_format(&available_formats)
    {
      let bytes =
        self
          .server_context
          .extract_clipboard_content(format, &available_formats, None)?;

      let text = String::from_utf8_lossy(&bytes);

      Ok(Some(Body::new_text(text.into_owned())))
    } else {
      Err(ErrorWrapper::ReadError(ClipboardError::NoMatchingFormat))
    }
  }
}

x11rb::atom_manager! {
  pub Atoms: AtomCookies {
    // Selection kinds
    CLIPBOARD,

    // Ignored formats
    MULTIPLE,
    SAVE_TARGETS,
    TIMESTAMP,

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

impl XServerContext {
  fn request_and_read_property(
    &self,
    format_to_read: Atom,
    property_name: Atom,
  ) -> Result<Vec<u8>, ErrorWrapper> {
    let property_atom = self.request_property(format_to_read, property_name)?;

    self.read_property_data(property_atom)
  }

  fn get_available_formats(&self) -> Result<Vec<Atom>, ErrorWrapper> {
    let prop_reply = self.request_and_read_property(self.atoms.TARGETS, self.atoms.METADATA)?;

    let ignored_formats = [
      self.atoms.TIMESTAMP,
      self.atoms.MULTIPLE,
      self.atoms.TARGETS,
      self.atoms.SAVE_TARGETS,
    ];

    // The data is a raw byte buffer. An Atom is a u32 (4 bytes).
    // We need to convert the Vec<u8> into a Vec<Atom>.
    let available_formats: Vec<Atom> = prop_reply
      // Split in chunks of 4 bytes
      .chunks_exact(4)
      .map(|chunk| u32::from_ne_bytes(chunk.try_into().unwrap()))
      .filter(|atom| !ignored_formats.contains(atom))
      .collect();

    Ok(available_formats)
  }

  fn new() -> Result<Self, String> {
    let (conn, screen) =
      x11rb::connect(None).map_err(|e| format!("Failed to connect to the x11 server: {e}"))?;

    let win_id = conn
      .generate_id()
      .map_err(|e| format!("Failed to generate a window id: {e}"))?;

    {
      let screen = conn
        .setup()
        .roots
        .get(screen)
        .ok_or("Failed to get the root window".to_string())?;

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
        .map_err(|e| format!("Failed to create a new x11 window: {e}"))?
        .check()
        .map_err(|e| format!("Failed to create a new x11 window: {e}"))?;
    }

    let atoms = Atoms::new(&conn)
      .map_err(|e| format!("Failed to get the atoms identifiers: {e}"))?
      .reply()
      .map_err(|e| format!("Failed to get the atoms identifiers: {e}"))?;

    Ok(Self {
      conn,
      win_id,
      screen,
      atoms,
    })
  }

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
          && ev.requestor == self.win_id && ev.selection == self.atoms.CLIPBOARD {
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

  fn extract_file_list(&self, available_formats: &[Atom]) -> Result<Vec<PathBuf>, ErrorWrapper> {
    let raw_data = self.extract_clipboard_content(self.atoms.FILE_LIST, available_formats, None)?;

    Ok(paths_from_uri_list(raw_data))
  }

  fn extract_clipboard_content(
    &self,
    format_to_read: Atom,
    available_formats: &[Atom],
    max_size: Option<u32>,
  ) -> Result<Vec<u8>, ErrorWrapper> {
    // 1. Try the cheap size verification first
    if let Some(max_size) = max_size && available_formats.contains(&self.atoms.LENGTH) {
      let size_bytes =
        self.request_and_read_property(self.atoms.LENGTH, self.atoms.METADATA, )?;

      if size_bytes.len() >= 4 {
        let size = usize::from_ne_bytes(size_bytes[0..4].try_into().unwrap());

        if size == 0 {
          return Err(ErrorWrapper::EmptyContent);
        }

        if size as u32 > max_size {
          debug!(
            "Found content with {:.2}MB size, beyond maximum allowed size. Skipping it...",
            bytes_to_mb(size)
          );

          return Err(ErrorWrapper::SizeTooLarge);
        }
        // Size is OK, now we must do a *second* request for the actual data.
        return self.request_and_read_property(format_to_read, self.atoms.DATA, );
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
          "Found content with {:.2}MB size, beyond maximum allowed size. Skipping it...",
          bytes_to_mb(size as usize)
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

  fn intern_custom_formats(
    &self,
    format_names: Vec<Arc<str>>,
  ) -> Result<HashMap<Arc<str>, Atom>, String> {
    let mut atoms_map: HashMap<Arc<str>, Atom> = HashMap::new();

    for name in format_names {
      // Send the request to the server to get the Atom for this string.
      let cookie = self
        .conn
        // false` means "create `it if it doesn't exist"
        .intern_atom(false, name.as_bytes())
        .map_err(|e| format!("Failed to register custom format `{name}`: {e}"))?;

      let reply = cookie
        .reply()
        .map_err(|e| format!("Failed to register custom format `{name}`: {e}"))?;

      atoms_map.insert(name, reply.atom);
    }

    Ok(atoms_map)
  }

  fn available_text_format(&self, available_formats: &[Atom]) -> Option<Atom> {
    [
      self.atoms.UTF8_MIME_0,
      self.atoms.UTF8_MIME_1,
      self.atoms.UTF8_STRING,
    ]
    .into_iter()
    .find(|&format| available_formats.contains(&format))
  }
}

// From [arboard](https://github.com/1Password/arboard)
fn paths_from_uri_list(uri_list: Vec<u8>) -> Vec<PathBuf> {
  uri_list
    .split(|char| *char == b'\n')
    // Removing any trailing \r that might be captured
    .map(|line| line.strip_suffix(b"\r").unwrap_or(line))
    .filter_map(|line| line.strip_prefix(b"file://"))
    .filter_map(|s| percent_decode(s).decode_utf8().ok())
    .map(|decoded| PathBuf::from(decoded.as_ref()))
    .collect()
}
