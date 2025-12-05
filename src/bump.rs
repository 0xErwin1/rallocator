use std::{alloc, mem, ptr};
use libc::{c_void, intptr_t, sbrk};

use crate::{align, align_to, block::Block};

pub unsafe fn print_alloc(
  layout: alloc::Layout,
  addr: *mut u8,
) {
  println!(
    "Allocated {} bytes, address = {:?}, program break = {:?}",
    layout.size(),
    addr,
    unsafe { sbrk(0) }
  );
}

pub struct BumpAllocator {
  first: *mut Block,
  last: *mut Block,
}

impl BumpAllocator {
  pub fn new() -> Self {
    Self {
      first: ptr::null_mut(),
      last: ptr::null_mut(),
    }
  }

  unsafe fn find_free_block(
    &self,
    size: usize,
  ) -> *mut Block {
    unsafe {
      let mut current: *mut Block = self.first;

      while !current.is_null() {
        if (*current).is_free && (*current).size >= size {
          return current;
        }
        current = (*current).next;
      }

      ptr::null_mut()
    }
  }

  pub unsafe fn allocate(
    &mut self,
    layout: alloc::Layout,
  ) -> *mut u8 {
    unsafe {
      let align = layout.align();
      let header_size = mem::size_of::<Block>();
      let size_for_sbrk = align!(header_size + layout.size() + (align - 1));

      let raw_address = sbrk(size_for_sbrk as intptr_t);
      if raw_address == usize::MAX as *mut c_void {
        return ptr::null_mut();
      }

      let content_addr = align_to!((raw_address as usize) + header_size, align);

      let block = (content_addr - header_size) as *mut Block;
      (*block).is_free = false;
      (*block).size = layout.size();
      (*block).next = ptr::null_mut();

      if self.first.is_null() {
        self.first = block;
        self.last = block;
      } else {
        (*self.last).next = block;
        self.last = block;
      }

      content_addr as *mut u8
    }
  }

  pub unsafe fn deallocate(
    &mut self,
    address: *mut u8,
  ) {
    unsafe {
      if address.is_null() {
        return;
      }

      let block = self.find_block(address);
      (*block).is_free = true;

      if block != self.last {
        return;
      }

      if self.first == self.last {
        self.first = ptr::null_mut();
        self.last = ptr::null_mut();
      } else {
        let mut current: *mut Block = self.first;
        while !(*current).next.is_null() && (*current).next != self.last {
          current = (*current).next;
        }
        self.last = current;
      }

      let to_release: usize = align!((*block).size + mem::size_of::<Block>() + mem::size_of::<Block>());

      let decrement: isize = -(to_release as isize);

      sbrk(decrement as intptr_t);
    }
  }

  unsafe fn find_block(
    &self,
    address: *mut u8,
  ) -> *mut Block {
    let block = unsafe { address.sub(mem::size_of::<Block>()) } as *mut Block;
    block
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use std::alloc::Layout;
  use libc::sbrk;

  /// Helper: check that a pointer is aligned to `align` bytes.
  fn is_aligned(
    ptr: *mut u8,
    align: usize,
  ) -> bool {
    (ptr as usize) % align == 0
  }

  #[test]
  fn basic_allocation_and_write_read() {
    let mut allocator = BumpAllocator::new();

    unsafe {
      // Allocate a u64 and write to it
      let layout_u64 = Layout::new::<u64>();
      let ptr_u64 = allocator.allocate(layout_u64) as *mut u64;
      assert!(!ptr_u64.is_null());

      *ptr_u64 = 0xDEADBEEFDEADBEEF;
      assert_eq!(*ptr_u64, 0xDEADBEEFDEADBEEF);

      // Allocate an array of u16 and write a small pattern
      let count = 8usize;
      let layout_u16 = Layout::array::<u16>(count).unwrap();
      let ptr_u16 = allocator.allocate(layout_u16) as *mut u16;
      assert!(!ptr_u16.is_null());

      for i in 0..count {
        ptr_u16.add(i).write((i as u16) + 1);
      }

      // Check that the original u64 wasn't corrupted
      assert_eq!(*ptr_u64, 0xDEADBEEFDEADBEEF);

      for i in 0..count {
        assert_eq!((i as u16) + 1, ptr_u16.add(i).read());
      }
    }
  }

  #[test]
  fn allocations_respect_layout_alignment() {
    let mut allocator = BumpAllocator::new();

    unsafe {
      let layouts = [
        Layout::new::<u8>(),
        Layout::new::<u16>(),
        Layout::new::<u32>(),
        Layout::new::<u64>(),
        Layout::new::<u128>(),
        Layout::array::<u64>(4).unwrap(),
      ];

      for layout in layouts {
        let ptr = allocator.allocate(layout);
        assert!(!ptr.is_null());

        assert!(
          is_aligned(ptr, layout.align()),
          "allocation must be {}-byte aligned, got {:p}",
          layout.align(),
          ptr
        );
      }
    }
  }

  #[test]
  fn multiple_allocations_are_monotonic_and_distinct() {
    let mut allocator = BumpAllocator::new();

    unsafe {
      let layouts = [
        Layout::array::<u8>(8).unwrap(),
        Layout::array::<u16>(16).unwrap(),
        Layout::array::<u64>(4).unwrap(),
        Layout::array::<u128>(2).unwrap(),
      ];

      let mut addrs = Vec::new();

      for layout in layouts {
        let ptr = allocator.allocate(layout);
        assert!(!ptr.is_null());
        addrs.push(ptr as usize);
      }

      // Each subsequent allocation should be at or after the previous one.
      // We don't require contiguity, just monotonic non-decreasing addresses.
      for w in addrs.windows(2) {
        assert!(
          w[1] >= w[0],
          "addresses should be monotonic, got {:p} then {:p}",
          w[0] as *mut u8,
          w[1] as *mut u8
        );
      }
    }
  }

  #[test]
  fn deallocate_null_is_noop_and_deallocate_last_block_does_not_crash() {
    let mut allocator = BumpAllocator::new();

    unsafe {
      // deallocating null should be a no-op
      allocator.deallocate(std::ptr::null_mut());

      // Keep track of break before
      let brk_before = sbrk(0);

      // Single allocation
      let layout = Layout::new::<u64>();
      let ptr_u64 = allocator.allocate(layout) as *mut u64;
      assert!(!ptr_u64.is_null());

      *ptr_u64 = 123;
      assert_eq!(*ptr_u64, 123);

      // Deallocate that block (it should be the last block)
      allocator.deallocate(ptr_u64 as *mut u8);

      // Just ensure this does not crash and the program break
      // did not go *up* as a result of deallocation.
      let brk_after = sbrk(0);

      // Some libc implementations may or may not shrink the break exactly,
      // so we only assert it doesn't increase.
      assert!(
        (brk_after as isize) <= (brk_before as isize),
        "program break should not increase after deallocation"
      );
    }
  }

  #[test]
  fn large_block_allocation_and_integrity() {
    let mut allocator = BumpAllocator::new();

    unsafe {
      let count = 4096usize;
      let layout = Layout::array::<u32>(count).unwrap();
      let ptr = allocator.allocate(layout) as *mut u32;
      assert!(!ptr.is_null());

      for i in 0..count {
        ptr.add(i).write((i as u32) ^ 0xA5A5_A5A5);
      }

      for i in 0..count {
        let val = ptr.add(i).read();
        assert_eq!(val, (i as u32) ^ 0xA5A5_A5A5);
      }
    }
  }
}
