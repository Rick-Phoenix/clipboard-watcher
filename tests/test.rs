use std::{
  io::{Cursor, Write},
  process::{Command, Stdio},
  time::Duration,
};

use arboard::Clipboard;
use clipboard_watcher::{Body, ClipboardEventListener, RawImage};
use futures::StreamExt;
use image::{ImageFormat, RgbImage};
use log::debug;
use tokio::sync::mpsc;

fn init_logging() {
  let _ = env_logger::builder()
    .is_test(true)
    .filter_level(log::LevelFilter::Trace)
    .try_init();
}

#[tokio::test]
async fn plain_text() {
  init_logging();

  let (signal_tx, mut signal_rx) = mpsc::channel(1);

  let mut event_listener = ClipboardEventListener::builder().spawn().unwrap();

  let mut stream = event_listener.new_stream(1);

  let test_string = "they're taking the hobbits to Isengard!";

  let listener_task = tokio::spawn(async move {
    while let Some(result) = stream.next().await {
      if let Ok(content) = result
        && let Body::PlainText(text) = content.as_ref()
      {
        assert_eq!(text, test_string);

        signal_tx.send(()).await.unwrap();
      }
    }
  });

  tokio::time::sleep(Duration::from_millis(100)).await;

  if cfg!(windows) {
    Command::new("powershell")
      .arg("-Command")
      .arg(format!(
        "Set-Clipboard -Value '{}'",
        // Escape single quote
        test_string.replace("'", "''")
      ))
      .status()
      .expect("Failed to execute PowerShell command.");
  } else if cfg!(target_os = "macos") {
    let mut child = Command::new("pbcopy")
      .stdin(Stdio::piped())
      .spawn()
      .expect("Failed to spawn pbcopy. This should be available on all macOS systems.");

    let mut stdin = child.stdin.take().expect("Failed to open pbcopy stdin");

    stdin
      .write_all(test_string.as_bytes())
      .expect("Failed to write to pbcopy stdin");

    drop(stdin);

    let status = child.wait().expect("pbcopy command failed to run");
    assert!(status.success(), "pbcopy command exited with an error");
  } else if cfg!(target_os = "linux") {
    let mut child = Command::new("xclip")
      .arg("-selection")
      .arg("clipboard")
      .stdin(Stdio::piped())
      .spawn()
      .expect("Failed to spawn xclip. Is it installed?");

    let mut stdin = child.stdin.take().unwrap();
    stdin.write_all(test_string.as_bytes()).unwrap();
    drop(stdin);

    let status = child.wait().unwrap();
    assert!(status.success());
  }

  match tokio::time::timeout(Duration::from_secs(3), signal_rx.recv()).await {
    Ok(Some(_)) => {}
    Ok(None) => {
      panic!("Listening task finished without receiving the correct clipboard content.");
    }
    Err(_) => {
      panic!("Test timed out: Did not receive clipboard update in time.");
    }
  }

  // Clean up the spawned task.
  listener_task.abort();
}

#[tokio::test]
async fn file_list() {
  init_logging();

  let temp_file = tempfile::NamedTempFile::new().unwrap();
  let file_path = temp_file
    .path()
    .to_path_buf()
    .canonicalize()
    .expect("Failed to canonicalize path");
  let file_uri = format!("file://{}", file_path.display());

  log::debug!("Created temporary file `{}`", file_path.display());

  let (signal_tx, mut signal_rx) = mpsc::channel(1);

  let mut event_listener = ClipboardEventListener::builder().spawn().unwrap();

  let mut stream = event_listener.new_stream(1);

  let file_path_clone = file_path.clone();
  let listener_task = tokio::spawn(async move {
    while let Some(result) = stream.next().await {
      if let Ok(content) = result
        && let Body::FileList(files) = content.as_ref()
      {
        assert_eq!(files.len(), 1);
        assert_eq!(files[0], file_path_clone);

        signal_tx.send(()).await.unwrap();
      }
    }
  });

  // Give ample time for the file to be created
  tokio::time::sleep(Duration::from_secs(1)).await;

  if cfg!(windows) {
    Command::new("powershell")
      .arg("-Command")
      .arg(format!("Set-Clipboard -Path '{}'", file_path.display()))
      .status()
      .expect("Failed to execute PowerShell command.");
  } else if cfg!(target_os = "macos") {
    let mut clipboard = Clipboard::new().expect("Failed to access the clipboard");

    clipboard
      .set()
      .file_list(&[file_path])
      .expect("Failed to set file list");
  } else if cfg!(target_os = "linux") {
    let mut child = Command::new("xclip")
      .arg("-selection")
      .arg("clipboard")
      .arg("-target")
      .arg("text/uri-list")
      .stdin(Stdio::piped())
      .spawn()
      .expect("Failed to spawn xclip. Is it installed?");

    let mut stdin = child.stdin.take().unwrap();
    stdin.write_all(file_uri.as_bytes()).unwrap();
    drop(stdin);

    let status = child.wait().unwrap();
    assert!(status.success());
  }

  match tokio::time::timeout(Duration::from_secs(3), signal_rx.recv()).await {
    Ok(Some(_)) => {}
    Ok(None) => {
      panic!("Listening task finished without receiving the correct clipboard content.");
    }
    Err(_) => {
      panic!("Test timed out: Did not receive clipboard update in time.");
    }
  }

  // Clean up the spawned task.
  listener_task.abort();
}

