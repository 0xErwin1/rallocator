/// Calculates the machine word alignment for the given size.
///
/// # Examples
///
/// ```rust
/// use std::mem;
/// use allocator::align;
///
/// match mem::size_of::<usize>() {
///     8 => assert_eq!(align!(13), 16), // 64 bit machine.
///     4 => assert_eq!(align!(11), 12), // 32 bit machine.
///     _ => {},
/// };
/// ```
#[macro_export]
macro_rules! align {
  ($value:expr) => {
    ($value + mem::size_of::<usize>() - 1) & !(mem::size_of::<usize>() - 1)
  };
}

#[cfg(test)]
mod tests {
  use std::mem;

  #[test]
  fn test_align() {
    let ptr_size = mem::size_of::<usize>();

    let mut alignments = Vec::new();

    for i in 0..10 {
      let sizes = (ptr_size * i + 1)..=(ptr_size * (i + 1));

      let expected_alignment = ptr_size * (i + 1);

      alignments.push((sizes, expected_alignment));
    }

    for (sizes, expected) in alignments {
      for size in sizes {
        assert_eq!(expected, align!(size));
      }
    }
  }
}
