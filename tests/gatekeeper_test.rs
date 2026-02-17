#![allow(
  clippy::ignored_unit_patterns,
  clippy::cast_possible_truncation,
  clippy::cast_possible_wrap
)]

use clipboard_watcher::ClipboardEventListener;
use futures::StreamExt;
use serial_test::serial;
use std::time::Duration;

enum FlagKind {
  ExcludeClipboard,
  CanInclude,
}

#[cfg(windows)]
mod win {
  use super::*;

  use clipboard_win::raw::register_format;
  use clipboard_win::{Clipboard, Setter, formats};

  #[tokio::test]
  async fn gatekeeper_win_1() {
    let mut event_listener = ClipboardEventListener::builder()
      .with_gatekeeper(|ctx| {
        if ctx.has_format("ExcludeClipboardContentFromMonitorProcessing") {
          return false;
        }

        true
      })
      .spawn()
      .unwrap();

    let mut stream = event_listener.new_stream(5);

    set_private_clipboard_win(FlagKind::ExcludeClipboard).unwrap();

    let result = tokio::time::timeout(Duration::from_secs(2), stream.next()).await;

    match result {
      Ok(Some(_)) => {
        panic!("ExcludeClipboardContentFromMonitorProcessing was not detected");
      }
      Ok(None) => {
        panic!("Channel was closed prematurely");
      }
      Err(_) => {}
    };
  }

  #[tokio::test]
  #[serial]
  async fn gatekeeper_win_2() {
    let mut event_listener = ClipboardEventListener::builder()
      .with_gatekeeper(|ctx| {
        if let Some(can_include_flag) = ctx.get_u32("CanIncludeInClipboardHistory")
          && can_include_flag == 0
        {
          return false;
        }

        true
      })
      .spawn()
      .unwrap();

    let mut stream = event_listener.new_stream(5);

    set_private_clipboard_win(FlagKind::CanInclude).unwrap();

    let result = tokio::time::timeout(Duration::from_secs(2), stream.next()).await;

    match result {
      Ok(Some(_)) => {
        panic!("CanIncludeInClipboardHistory was not detected");
      }
      Ok(None) => {
        panic!("Channel was closed prematurely");
      }
      Err(_) => {}
    };
  }

  #[allow(clippy::needless_pass_by_value)]
  fn set_private_clipboard_win(flag: FlagKind) -> Result<(), String> {
    let _clip =
      Clipboard::new_attempts(10).map_err(|e| format!("Failed to open clipboard: {e}"))?;

    clipboard_win::empty().map_err(|e| format!("Failed to empty clipboard: {e}"))?;

    formats::Unicode
      .write_clipboard(&"this should have been ignored")
      .map_err(|e| format!("Failed to write text: {e}"))?;

    match flag {
      FlagKind::ExcludeClipboard => {
        let exclude_id = register_format("ExcludeClipboardContentFromMonitorProcessing")
          .ok_or("Failed to register Exclude format")?;

        let bytes = [1u8; 1];
        formats::RawData(exclude_id.get())
          .write_clipboard(&bytes)
          .map_err(|e| {
            format!("Failed to write ExcludeClipboardContentFromMonitorProcessing flag: {e}")
          })?;
      }
      FlagKind::CanInclude => {
        let include_id = register_format("CanIncludeInClipboardHistory")
          .ok_or("Failed to register CanInclude format")?;

        let value: u32 = 0;
        let bytes = value.to_le_bytes();

        formats::RawData(include_id.get())
          .write_clipboard(&bytes)
          .map_err(|e| format!("Failed to write CanIncludeInClipboardHistory flag: {e}"))?;
      }
    }

    Ok(())
  }
}

#[cfg(target_os = "linux")]
mod linux {
  use super::*;

  use std::thread;
  use x11rb::connection::Connection;
  use x11rb::protocol::Event;
  use x11rb::protocol::xproto::{
    AtomEnum, ConnectionExt, EventMask, PropMode, SelectionNotifyEvent, Time, WindowClass,
  };
  use x11rb::rust_connection::RustConnection;
  use x11rb::wrapper::ConnectionExt as WrapperExt;

