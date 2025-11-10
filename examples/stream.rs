use clipboard_watcher::{Body, ClipboardEventListener};
use futures::StreamExt;
use log::LevelFilter;

#[tokio::main]
async fn main() {
  let mut event_listener = ClipboardEventListener::builder().spawn().unwrap();

  let mut stream = event_listener.new_stream(32);

  env_logger::builder()
    .filter_level(LevelFilter::max())
    .init();

  while let Some(result) = stream.next().await {
    match result {
      Ok(content) => {
        match content.as_ref() {
          Body::PlainText(v) => println!("Received string:\n{v}"),
          Body::Image(image) => {
            println!("Received image");
            if let Some(path) = &image.path {
              println!("Image Path: {path:#?}");
            }
          }
          Body::FileList(files) => println!("Received files: {files:#?}"),
          Body::Html(html) => println!("Received html: \n{html}"),
          _ => {}
        };
      }
      Err(e) => eprintln!("Got an error: {e}"),
    }
  }
}
