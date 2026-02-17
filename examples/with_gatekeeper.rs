use clipboard_watcher::{Body, ClipboardEventListener};
use futures::StreamExt;
use log::Level;

#[tokio::main]
async fn main() {
  let mut event_listener = ClipboardEventListener::builder()
    .with_gatekeeper(|ctx| {
      if let Some(can_include_flag) = ctx.get_u32("CanIncludeInClipboardHistory")
        && can_include_flag == 0
      {
        eprintln!("Detected `CanIncludeInClipboardHistory` being set to 0. Skipped processing");
        return false;
      }

      if ctx.has_format("ExcludeClipboardContentFromMonitorProcessing") {
        eprintln!("Detected `ExcludeClipboardContentFromMonitorProcessing`. Skipped processing");
        return false;
      }

      true
    })
    .spawn()
    .unwrap();

  let mut stream = event_listener.new_stream(5);

  env_logger::init();

  while let Some(result) = stream.next().await {
    // Can enable logging with RUST_LOG
    if !log::log_enabled!(Level::Debug) {
      match result {
        Ok(content) => {
          match content.as_ref() {
            Body::PlainText(v) => println!("Received string:\n{v}"),
            Body::RawImage(image) => {
              println!("Received raw image");
              if let Some(path) = &image.path {
                println!("Image Path: {}", path.display());
              }
            }
            Body::PngImage {
              path,
              bytes: _bytes,
            } => {
              println!("Received png image");
              if let Some(path) = &path {
                println!("Image Path: {}", path.display());
              }
            }
            Body::FileList(files) => println!("Received files: {files:#?}"),
            Body::Html(html) => println!("Received html: \n{html}"),
            Body::Custom { .. } => {}
          };
        }
        Err(e) => eprintln!("Got an error: {e}"),
      }
    }
  }
}