  #[tokio::test]
  #[serial]
  async fn gatekeeper_linux_1() {
    let _owner_handle = spawn_x11_privacy_owner(FlagKind::ExcludeClipboard);
    tokio::time::sleep(Duration::from_millis(100)).await;

    let mut event_listener = ClipboardEventListener::builder()
      .with_gatekeeper(|ctx| {
        if ctx.has_format("ExcludeClipboardContentFromMonitorProcessing") {
          return false;
        }

        true
      })
      .spawn()
      .unwrap();

    let mut stream = event_listener.new_stream(5);

    let result = tokio::time::timeout(Duration::from_secs(2), stream.next()).await;

    match result {
      Ok(Some(_)) => {
        panic!("CanIncludeInClipboardHistory was not detected");
      }
      Ok(None) => {
        panic!("Stream was closed prematurely");
      }
      Err(_) => {}
    };
  }

  #[tokio::test]
  #[serial]
  async fn gatekeeper_linux_2() {
    let _owner_handle = spawn_x11_privacy_owner(FlagKind::CanInclude);
    tokio::time::sleep(Duration::from_millis(100)).await;

    let mut event_listener = ClipboardEventListener::builder()
      .with_gatekeeper(|ctx| {
        if let Some(can_include_flag) = ctx.get_u32("CanIncludeInClipboardHistory")
          && can_include_flag == 0
        {
          return false;
        }

        true
      })
      .spawn()
      .unwrap();

    let mut stream = event_listener.new_stream(5);

    let result = tokio::time::timeout(Duration::from_secs(2), stream.next()).await;

    match result {
      Ok(Some(_)) => {
        panic!("CanIncludeInClipboardHistory was not detected");
      }
      Ok(None) => {
        panic!("Stream was closed prematurely");
      }
      Err(_) => {}
    };
  }

  fn spawn_x11_privacy_owner(flag: FlagKind) -> thread::JoinHandle<()> {
    thread::spawn(move || {
      let (conn, screen_num) = RustConnection::connect(None).unwrap();
      let screen = &conn.setup().roots[screen_num];

      // 1. Create a dummy window to own the selection
      let win_id = conn.generate_id().unwrap();
      conn
        .create_window(
          x11rb::COPY_FROM_PARENT as u8,
          win_id,
          screen.root,
          0,
          0,
          1,
          1,
          0,
          WindowClass::INPUT_OUTPUT,
          x11rb::COPY_FROM_PARENT,
          &Default::default(),
        )
        .unwrap();

      // 2. Intern Atoms (Formats)
      let clipboard_atom = conn
        .intern_atom(false, b"CLIPBOARD")
        .unwrap()
        .reply()
        .unwrap()
        .atom;
      let targets_atom = conn
        .intern_atom(false, b"TARGETS")
        .unwrap()
        .reply()
        .unwrap()
        .atom;
      let utf8_atom = conn
        .intern_atom(false, b"UTF8_STRING")
        .unwrap()
        .reply()
        .unwrap()
        .atom;

      let exclude_atom = conn
        .intern_atom(false, b"ExcludeClipboardContentFromMonitorProcessing")
        .unwrap()
        .reply()
        .unwrap()
        .atom;
      let include_atom = conn
        .intern_atom(false, b"CanIncludeInClipboardHistory")
        .unwrap()
        .reply()
        .unwrap()
        .atom;

      // 3. Claim Ownership
      conn
        .set_selection_owner(win_id, clipboard_atom, Time::CURRENT_TIME)
        .unwrap();
      conn.flush().unwrap();

      // 4. Event Loop (Answer Requests)
      while let Ok(event) = conn.wait_for_event() {
        match event {
          Event::SelectionRequest(req) => {
            if req.target == targets_atom {
              let mut targets = vec![targets_atom, utf8_atom];
              match flag {
                FlagKind::CanInclude => {
                  targets.push(include_atom);
                }
                FlagKind::ExcludeClipboard => {
                  targets.push(exclude_atom);
                }
              };

              conn
                .change_property32(
                  PropMode::REPLACE,
                  req.requestor,
                  req.property,
                  AtomEnum::ATOM,
                  &targets,
                )
                .unwrap();
            } else if req.target == utf8_atom {
              conn
                .change_property8(
                  PropMode::REPLACE,
                  req.requestor,
                  req.property,
                  utf8_atom,
                  "this should have been ignored".as_bytes(),
                )
                .unwrap();
            } else if req.target == exclude_atom {
              conn
                .change_property8(
                  PropMode::REPLACE,
                  req.requestor,
                  req.property,
                  exclude_atom,
                  &[],
                )
                .unwrap();
            } else if req.target == include_atom {
              let zero: u32 = 0;
              conn
                .change_property32(
                  PropMode::REPLACE,
                  req.requestor,
                  req.property,
                  AtomEnum::INTEGER,
                  &[zero],
                )
                .unwrap();
            }

            let notify = SelectionNotifyEvent {
              response_type: x11rb::protocol::xproto::SELECTION_NOTIFY_EVENT,
              sequence: 0,
              time: req.time,
              requestor: req.requestor,
              selection: req.selection,
              target: req.target,
              property: req.property,
            };

            conn
              .send_event(false, req.requestor, EventMask::NO_EVENT, notify)
              .unwrap();
            conn.flush().unwrap();
          }
          Event::SelectionClear(_) => {
            // Someone else claimed the clipboard. We can exit.
            break;
          }
          _ => {}
        }
      }
    })
  }
}

