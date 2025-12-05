use std::{alloc::Layout, io::Read, ptr};

use libc::sbrk;
use rallocator::{BumpAllocator, print_alloc};

/// Waits until the user presses ENTER.
/// Useful when you want to inspect memory state with tools like `pmap`, `htop`,
/// `gdb`, or just visually track how allocations change the program break.
fn block_until_enter_pressed() {
  println!("\n>>> Press ENTER to continue...");
  let _ = std::io::stdin().bytes().next();
}

/// Prints the current program break using `sbrk(0)`.
/// The program break is the upper boundary of the heap managed via brk/sbrk.
unsafe fn print_program_break(label: &str) {
  println!(
    "[{}] PID = {}, program break (sbrk(0)) = {:?}",
    label,
    std::process::id(),
    unsafe { sbrk(0) },
  );
}

fn main() {
  // Our bump allocator. Typically it holds:
  // - a `start` pointer
  // - a `current` pointer
  // - an `end` pointer
  // and bumps the `current` pointer forward on each allocation.
  let mut allocator = BumpAllocator::new();

  unsafe {
    // Initial heap state
    print_program_break("start");
    block_until_enter_pressed();

    // --------------------------------------------------------------------
    // 1) Allocate space for a u32 (4 bytes, usually 4-byte aligned).
    // --------------------------------------------------------------------
    let layout_u32 = Layout::new::<u32>();
    let first_block = allocator.allocate(layout_u32);
    println!("\n[1] Allocate u32");
    print_alloc(layout_u32, first_block);

    // Write something into the allocated memory to show it's usable.
    let first_ptr = first_block as *mut u32;
    first_ptr.write(0xDEADBEEF);
    println!("[1] Value written to first_block = 0x{:X}", first_ptr.read());

    block_until_enter_pressed();

    // --------------------------------------------------------------------
    // 2) Allocate 12 bytes (u8[12]).
    //    This shows how the allocator handles "odd-sized" allocations.
    // --------------------------------------------------------------------
    let layout_12_bytes = Layout::array::<u8>(12).unwrap();
    let second_block = allocator.allocate(layout_12_bytes);
    println!("\n[2] Allocate [u8; 12]");
    print_alloc(layout_12_bytes, second_block);

    // Initialize the block with a byte pattern.
    let second_ptr = second_block as *mut u8;
    ptr::write_bytes(second_ptr, 0xAB, layout_12_bytes.size());
    println!("[2] Initialized second block with 0xAB");

    block_until_enter_pressed();

    // --------------------------------------------------------------------
    // 3) Allocate a u64 to test alignment (typically needs 8-byte alignment).
    // --------------------------------------------------------------------
    let layout_u64 = Layout::new::<u64>();
    let third_block = allocator.allocate(layout_u64);
    println!("\n[3] Allocate u64 (observe alignment)");
    print_alloc(layout_u64, third_block);

    let third_ptr = third_block as *mut u64;
    third_ptr.write(0x1122334455667788);
    println!("[3] Value written = 0x{:X}", third_ptr.read());

    // Manual alignment check
    let addr_third = third_block as usize;
    println!(
      "[3] Address = {:#X}, addr % align = {}",
      addr_third,
      addr_third % layout_u64.align()
    );

    block_until_enter_pressed();

    // --------------------------------------------------------------------
    // 4) Allocate an array of u16 to force more pointer movement.
    // --------------------------------------------------------------------
    let layout_u16_array = Layout::array::<u16>(16).unwrap(); // 32 bytes
    let fourth_block = allocator.allocate(layout_u16_array);
    println!("\n[4] Allocate [u16; 16]");
    print_alloc(layout_u16_array, fourth_block);

    let fourth_ptr = fourth_block as *mut u16;
    for i in 0..16 {
      fourth_ptr.add(i).write(i as u16);
    }
    println!("[4] Wrote 0..15 into the u16 array");

    block_until_enter_pressed();

    // --------------------------------------------------------------------
    // 5) Deallocate the first block.
    //
    //    What happens now depends on your allocator:
    //    - If it has a free-list, the block might be reused later.
    //    - If it's a pure bump allocator (monotonic), deallocate may be a no-op.
    // --------------------------------------------------------------------
    allocator.deallocate(first_block);
    println!("\n[5] Deallocated first_block at {:?}", first_block);
    block_until_enter_pressed();

    // --------------------------------------------------------------------
    // 6) Allocate a small block (2 bytes) to see if the allocator
    //    reuses the freed block.
    // --------------------------------------------------------------------
    let layout_2_bytes = Layout::array::<u8>(2).unwrap();
    let fifth_block = allocator.allocate(layout_2_bytes);
    println!("\n[6] Allocate [u8; 2] (check reuse of freed block)");
    print_alloc(layout_2_bytes, fifth_block);

    println!(
      "[6] fifth_block == first_block? {}",
      if fifth_block == first_block {
        "Yes, it reused the freed block"
      } else {
        "No, it allocated somewhere else"
      }
    );

    block_until_enter_pressed();

    // --------------------------------------------------------------------
    // 7) Allocate a large block to observe heap growth.
    //    This usually changes the result of `sbrk(0)`.
    // --------------------------------------------------------------------
    print_program_break("before large alloc");

    // Example: 64 KiB
    let layout_big = Layout::array::<u8>(64 * 1024).unwrap();
    let big_block = allocator.allocate(layout_big);
    println!("\n[7] Allocate large 64 KiB block");
    print_alloc(layout_big, big_block);

    print_program_break("after large alloc");
    block_until_enter_pressed();

    // --------------------------------------------------------------------
    // 8) End of demo.
    //
    //    A bump allocator typically doesn’t free memory individually.
    //    The OS reclaims all memory when the process exits.
    // --------------------------------------------------------------------
    println!("\n[8] End of example. Process will exit and the OS will reclaim all memory.");
  }
}
