# clipboard-watcher

This crate can be used to subscribe to the system clipboard and read its contents whenever a new item is added to it. 

## Features

- **Leverages the Stream async primitive**
    The listener implements `Stream`, which unlocks access to all implementations that have been built around this trait, such as throttling, debouncing and so on.

- **Max size filter**
    The user can define a maximum allowed size for a clipboard item. This can be useful to avoid processing very large images or custom formats.
    The logic for checking an item's size vary from platform to platform.
    On windows, the size can always be checked without processing the data immediately. On linux, this is also possible in the majority of cases (as long as the clipboard owner supports requests for the `LENGTH` property).
    On macos, there isn't a way of doing this in a cheap way (as far as I know), so the data will be loaded first and then its size will be inspected.

- **Custom formats**
    The listener supports any arbitrary custom format.

- **Customizable polling interval**

# Supported Formats

- HTML
- Text
- File list
- Png Images
- Other Images (normalized to raw rgb8)
- Custom formats

# Example

You can run this example with cargo: `cargo run --example stream`

```rust
use clipboard_watcher::{Body, ClipboardEventListener};
use futures::StreamExt;
use log::Level;

#[tokio::main]
async fn main() {
  let mut event_listener = ClipboardEventListener::builder().spawn().unwrap();

  // Specifies the buffer size
  let mut stream = event_listener.new_stream(32);

  env_logger::init();

  while let Some(result) = stream.next().await {
    // You can enable logging with RUST_LOG for more detailed inspection.
    // Otherwise, the activity will be logged as follows
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
```

# Platforms

- Windows
- Macos
- Linux (requires x11/xWayland)

## Credits And Licenses

Licensed under the Apache-2.0 license.

Initial concept for stream-based architecture is taken from [clipboard-stream](https://github.com/nakaryo716/clipboard-stream), licensed under MIT.

Various bits of code are also taken from [clipboard-rs](https://github.com/ChurchTao/clipboard-rs) (MIT) and [arboard](https://github.com/1Password/arboard) (MIT/Apache-2.0)
