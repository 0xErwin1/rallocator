//! # Bump Allocator
//!
//! A simple bump (or arena) allocator implementation that manages memory by
//! extending the program's data segment using the `sbrk` system call.
//!
//! ## Overview
//!
//! A bump allocator is one of the simplest allocation strategies. It maintains
//! a pointer that "bumps" forward with each allocation. Memory is obtained from
//! the operating system by moving the program break (the end of the data segment).
//!
//! ## How It Works
//!
//! The allocator uses a **linked list** of blocks to track allocations. Each
//! allocation creates a new block at the end of the heap.
//!
//! ### Memory Layout Diagram
//!
//! ```text
//!                          HEAP MEMORY (grows upward →)
//!
//!     Program Break (before)                    Program Break (after)
//!            │                                          │
//!            ▼                                          ▼
//!  ┌─────────┬──────────────────────────────────────────┐
//!  │ Existing│           New Allocation                 │
//!  │  Data   │                                          │
//!  └─────────┴──────────────────────────────────────────┘
//!            │                                          │
//!            └──── sbrk(size) moves break here ─────────┘
//! ```
//!
//! ### Block Structure
//!
//! Each allocation is preceded by a metadata header (`Block`):
//!
//! ```text
//!                    Single Allocation
//!     ┌──────────────────────────────────────────────┐
//!     │                                              │
//!     ▼                                              │
//!  ┌──────────────────┬─────────────────────────┐    │
//!  │   Block Header   │      User Data          │    │
//!  │   (metadata)     │      (payload)          │    │
//!  ├──────────────────┼─────────────────────────┤    │
//!  │ size: usize      │                         │    │
//!  │ is_free: bool    │   [    N bytes    ]     │    │
//!  │ next: *mut Block │                         │    │
//!  └──────────────────┴─────────────────────────┘    │
//!     │                  ▲                           │
//!     │                  │                           │
//!     │                  └── Pointer returned to ────┘
//!                            the user (aligned)
//! ```
//!
//! ### Linked List of Blocks
//!
//! Multiple allocations form a singly-linked list:
//!
//! ```text
//!   BumpAllocator
//!   ┌─────────┐
//!   │ first ──┼──┐
//!   │ last ───┼──┼──────────────────────────────────────────┐
//!   └─────────┘  │                                          │
//!                ▼                                          ▼
//!   ┌────────────────────┐    ┌────────────────────┐    ┌────────────────────┐
//!   │  Block 1           │    │  Block 2           │    │  Block 3           │
//!   ├────────────────────┤    ├────────────────────┤    ├────────────────────┤
//!   │ size: 64           │    │ size: 128          │    │ size: 32           │
//!   │ is_free: false     │    │ is_free: true      │    │ is_free: false     │
//!   │ next: ─────────────┼───►│ next: ─────────────┼───►│ next: null         │
//!   ├────────────────────┤    ├────────────────────┤    ├────────────────────┤
//!   │    [User Data]     │    │    [User Data]     │    │    [User Data]     │
//!   │    (64 bytes)      │    │    (128 bytes)     │    │    (32 bytes)      │
//!   └────────────────────┘    └────────────────────┘    └────────────────────┘
//!
//!   ◄─────────────────── Heap grows this direction ─────────────────────────►
//! ```
//!
//! ### Alignment Handling
//!
//! When allocating, the allocator ensures proper alignment for the user data:
//!
//! ```text
//!   sbrk returns
//!   raw address
//!        │
//!        ▼
//!   ┌────┬───────────────────┬───────────────────────────────────────┐
//!   │pad │   Block Header    │           User Data                   │
//!   │    │   (24 bytes on    │           (aligned to                 │
//!   │    │    64-bit)        │            requested alignment)       │
//!   └────┴───────────────────┴───────────────────────────────────────┘
//!        │                   │
//!        │                   └── content_addr (aligned)
//!        │
//!        └── Block header placed just before content_addr
//!
//!   The formula:
//!   1. Request: header_size + user_size + (align - 1)   [for alignment slack]
//!   2. Calculate: content_addr = align_to(raw_addr + header_size, align)
//!   3. Place header at: content_addr - header_size
//! ```
//!
//! ### Allocation Process (Step by Step)
//!
//! ```text
//!   STEP 1: Calculate total size needed
//!   ┌─────────────────────────────────────────────────────────┐
//!   │  size_for_sbrk = align(header_size + user_size + (A-1)) │
//!   │  where A = requested alignment                          │
//!   └─────────────────────────────────────────────────────────┘
//!
//!   STEP 2: Extend heap with sbrk()
//!   ┌─────────────────────────────────────────────────────────┐
//!   │  raw_address = sbrk(size_for_sbrk)                      │
//!   │                                                         │
//!   │  Before: [existing heap]|← program break                │
//!   │  After:  [existing heap][  new space  ]|← new break     │
//!   └─────────────────────────────────────────────────────────┘
//!
//!   STEP 3: Calculate aligned content address
//!   ┌─────────────────────────────────────────────────────────┐
//!   │  content_addr = align_to(raw_address + header_size, A)  │
//!   └─────────────────────────────────────────────────────────┘
//!
//!   STEP 4: Initialize block header
//!   ┌─────────────────────────────────────────────────────────┐
//!   │  block = (content_addr - header_size) as *mut Block     │
//!   │  (*block).is_free = false                               │
//!   │  (*block).size = user_size                              │
//!   │  (*block).next = null                                   │
//!   └─────────────────────────────────────────────────────────┘
//!
//!   STEP 5: Update linked list
//!   ┌─────────────────────────────────────────────────────────┐
//!   │  if first allocation:                                   │
//!   │      first = block                                      │
//!   │      last = block                                       │
//!   │  else:                                                  │
//!   │      (*last).next = block                               │
//!   │      last = block                                       │
//!   └─────────────────────────────────────────────────────────┘
//!
//!   STEP 6: Return pointer to user
//!   ┌─────────────────────────────────────────────────────────┐
//!   │  return content_addr as *mut u8                         │
//!   └─────────────────────────────────────────────────────────┘
//! ```
//!
//! ### Deallocation Process
//!
//! Deallocation marks a block as free. If the deallocated block is the **last**
//! block in the list, the heap is shrunk using a negative `sbrk` call:
//!
//! ```text
//!   BEFORE DEALLOCATION:
//!   ┌──────────┐    ┌──────────┐    ┌──────────┐
//!   │ Block 1  │───►│ Block 2  │───►│ Block 3  │◄── last
//!   │ in_use   │    │ free     │    │ in_use   │
//!   └──────────┘    └──────────┘    └──────────┘
//!                                               │
//!                                   program break
//!
//!   AFTER deallocate(block3_ptr):
//!   ┌──────────┐    ┌──────────┐
//!   │ Block 1  │───►│ Block 2  │◄── now last
//!   │ in_use   │    │ free     │
//!   └──────────┘    └──────────┘
//!                               │
//!                   program break (moved back via sbrk(-size))
//!
//!   NOTE: If a middle block is freed, it is only marked as free
//!         but NOT returned to the OS (cannot shrink the heap).
//! ```
//!
//! ## Trade-offs
//!
//! ### Advantages
//! - **Simple implementation**: Easy to understand and maintain
//! - **Fast allocation**: O(1) allocation time (just bump the pointer)
//! - **No fragmentation in allocation order**: Allocations are contiguous
//!
//! ### Disadvantages
//! - **Limited deallocation**: Can only truly free the last block
//! - **Memory waste**: Middle deallocations don't return memory to OS
//! - **No reuse of freed blocks**: The `find_free_block` method exists but
//!   `allocate` always requests new memory (potential optimization point)
//!
//! ## System Calls
//!
//! This allocator uses `sbrk(2)`:
//! - `sbrk(0)`: Returns the current program break
//! - `sbrk(n)`: Increases the program break by `n` bytes, returns old break
//! - `sbrk(-n)`: Decreases the program break by `n` bytes (frees memory)
//!
//! ```text
//!   Virtual Memory Layout
//!   ┌─────────────────────┐ High addresses
//!   │       Stack         │ ↓ grows down
//!   │         │           │
//!   │         ▼           │
//!   │                     │
//!   │         ▲           │
//!   │         │           │
//!   │       Heap          │ ↑ grows up (via sbrk)
//!   ├─────────────────────┤ ← Program break (brk)
//!   │   BSS (uninit data) │
//!   ├─────────────────────┤
//!   │   Data (init data)  │
//!   ├─────────────────────┤
//!   │       Text          │
//!   └─────────────────────┘ Low addresses
//! ```
//!
//! ## Safety
//!
//! This allocator uses **unsafe Rust** extensively because:
//! 1. Direct manipulation of raw pointers
//! 2. System calls to `sbrk`
//! 3. Manual memory management without Rust's borrow checker guarantees
//!
//! Callers must ensure:
//! - Pointers returned from `allocate` are valid until `deallocate` is called
//! - The same pointer is not deallocated twice
//! - Pointers are not used after deallocation
//!
//! ## Example
//!
//! ```rust,ignore
//! use std::alloc::Layout;
//! use rallocator::BumpAllocator;
//!
//! let mut allocator = BumpAllocator::new();
//!
//! unsafe {
//!     // Allocate space for a u64
//!     let layout = Layout::new::<u64>();
//!     let ptr = allocator.allocate(layout) as *mut u64;
//!
//!     // Write and read
//!     *ptr = 42;
//!     assert_eq!(*ptr, 42);
//!
//!     // Deallocate
//!     allocator.deallocate(ptr as *mut u8);
//! }
//! ```

