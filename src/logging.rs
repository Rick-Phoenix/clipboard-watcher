use std::fmt;

pub(crate) struct HumanBytes(pub usize);

impl fmt::Display for HumanBytes {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    const KIB: usize = 1024;
    const MIB: usize = 1024 * KIB;
    const GIB: usize = 1024 * MIB;

    let bytes = self.0;

    if bytes < KIB {
      // If less than a kilobyte, show as bytes.
      write!(f, "{bytes} B")
    } else if bytes < MIB {
      // If less than a megabyte, show as kilobytes with one decimal.
      write!(f, "{:.1} KiB", bytes as f64 / KIB as f64)
    } else if bytes < GIB {
      // If less than a gigabyte, show as megabytes with two decimals.
      write!(f, "{:.2} MiB", bytes as f64 / MIB as f64)
    } else {
      // Otherwise, show as gigabytes with two decimals.
      write!(f, "{:.2} GiB", bytes as f64 / GIB as f64)
    }
  }
}
