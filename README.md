# clipboard-watcher

This crate can be used to subscribe to the system clipboard and read its contents whenever a new item is added to it. 

It leverages the `Stream` async primitive, which unlocks common useful implementations for streams such as debouncing.

It allows for customization of the listener's parameters, such as:

- Custom formats
- Polling interval
- Maximum size
    - The crate will first try to use a cheap method to get the size of the clipboard item, and if the size is beyond the limit, the item will not be processed at all, providing better performance.
        - For linux/x11, the cheap method may or may not work depending on how the clipboard owner behaves (there isn't a centralized clipboard handler like in windows or macos). If this method is not available, then a less efficient method will be used, where the data is loaded and then its size gets checked.
        - For macos, there is not a cheap method to check for an item's size (as far as I know), so the best that can be done is to load the item and just discard it if the size is not within the allowed range.
# Supported Formats

- HTML
- Text
- File list
- Png Images
- Other Images (normalized to raw rgb8)
- Custom formats

# Example

```rust
use clipboard_watcher::{Body, ClipboardEventListener};
use futures::StreamExt;
use log::Level;

#[tokio::main]
async fn main() {
  let mut event_listener = ClipboardEventListener::builder().spawn().unwrap();

  let mut stream = event_listener.new_stream(32);

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
```

# Platforms

- Windows
- Macos
- Linux (requires x11/xWayland)

## Credits And Licenses

Licensed under the Apache-2.0 license.

Initial concept for stream-based architecture is taken from [clipboard-stream](https://github.com/nakaryo716/clipboard-stream), licensed under MIT.

Various bits of code are also taken from [clipboard-rs](https://github.com/ChurchTao/clipboard-rs) (MIT) and [arboard](https://github.com/1Password/arboard) (MIT/Apache-2.0)