use std::{alloc, mem, ptr};
use libc::{c_void, intptr_t, sbrk};

use crate::{align, align_to, block::Block};

/// Debug helper function that prints allocation information.
///
/// Outputs the allocation size, the returned address, and the current
/// program break position for debugging purposes.
///
/// # Arguments
///
/// * `layout` - The layout of the allocation (contains size and alignment info)
/// * `addr` - The pointer that was returned to the user
///
/// # Safety
///
/// This function calls `sbrk(0)` which is always safe, but the function
/// is marked unsafe to match the allocator's API conventions.
///
/// # Example Output
///
/// ```text
/// Allocated 64 bytes, address = 0x5555557a1040, program break = 0x5555557a2000
/// ```
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

/// A simple bump allocator that manages heap memory using `sbrk`.
///
/// # Memory Management Strategy
///
/// The `BumpAllocator` maintains a singly-linked list of allocation blocks.
/// Each block contains metadata (size, free status, next pointer) followed
/// by the user's data.
///
/// ```text
///   ┌───────────────────────────────────────────────────────────┐
///   │                    BumpAllocator                          │
///   │                                                           │
///   │   first ─────────►┌─────────┐                             │
///   │                   │ Block 1 │──────►┌─────────┐           │
///   │                   └─────────┘       │ Block 2 │──► null   │
///   │   last ───────────────────────────► └─────────┘           │
///   │                                                           │
///   └───────────────────────────────────────────────────────────┘
/// ```
///
/// # Fields
///
/// * `first` - Pointer to the first block in the allocation list (head)
/// * `last` - Pointer to the last block in the allocation list (tail)
///
/// Both pointers are `null` when the allocator is empty.
///
/// # Thread Safety
///
/// This allocator is **NOT** thread-safe. For multi-threaded usage,
/// external synchronization (e.g., a `Mutex`) is required.
pub struct BumpAllocator {
  /// Pointer to the first (oldest) block in the linked list.
  /// Used as the starting point when searching for free blocks.
  first: *mut Block,

