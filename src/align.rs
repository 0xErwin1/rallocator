/// Calculates the machine word alignment for the given size.
///
/// # Examples
///
/// ```rust
/// use std::mem;
/// use rallocator::align;
///
/// match mem::size_of::<usize>() {
///     8 => assert_eq!(align!(13), 16), // 64 bit machine.
///     4 => assert_eq!(align!(11), 12), // 32 bit machine.
///     _ => {},
/// };
/// ```
#[macro_export]
macro_rules! align {
  ($value:expr) => {{
    // Align to machine word size
    let word = std::mem::size_of::<usize>();
    ($value + word - 1) & !(word - 1)
  }};
}

#[macro_export]
macro_rules! align_to {
  ($value:expr, $align:expr) => {{ ($value + $align - 1) & !($align - 1) }};
}

#[cfg(test)]
mod tests {
  use std::mem;

  #[test]
  fn test_align_to_word_size() {
    let word = mem::size_of::<usize>();

    for i in 1..=word {
      assert_eq!(align_to!(i, word), word);
    }

    for k in 1..10 {
      let start = word * k + 1;
      let end = word * (k + 1);

      for size in start..=end {
        assert_eq!(
          align_to!(size, word),
          word * (k + 1),
          "size={} should align to {}",
          size,
          word * (k + 1)
        );
      }
    }
  }

  #[test]
  fn test_align_word_size() {
    let word = mem::size_of::<usize>();

    for i in 1..=word {
      assert_eq!(align!(i), word);
    }

    for k in 1..10 {
      let start = word * k + 1;
      let end = word * (k + 1);

      for size in start..=end {
        assert_eq!(align!(size), word * (k + 1), "size={} should align to {}", size, word * (k + 1));
      }
    }
  }

  #[test]
  fn test_align_exact_multiples() {
    let word = mem::size_of::<usize>();

    for k in 1..20 {
      let val = word * k;
      assert_eq!(align!(val), val, "Exact multiples must remain unchanged");
    }
  }
}
