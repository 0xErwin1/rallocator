use std::{alloc, mem, ptr};
use libc::{c_void, intptr_t, sbrk};

use crate::{align, block::Block};

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
      let free_block = self.find_free_block(layout.size());

      if !free_block.is_null() {
        (*free_block).is_free = false;

        return (free_block as *mut u8).add(mem::size_of::<Block>());
      }

      let total_size: usize = mem::size_of::<Block>() + layout.size();
      let size: usize = align!(total_size);

      let address = sbrk(size as intptr_t);

      if address == usize::MAX as *mut c_void {
        return ptr::null_mut();
      }

      let block: *mut Block = address as *mut Block;
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

      (address as *mut u8).add(mem::size_of::<Block>())
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

      let decrement = 0 - align!((*block).size + mem::size_of::<Block>() + mem::size_of::<Block>());
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

  #[test]
  fn test_alloc() {
    let mut allocator = BumpAllocator::new();

    unsafe {
      let first_addr = allocator.allocate(alloc::Layout::new::<u64>()) as *mut u64;

      *first_addr = 3u64;

      assert_eq!(*first_addr, 3);

      let size: usize = 6;

      let second_addr = allocator.allocate(alloc::Layout::array::<u16>(size).unwrap()) as *mut u16;

      for i in 0..size {
        *(second_addr.add(i)) = (i + 1) as u16;
      }

      assert_eq!(*first_addr, 3);

      for i in 0..size {
        assert_eq!((i + 1) as u16, *(second_addr.add(i)))
      }

      allocator.deallocate(first_addr as *mut u8);

      let third_addr = allocator.allocate(alloc::Layout::new::<u32>()) as *mut u32;

      assert_eq!(first_addr as *mut u32, third_addr);

      allocator.deallocate(third_addr as *mut u8);

      let fourth_addr = allocator.allocate(alloc::Layout::new::<u128>()) as *mut u128;

      *fourth_addr = 25;

      assert!(fourth_addr > third_addr as *mut u128);

      assert_eq!(*fourth_addr, 25);
    }
  }
}