#[tokio::test]
async fn html() {
  init_logging();

  let (signal_tx, mut signal_rx) = mpsc::channel(1);

  let mut event_listener = ClipboardEventListener::builder().spawn().unwrap();

  let mut stream = event_listener.new_stream(1);

  let test_html = "<h1>they're taking the hobbits to Isengard!</h1>";

  let listener_task = tokio::spawn(async move {
    while let Some(result) = stream.next().await {
      if let Ok(content) = result
        && let Body::Html(html) = content.as_ref()
      {
        assert_eq!(html, test_html);

        signal_tx.send(()).await.unwrap();
      }
    }
  });

  tokio::time::sleep(Duration::from_millis(100)).await;

  #[cfg(windows)]
  {
    use clipboard_win::options::DoClear;

    let _clipboard =
      clipboard_win::Clipboard::new_attempts(10).expect("Failed to get the windows clipboard");

    let html =
      clipboard_win::formats::Html::new().expect("Failed to get html format identifier in windows");

    clipboard_win::raw::set_html_with(html.code(), test_html, DoClear)
      .expect("Failed to write html");

    drop(_clipboard);
  }

  #[cfg(target_os = "macos")]
  {
    let hex_encoded_html = hex::encode(test_html.as_bytes());

    let script = format!(
      "set the clipboard to {{«class HTML»:«data HTML{}»}}",
      hex_encoded_html
    );

    let status = Command::new("osascript")
      .arg("-e")
      .arg(&script)
      .status()
      .expect("Failed to execute osascript for HTML.");

    assert!(status.success(), "osascript for HTML failed.");
  }

  #[cfg(target_os = "linux")]
  {
    let mut child = Command::new("xclip")
      .arg("-selection")
      .arg("clipboard")
      .arg("-target")
      .arg("text/html")
      .stdin(Stdio::piped())
      .spawn()
      .expect("Failed to spawn xclip. Is it installed?");

    let mut stdin = child.stdin.take().unwrap();
    stdin.write_all(test_html.as_bytes()).unwrap();
    drop(stdin);

    let status = child.wait().unwrap();
    assert!(status.success());
  }

  match tokio::time::timeout(Duration::from_secs(3), signal_rx.recv()).await {
    Ok(Some(_)) => {}
    Ok(None) => {
      panic!("Listening task finished without receiving the correct clipboard content.");
    }
    Err(_) => {
      panic!("Test timed out: Did not receive clipboard update in time.");
    }
  }

  // Clean up the spawned task.
  listener_task.abort();
}