  /// Pointer to the last (newest) block in the linked list.
  /// New allocations are appended here. Deallocation of this
  /// block allows heap shrinking via `sbrk(-size)`.
  last: *mut Block,
}

impl BumpAllocator {
  /// Creates a new, empty `BumpAllocator`.
  ///
  /// # Returns
  ///
  /// A new allocator instance with no blocks allocated.
  /// Both `first` and `last` pointers are initialized to null.
  ///
  /// # Example
  ///
  /// ```rust,ignore
  /// let allocator = BumpAllocator::new();
  /// // allocator.first == null
  /// // allocator.last == null
  /// ```
  ///
  /// # State Diagram
  ///
  /// ```text
  ///   After new():
  ///   ┌─────────────────┐
  ///   │  BumpAllocator  │
  ///   │                 │
  ///   │  first: null    │
  ///   │  last:  null    │
  ///   └─────────────────┘
  /// ```
  pub fn new() -> Self {
    Self {
      first: ptr::null_mut(),
      last: ptr::null_mut(),
    }
  }

  /// Searches the block list for a free block of sufficient size.
  ///
  /// This method implements a **first-fit** allocation strategy,
  /// returning the first block that is both free and large enough.
  ///
  /// # Arguments
  ///
  /// * `size` - The minimum size required for the allocation
  ///
  /// # Returns
  ///
  /// * A pointer to a suitable free block if found
  /// * `null` if no suitable block exists
  ///
  /// # Search Process
  ///
  /// ```text
  ///   Looking for size = 100
  ///
  ///   ┌────────────┐    ┌────────────┐    ┌────────────┐    ┌────────────┐
  ///   │ size: 64   │───►│ size: 128  │───►│ size: 200  │───►│ size: 50   │
  ///   │ free: no   │    │ free: yes  │    │ free: yes  │    │ free: yes  │
  ///   └────────────┘    └────────────┘    └────────────┘    └────────────┘
  ///        ↓                  ↓
  ///      skip             ✓ MATCH!
  ///   (not free)        (free && 128 >= 100)
  ///
  ///   Returns: pointer to Block 2
  /// ```
  ///
  /// # Note
  ///
  /// This method exists but is currently unused by `allocate()`, which
  /// always requests new memory from the OS. This is a potential
  /// optimization point for reusing freed blocks.
  ///
  /// # Safety
  ///
  /// The caller must ensure that the allocator's internal state is valid
  /// and that no other thread is modifying the block list concurrently.
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

