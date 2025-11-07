# clipboard-watcher

This crate can be used to subscribe to the system clipboard and read its contents whenever a new item is added to it. 

It leverages the `Stream` async primitive, which unlocks common useful implementations for streams such as debouncing.

It allows for customization of the listener's parameters, such as:

- Custom formats
- Polling interval
- Maximum size (items beyond this size are not processed)
- Maximum image size

# Supported Formats

- HTML
- Text
- File list
- Custom formats
- Images (normalized to PNG)

# Example

```rust
use clipboard_stream::{Body, ClipboardEventListener};
use futures::StreamExt;
use log::LevelFilter;

#[tokio::main]
async fn main() {
  let mut event_listener = ClipboardEventListener::builder().spawn().unwrap();

  let mut stream = event_listener.new_stream(32);

  env_logger::builder().filter_level(LevelFilter::max()).init();

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
      Err(e) => eprintln!("{e}"),
    }
  }
}
```

# Platforms

- Windows
- Macos

## Credits And Licenses

Licensed under the Apache-2.0 license.

Initial concept for stream-based architecture is taken from [clipboard-stream](https://github.com/nakaryo716/clipboard-stream), licensed under MIT.

Various bits of code are also taken from [clipboard-rs](https://github.com/ChurchTao/clipboard-rs) (MIT) and [arboard](https://github.com/1Password/arboard) (MIT/Apache-2.0)