#[tokio::test]
async fn png() {
  init_logging();

  let img = RgbImage::new(1, 1);
  let mut png_bytes = Vec::new();
  img
    .write_to(&mut Cursor::new(&mut png_bytes), ImageFormat::Png)
    .expect("Failed to encode dummy PNG");

  let (signal_tx, mut signal_rx) = mpsc::channel(1);

  let mut event_listener = ClipboardEventListener::builder().spawn().unwrap();

  let mut stream = event_listener.new_stream(1);

  let png_clone = png_bytes.clone();
  let listener_task = tokio::spawn(async move {
    while let Some(result) = stream.next().await {
      if let Ok(content) = result
        && let Body::PngImage { bytes, .. } = content.as_ref()
      {
        assert_eq!(&png_clone, bytes);

        signal_tx.send(()).await.unwrap();
      }
    }
  });

  tokio::time::sleep(Duration::from_millis(100)).await;

  #[cfg(windows)]
  {
    let _clipboard =
      clipboard_win::Clipboard::new_attempts(10).expect("Failed to access clipboard");

    let png_format = clipboard_win::register_format("PNG").expect("Failed to register PNG format");

    clipboard_win::set(clipboard_win::formats::RawData(png_format.get()), png_bytes)
      .expect("Failed to write PNG to the clipboard");

    drop(_clipboard);
  }

  #[cfg(target_os = "macos")]
  {
    let hex_encoded_png = hex::encode(&png_bytes);

    // Construct the AppleScript command. This creates a record containing
    // raw data of type 'PNGf'.
    let script = format!(
      "set the clipboard to {{«class PNGf»:«data PNGf{}»}}",
      hex_encoded_png
    );

    let status = Command::new("osascript")
      .arg("-e")
      .arg(&script)
      .status()
      .expect("Failed to execute osascript for PNG data.");

    assert!(status.success(), "osascript for PNG data failed.");
  }

  #[cfg(target_os = "linux")]
  {
    let mut child = Command::new("xclip")
      .arg("-selection")
      .arg("clipboard")
      .arg("-target")
      .arg("image/png")
      .stdin(Stdio::piped())
      .spawn()
      .expect("Failed to spawn xclip. Is it installed?");

    let mut stdin = child.stdin.take().expect("Failed to open xclip stdin");
    stdin
      .write_all(&png_bytes)
      .expect("Failed to write to xclip stdin");
    drop(stdin);

    let status = child.wait().expect("xclip command failed to run");
    assert!(status.success(), "xclip command exited with an error");
  }

  match tokio::time::timeout(Duration::from_secs(3), signal_rx.recv()).await {
    Ok(Some(_)) => {}
    Ok(None) => {
      panic!("Listening task finished without receiving the correct clipboard content.");
    }
    Err(_) => {
      panic!("Test timed out: Did not receive clipboard update in time.");
    }
  };

  // Clean up the spawned task.
  listener_task.abort();
}