  /// Allocates a block of memory with the specified layout.
  ///
  /// This is the primary allocation method. It extends the heap using `sbrk`,
  /// creates a new block with metadata, and returns an aligned pointer to
  /// the user data region.
  ///
  /// # Arguments
  ///
  /// * `layout` - The [`Layout`] describing size and alignment requirements
  ///
  /// # Returns
  ///
  /// * A properly aligned pointer to the allocated memory
  /// * `null` if allocation fails (e.g., `sbrk` returns an error)
  ///
  /// # Memory Layout Created
  ///
  /// ```text
  ///   Memory obtained from sbrk:
  ///   ┌──────────────────────────────────────────────────────────────────┐
  ///   │                                                                  │
  ///   ├────────┬────────────────────────┬───────────────────────────────┤
  ///   │ Padding│     Block Header       │         User Data             │
  ///   │ (opt.) │                        │                               │
  ///   │        │ ┌───────────────────┐  │  ┌─────────────────────────┐  │
  ///   │  ???   │ │ size: layout.size │  │  │                         │  │
  ///   │ bytes  │ │ is_free: false    │  │  │    layout.size bytes    │  │
  ///   │        │ │ next: null        │  │  │    (user accessible)    │  │
  ///   │        │ └───────────────────┘  │  └─────────────────────────┘  │
  ///   └────────┴────────────────────────┴───────────────────────────────┘
  ///            ▲                        ▲
  ///            │                        │
  ///         Block*                 Returned pointer
  ///      (internal use)            (aligned to layout.align())
  /// ```
  ///
  /// # Alignment Calculation
  ///
  /// ```text
  ///   Given: raw_address from sbrk, header_size, requested align
  ///
  ///   Step 1: Find where content would be without alignment
  ///           unaligned_content = raw_address + header_size
  ///
  ///   Step 2: Align the content address upward
  ///           content_addr = (unaligned_content + align - 1) & !(align - 1)
  ///
  ///   Step 3: Place header just before content
  ///           block_addr = content_addr - header_size
  ///
  ///   Example with 16-byte alignment:
  ///
  ///     raw_address = 0x1000
  ///     header_size = 24 bytes
  ///     align = 16
  ///
  ///     unaligned = 0x1000 + 24 = 0x1018
  ///     content_addr = align_to(0x1018, 16) = 0x1020
  ///     block_addr = 0x1020 - 24 = 0x1008
  ///
  ///     Memory:
  ///     0x1000 ┌────────┐
  ///            │ unused │ (8 bytes of padding)
  ///     0x1008 ├────────┤ ← Block header starts here
  ///            │ header │ (24 bytes)
  ///     0x1020 ├────────┤ ← Content starts here (16-byte aligned)
  ///            │  data  │
  ///            └────────┘
  /// ```
  ///
  /// # Linked List Update
  ///
  /// ```text
  ///   BEFORE (2 existing blocks):
  ///   ┌─────────────────┐
  ///   │  BumpAllocator  │
  ///   │  first ─────────┼──────►[Block A]────►[Block B]
  ///   │  last ──────────┼─────────────────────────┘
  ///   └─────────────────┘
  ///
  ///   AFTER allocate() adds Block C:
  ///   ┌─────────────────┐
  ///   │  BumpAllocator  │
  ///   │  first ─────────┼──────►[Block A]────►[Block B]────►[Block C]
  ///   │  last ──────────┼──────────────────────────────────────┘
  ///   └─────────────────┘
  /// ```
  ///
  /// # Safety
  ///
  /// This function is unsafe because:
  /// - It performs raw pointer arithmetic
  /// - It dereferences raw pointers without bounds checking
  /// - It modifies global process state via `sbrk`
  ///
  /// The caller must ensure:
  /// - The layout is valid (non-zero size, power-of-two alignment)
  /// - No concurrent modifications to the allocator
  ///
  /// # Errors
  ///
  /// Returns `null` if:
  /// - `sbrk` fails (returns `(void*)-1`), typically due to:
  ///   - Out of memory
  ///   - Resource limits (`RLIMIT_DATA`) exceeded
  pub unsafe fn allocate(
    &mut self,
    layout: alloc::Layout,
  ) -> *mut u8 {
    unsafe {
      let align = layout.align();
      let header_size = mem::size_of::<Block>();

      // Calculate total size needed:
      // - header_size: space for Block metadata
      // - layout.size(): user-requested allocation size
      // - (align - 1): worst-case padding for alignment
      // The result is word-aligned via the align! macro
      let size_for_sbrk = align!(header_size + layout.size() + (align - 1));

      // Extend the heap by requesting more memory from the OS
      // sbrk returns the OLD program break (start of new memory)
      let raw_address = sbrk(size_for_sbrk as intptr_t);
      if raw_address == usize::MAX as *mut c_void {
        // sbrk returns (void*)-1 on failure
        return ptr::null_mut();
      }

      // Calculate the aligned address for user content
      // This ensures the returned pointer meets the layout's alignment requirements
      let content_addr = align_to!((raw_address as usize) + header_size, align);

      // Place the block header immediately before the content
      // This allows us to find the header given only the content pointer
      let block = (content_addr - header_size) as *mut Block;
      (*block).is_free = false;
      (*block).size = layout.size();
      (*block).next = ptr::null_mut();

      // Update the linked list of blocks
      if self.first.is_null() {
        // First allocation ever
        self.first = block;
        self.last = block;
      } else {
        // Append to the end of the list
        (*self.last).next = block;
        self.last = block;
      }

      content_addr as *mut u8
    }
  }

