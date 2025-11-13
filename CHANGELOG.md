## [0.1.0] - 2025-11-13

### ⛰️  Features

- Added windows implementation
- Added logging
- Linux implementation
- Added serde support for errors
- Utility method to check if a Body is an image

### 🐛 Bug Fixes

- Fixed logging for start of monitor
- Made macos driver creation infallible
- Fixed race condition in x11 implementation
- Adjusted uri list converter to account for trailing \r
- *(linux)* Do not use max_size for text and html
- *(macos)* Handle empty file lists
- *(macos)* Use right format for tiff
- Consume image when converting it to rgb8
- *(logging)* Refine logging of sizes to account for sizes smaller than 0.01 mbs
- Make RawImage public
- Start listener without initial delay
- Separated tests in ci
- Preventing platform tests from failing fast
- Give more time to file list test
- Removed redundant extra iteration for storage of custom formats

### 🚜 Refactor

- Reworked macos implementation
- Refined logging
- Remove rich text formatting, as html seems to work fine in rtf processors anyway
- Changed clipboard listener builder so it can accept many kinds of strings
- Remove max image bytes, use global max size only
- Separate initialization error from runtime errors
- *(win)* Make windows observer initialization fallible
- Simplified error handling logic
- *(linux)* Use a default timeout
- *(logging)* Centralized logging
- *(win)* Fail initialization if png and html formats are not available
- *(win)* Skip empty strings
- *(linux)* Log fatal error when failing to monitor the clipboard
- *(linux)* Remove unwraps from x11 context creation
- Do not treat file list with single image item as an image to avoid expensive conversions
- Avoid conversion to png, use raw rgb8 instead
- *(win)* Fixed faulty format handling
- Use warn for non fatal errors
- Handle pngs separately to retain encoded bytes
- Slightly refine image path extraction verbosity
- Remove unused error variant

### 📚 Documentation

- Added documentation
- Mention linux support
- Expand on the purpose of the max_size parameter
- Add new keywords
- Updated image handling documentation
- Updated example
- Clarify priority for pngs
- Added more internal and external documentation
- Using readme for rustdoc

### 🚀 Performance

- Skipping other formats if a format is found but is not of the valid size
- *(macos)* Write string to buffer after checking size
- *(macos)* Optimize macos implementation

### 🧪 Testing

- Added env logger in example
- Remove older tests
- Add tests
- Added size limits test
- Added test for custom formats

### 📦 CI/CD

- Add automated tests

### ⚙️ Miscellaneous Tasks

- Reorder dependencies in cargo.toml