#[cfg(windows)]
#[tokio::test]
async fn dib() {
  use clipboard_win::options::DoClear;
  use std::mem::size_of;
  use std::slice;
  use windows_sys::Win32::Graphics::Gdi::{BI_RGB, BITMAPFILEHEADER, BITMAPINFOHEADER};

  init_logging();

  let width: u32 = 2;
  let height: u32 = 2;
  let bpp: u16 = 32;
  let bytes_per_pixel = (bpp / 8) as usize;

  let bgra_pixel_data: Vec<u8> = vec![0, 0, 255, 255, 0, 255, 0, 255, 255, 0, 0, 255, 0, 0, 0, 255];
  let flipped_pixel_data: Vec<u8> = bgra_pixel_data
    // 1. Get each row of pixels.
    .chunks_exact(width as usize * bytes_per_pixel)
    // 2. Reverse the order of the rows.
    .rev()
    // 3. Join the reversed rows back together.
    .flatten()
    .cloned()
    .collect();

  // 1. Create the info and file headers
  let info_header = BITMAPINFOHEADER {
    biSize: size_of::<BITMAPINFOHEADER>() as u32,
    biWidth: width as i32,
    biHeight: height as i32,
    biPlanes: 1,
    biBitCount: bpp,
    biCompression: BI_RGB,
    biSizeImage: flipped_pixel_data.len() as u32,
    biXPelsPerMeter: 0,
    biYPelsPerMeter: 0,
    biClrUsed: 0,
    biClrImportant: 0,
  };

  // Create the outer file header.
  let file_header_size = size_of::<BITMAPFILEHEADER>();
  let info_header_size = size_of::<BITMAPINFOHEADER>();

  let file_header = BITMAPFILEHEADER {
    bfType: 0x4D42, // The magic number for a bitmap file: 'B' 'M'
    bfSize: (file_header_size + info_header_size + flipped_pixel_data.len()) as u32,
    bfReserved1: 0,
    bfReserved2: 0,
    bfOffBits: (file_header_size + info_header_size) as u32,
  };

  // 2. Combine in one buffer
  let mut final_buffer: Vec<u8> = Vec::new();

  // Write the file header first
  final_buffer.extend_from_slice(unsafe {
    slice::from_raw_parts(&file_header as *const _ as *const u8, file_header_size)
  });
  // Write the info header second
  final_buffer.extend_from_slice(unsafe {
    slice::from_raw_parts(&info_header as *const _ as *const u8, info_header_size)
  });
  // Write the pixel data last
  final_buffer.extend_from_slice(&flipped_pixel_data);

  let (signal_tx, mut signal_rx) = mpsc::channel(1);

  let mut event_listener = ClipboardEventListener::builder().spawn().unwrap();

  let mut stream = event_listener.new_stream(1);

  let expected_rgb_bytes: Vec<u8> = bgra_pixel_data
    .chunks_exact(4) // Get an iterator over each 4-byte BGRA pixel
    .flat_map(|bgra_pixel| {
      // For each pixel, we extract the R, G, and B channels.
      // BGRA layout is [B, G, R, A] at indices [0, 1, 2, 3].
      let r = bgra_pixel[2];
      let g = bgra_pixel[1];
      let b = bgra_pixel[0];
      // We return them in RGB order, discarding Alpha.
      [r, g, b]
    })
    .collect();

  let listener_task = tokio::spawn(async move {
    while let Some(result) = stream.next().await {
      if let Ok(content) = result
        && let Body::RawImage(RawImage {
          bytes,
          width: received_width,
          height: received_height,
          ..
        }) = content.as_ref()
      {
        assert_eq!(&expected_rgb_bytes, bytes);
        assert_eq!(width, *received_width);
        assert_eq!(height, *received_height);

        signal_tx.send(()).await.unwrap();
      }
    }
  });

  tokio::time::sleep(Duration::from_millis(100)).await;

  let _clipboard = clipboard_win::Clipboard::new_attempts(10).expect("Failed to access clipboard");

  // We must specify DoClear here because set_bitmap does not clear the clipboard
  // and causes trouble when the tests are run sequentially
  clipboard_win::raw::set_bitmap_with(&final_buffer, DoClear).expect("Failed to write dib");

  drop(_clipboard);

  match tokio::time::timeout(Duration::from_secs(3), signal_rx.recv()).await {
    Ok(Some(_)) => {}
    Ok(None) => {
      panic!("Listening task finished without receiving the correct clipboard content.");
    }
    Err(_) => {
      panic!("Test timed out: Did not receive clipboard update in time.");
    }
  }

  // Clean up the spawned task.
  listener_task.abort();
}

#[cfg(target_os = "macos")]
#[tokio::test]
async fn tiff() {
  init_logging();

  let width = 1;
  let height = 1;

  let img = RgbImage::new(width, height);

  let mut tiff_bytes = Vec::new();
  img
    .write_to(&mut Cursor::new(&mut tiff_bytes), ImageFormat::Tiff)
    .expect("Failed to encode dummy TIFF");

  let (signal_tx, mut signal_rx) = mpsc::channel(1);

  let mut event_listener = ClipboardEventListener::builder().spawn().unwrap();

  let mut stream = event_listener.new_stream(1);

  let expected_rgb_bytes = img.into_raw();
  let listener_task = tokio::spawn(async move {
    while let Some(result) = stream.next().await {
      if let Ok(content) = result
        && let Body::RawImage(RawImage {
          bytes,
          height: received_height,
          width: received_width,
          ..
        }) = content.as_ref()
      {
        assert_eq!(&expected_rgb_bytes, bytes);
        assert_eq!(height, *received_height);
        assert_eq!(width, *received_width);

        signal_tx.send(()).await.unwrap();
      }
    }
  });

  tokio::time::sleep(Duration::from_millis(100)).await;

  let hex_encoded_tiff = hex::encode(&tiff_bytes);

  let script = format!(
    "set the clipboard to {{«class TIFF»:«data TIFF{}»}}",
    hex_encoded_tiff
  );

  let status = Command::new("osascript")
    .arg("-e")
    .arg(&script)
    .status()
    .expect("Failed to execute osascript for TIFF data.");

  assert!(status.success(), "osascript for TIFF data failed.");

  match tokio::time::timeout(Duration::from_secs(3), signal_rx.recv()).await {
    Ok(Some(_)) => {}
    Ok(None) => {
      panic!("Listening task finished without receiving the correct clipboard content.");
    }
    Err(_) => {
      panic!("Test timed out: Did not receive clipboard update in time.");
    }
  }

  // Clean up the spawned task.
  listener_task.abort();
}