  /// Deallocates a previously allocated block of memory.
  ///
  /// This method marks the block as free. If the block is the **last** block
  /// in the list, it also shrinks the heap by calling `sbrk` with a negative
  /// value, returning the memory to the operating system.
  ///
  /// # Arguments
  ///
  /// * `address` - Pointer to the user data region (as returned by `allocate`)
  ///
  /// # Behavior
  ///
  /// ```text
  ///   CASE 1: Deallocating a middle block (only marks as free)
  ///   ═══════════════════════════════════════════════════════════════
  ///
  ///   Before:
  ///   [Block A: in_use] ──► [Block B: in_use] ──► [Block C: in_use]
  ///                                ▲
  ///                         deallocate this
  ///
  ///   After:
  ///   [Block A: in_use] ──► [Block B: FREE] ──► [Block C: in_use]
  ///                                │
  ///                         marked free, but
  ///                         memory NOT returned to OS
  ///
  ///   CASE 2: Deallocating the last block (shrinks heap)
  ///   ═══════════════════════════════════════════════════════════════
  ///
  ///   Before:
  ///   [Block A: in_use] ──► [Block B: in_use] ──► [Block C: in_use]
  ///                                                     ▲
  ///                                              deallocate this
  ///                                                     │
  ///                                              (this is `last`)
  ///
  ///   After:
  ///   [Block A: in_use] ──► [Block B: in_use]
  ///                                │
  ///                         now `last`
  ///
  ///   Heap shrunk via: sbrk(-(block_C_size + overhead))
  /// ```
  ///
  /// # List Update for Last Block Deallocation
  ///
  /// ```text
  ///   Finding the new last block requires traversal:
  ///
  ///   ┌─────────────────┐
  ///   │  BumpAllocator  │
  ///   │  first ─────────┼──► [A] ──► [B] ──► [C]  ◄── last (to be freed)
  ///   └─────────────────┘
  ///
  ///   Traversal: start at first, walk until current.next == last
  ///
  ///   current = A
  ///     └─► A.next = B (not last) ──► continue
  ///   current = B
  ///     └─► B.next = C (== last) ──► STOP
  ///
  ///   Set last = B, then shrink heap
  /// ```
  ///
  /// # Special Case: Single Block
  ///
  /// ```text
  ///   Before:
  ///   ┌─────────────────┐
  ///   │  first ─────────┼──► [Only Block] ◄── last
  ///   └─────────────────┘
  ///
  ///   After deallocate():
  ///   ┌─────────────────┐
  ///   │  first: null    │
  ///   │  last:  null    │
  ///   └─────────────────┘
  ///
  ///   (Heap shrunk, allocator reset to empty state)
  /// ```
  ///
  /// # Safety
  ///
  /// This function is unsafe because:
  /// - It performs raw pointer arithmetic
  /// - It modifies global process state via `sbrk`
  /// - It trusts that `address` was returned by this allocator
  ///
  /// The caller must ensure:
  /// - `address` was previously returned by `allocate` on this allocator
  /// - `address` has not already been deallocated (no double-free)
  /// - No concurrent modifications to the allocator
  ///
  /// # Panics
  ///
  /// This function does not panic, but passing an invalid pointer
  /// results in undefined behavior.
  pub unsafe fn deallocate(
    &mut self,
    address: *mut u8,
  ) {
    unsafe {
      // Null pointer deallocation is a no-op (matches C free() behavior)
      if address.is_null() {
        return;
      }

      // Find the block header by going back header_size bytes
      let block = self.find_block(address);
      (*block).is_free = true;

      // Only the last block can be returned to the OS
      // Middle blocks remain as "holes" in the heap
      if block != self.last {
        return;
      }

      // Update the linked list to remove the last block
      if self.first == self.last {
        // This was the only block - reset to empty state
        self.first = ptr::null_mut();
        self.last = ptr::null_mut();
      } else {
        // Find the second-to-last block (new last)
        // This requires O(n) traversal since we have a singly-linked list
        let mut current: *mut Block = self.first;
        while !(*current).next.is_null() && (*current).next != self.last {
          current = (*current).next;
        }
        self.last = current;
      }

      // Calculate how much memory to release
      // Note: includes extra header_size for alignment padding considerations
      let to_release: usize = align!((*block).size + mem::size_of::<Block>() + mem::size_of::<Block>());

      // Shrink the heap by calling sbrk with a negative value
      let decrement: isize = -(to_release as isize);

      sbrk(decrement as intptr_t);
    }
  }

  /// Finds the block header associated with a user data pointer.
  ///
  /// Given a pointer returned by `allocate`, this method calculates
  /// the location of the corresponding `Block` metadata.
  ///
  /// # Arguments
  ///
  /// * `address` - Pointer to user data (as returned by `allocate`)
  ///
  /// # Returns
  ///
  /// Pointer to the `Block` header for this allocation.
  ///
  /// # Layout
  ///
  /// ```text
  ///   Memory layout:
  ///   ┌────────────────────┬────────────────────────────┐
  ///   │    Block Header    │        User Data           │
  ///   │    (header_size)   │                            │
  ///   └────────────────────┴────────────────────────────┘
  ///   ▲                    ▲
  ///   │                    │
  ///   │                    └── address (input)
  ///   │
  ///   └── returned pointer (address - header_size)
  /// ```
  ///
  /// # Safety
  ///
  /// The caller must ensure:
  /// - `address` was returned by `allocate` on this allocator
  /// - `address` points to valid memory
  ///
  /// Passing an invalid pointer results in undefined behavior.
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