#[cfg(target_os = "macos")]
mod macos {
  use super::*;

  use objc2::rc::Retained;
  use objc2_app_kit::{NSPasteboard, NSPasteboardTypeString};
  use objc2_foundation::{NSArray, NSData, NSString};

  #[tokio::test]
  #[serial]
  async fn gatekeeper_macos_1() {
    set_private_clipboard_mac(FlagKind::ExcludeClipboard);

    tokio::time::sleep(Duration::from_millis(100)).await;

    let mut event_listener = ClipboardEventListener::builder()
      .with_gatekeeper(|ctx| {
        if ctx.has_format("ExcludeClipboardContentFromMonitorProcessing") {
          return false;
        }

        true
      })
      .spawn()
      .unwrap();

    let mut stream = event_listener.new_stream(5);

    let result = tokio::time::timeout(Duration::from_secs(2), stream.next()).await;

    match result {
      Ok(Some(_)) => {
        panic!("ExcludeClipboardContentFromMonitorProcessing was not detected");
      }
      Ok(None) => {
        panic!("Stream was closed prematurely");
      }
      Err(_) => {}
    };
  }

  #[tokio::test]
  #[serial]
  async fn gatekeeper_macos_2() {
    set_private_clipboard_mac(FlagKind::CanInclude);

    tokio::time::sleep(Duration::from_millis(100)).await;

    let mut event_listener = ClipboardEventListener::builder()
      .with_gatekeeper(|ctx| {
        if let Some(can_include_flag) = ctx.get_u32("CanIncludeInClipboardHistory")
          && can_include_flag == 0
        {
          return false;
        }

        true
      })
      .spawn()
      .unwrap();

    let mut stream = event_listener.new_stream(5);

    let result = tokio::time::timeout(Duration::from_secs(2), stream.next()).await;

    match result {
      Ok(Some(_)) => {
        panic!("CanIncludeInClipboardHistory was not detected");
      }
      Ok(None) => {
        panic!("Stream was closed prematurely");
      }
      Err(_) => {}
    };
  }

  pub fn set_private_clipboard_mac(flag: FlagKind) {
    unsafe {
      let pb = NSPasteboard::generalPasteboard();

      let type_text = NSPasteboardTypeString;
      let flag_name = match flag {
        FlagKind::CanInclude => "CanIncludeInClipboardHistory",
        FlagKind::ExcludeClipboard => "ExcludeClipboardContentFromMonitorProcessing",
      };

      let type_flag = NSString::from_str(flag_name);

      // 1. Declare types
      let types = NSArray::from_slice(&[type_text, &*type_flag]);
      pb.declareTypes_owner(&types, None);

      // 2. Write Text
      pb.setString_forType(
        &NSString::from_str("this should have been ignored"),
        type_text,
      );

      // 3. Write Data (The u32 value)
      let value: u32 = 0;
      let bytes = value.to_le_bytes();

      // Create NSData from the slice
      let data = NSData::with_bytes(&bytes);

      pb.setData_forType(Some(&data), &type_flag);
    }
  }
}
