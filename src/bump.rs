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

/// Strategy for searching free blocks in the allocator.
///
/// When reusing freed memory blocks, different search strategies offer
/// different trade-offs between allocation speed and memory utilization.
///
/// # Strategies
///
/// ```text
///   FREE BLOCK SEARCH STRATEGIES
///   ═══════════════════════════════════════════════════════════════════════
///
///   Given blocks: [A:64] → [B:128,free] → [C:32,free] → [D:256,free] → [E:100]
///   Request: 50 bytes
///
///   FIRST FIT: Start from beginning, return first match
///   ┌──────────────────────────────────────────────────────────────────────┐
///   │  [A:64] → [B:128,free] → [C:32,free] → [D:256,free] → [E:100]       │
///   │     ↓           ↓                                                    │
///   │   skip     ✓ MATCH! (128 >= 50)                                      │
///   │  (in use)                                                            │
///   │                                                                      │
///   │  Returns: B (first free block that fits)                             │
///   │  Pros: Fast - O(n) worst case, often much faster                     │
///   │  Cons: Can cause fragmentation at the start of the heap              │
///   └──────────────────────────────────────────────────────────────────────┘
///
///   NEXT FIT: Start from last allocation position, wrap around if needed
///   ┌──────────────────────────────────────────────────────────────────────┐
///   │  Last allocation was at C, so search starts after C:                 │
///   │                                                                      │
///   │  [A:64] → [B:128,free] → [C:32,free] → [D:256,free] → [E:100]       │
///   │                               │             ↓                        │
///   │                          last_search   ✓ MATCH! (256 >= 50)          │
///   │                                                                      │
///   │  Returns: D (first free block after last_search that fits)           │
///   │  Pros: Spreads allocations, avoids always fragmenting start          │
///   │  Cons: May miss better-fitting blocks earlier in list                │
///   └──────────────────────────────────────────────────────────────────────┘
///
///   BEST FIT: Search entire list, return smallest adequate block
///   ┌──────────────────────────────────────────────────────────────────────┐
///   │  [A:64] → [B:128,free] → [C:32,free] → [D:256,free] → [E:100]       │
///   │              ↓               ↓             ↓                         │
///   │          128 >= 50       32 < 50       256 >= 50                     │
///   │          candidate      too small      candidate                     │
///   │              ↓                             ↓                         │
///   │          128 bytes                     256 bytes                     │
///   │              ↓                                                       │
///   │          ✓ BEST! (128 < 256, smallest that fits)                     │
///   │                                                                      │
///   │  Returns: B (smallest free block that fits)                          │
///   │  Pros: Minimizes wasted space within blocks                          │
///   │  Cons: Slower - always O(n), must check all blocks                   │
///   └──────────────────────────────────────────────────────────────────────┘
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SearchMode {
  /// First Fit: Returns the first free block large enough.
  ///
  /// Starts searching from the beginning of the block list and returns
  /// the first block that is both free and has sufficient size.
  ///
  /// - **Time Complexity**: O(n) worst case, but often faster
  /// - **Memory Efficiency**: Can cause fragmentation at heap start
  /// - **Best For**: General-purpose use, when speed is priority
  #[default]
  FirstFit,

  /// Next Fit: Like First Fit, but remembers where the last search ended.
  ///
  /// Starts searching from where the previous successful search ended,
  /// wrapping around to the beginning if necessary. This distributes
  /// allocations more evenly across the heap.
  ///
  /// - **Time Complexity**: O(n) worst case
  /// - **Memory Efficiency**: Better distribution, less clustering
  /// - **Best For**: Long-running programs with many alloc/free cycles
  NextFit,

  /// Best Fit: Returns the smallest free block that fits.
  ///
  /// Searches the entire list to find the free block that most closely
  /// matches the requested size, minimizing internal fragmentation.
  ///
  /// - **Time Complexity**: Always O(n) - must check all blocks
  /// - **Memory Efficiency**: Minimizes wasted space per allocation
  /// - **Best For**: Memory-constrained environments
  BestFit,
}

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
/// * `search_mode` - Strategy for finding free blocks (FirstFit, NextFit, BestFit)
/// * `last_search` - Used by NextFit to remember where the last search ended
///
/// Both `first` and `last` pointers are `null` when the allocator is empty.
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

  /// Strategy used to search for free blocks when reusing memory.
  /// See [`SearchMode`] for available strategies.
  search_mode: SearchMode,

  /// Pointer to the block where the last successful search ended.
  /// Used exclusively by [`SearchMode::NextFit`] to remember the
  /// starting position for the next search.
  last_search: *mut Block,
}

impl BumpAllocator {
  /// Creates a new, empty `BumpAllocator` with the default search mode (FirstFit).
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
  /// // allocator.search_mode == SearchMode::FirstFit
  /// ```
  ///
  /// # State Diagram
  ///
  /// ```text
  ///   After new():
  ///   ┌───────────────────────────┐
  ///   │      BumpAllocator        │
  ///   │                           │
  ///   │  first: null              │
  ///   │  last:  null              │
  ///   │  search_mode: FirstFit    │
  ///   │  last_search: null        │
  ///   └───────────────────────────┘
  /// ```
  pub fn new() -> Self {
    Self {
      first: ptr::null_mut(),
      last: ptr::null_mut(),
      search_mode: SearchMode::default(),
      last_search: ptr::null_mut(),
    }
  }

  /// Creates a new, empty `BumpAllocator` with the specified search mode.
  ///
  /// # Arguments
  ///
  /// * `search_mode` - The strategy to use when searching for free blocks.
  ///   See [`SearchMode`] for available options.
  ///
  /// # Returns
  ///
  /// A new allocator instance configured with the specified search mode.
  ///
  /// # Example
  ///
  /// ```rust,ignore
  /// use rallocator::{BumpAllocator, SearchMode};
  ///
  /// // Create allocator with Best Fit strategy
  /// let allocator = BumpAllocator::with_search_mode(SearchMode::BestFit);
  ///
  /// // Create allocator with Next Fit strategy
  /// let allocator = BumpAllocator::with_search_mode(SearchMode::NextFit);
  /// ```
  ///
  /// # Search Mode Comparison
  ///
  /// ```text
  ///   ┌─────────────┬───────────────────────────────────────────────────────┐
  ///   │   Mode      │   Description                                         │
  ///   ├─────────────┼───────────────────────────────────────────────────────┤
  ///   │ FirstFit    │ Fast, returns first adequate block                    │
  ///   │ NextFit     │ Balanced, distributes allocations evenly              │
  ///   │ BestFit     │ Memory-efficient, minimizes wasted space              │
  ///   └─────────────┴───────────────────────────────────────────────────────┘
  /// ```
  pub fn with_search_mode(search_mode: SearchMode) -> Self {
    Self {
      first: ptr::null_mut(),
      last: ptr::null_mut(),
      search_mode,
      last_search: ptr::null_mut(),
    }
  }

  /// Returns the current search mode of the allocator.
  ///
  /// # Example
  ///
  /// ```rust,ignore
  /// use rallocator::{BumpAllocator, SearchMode};
  ///
  /// let allocator = BumpAllocator::with_search_mode(SearchMode::BestFit);
  /// assert_eq!(allocator.search_mode(), SearchMode::BestFit);
  /// ```
  pub fn search_mode(&self) -> SearchMode {
    self.search_mode
  }

  /// Sets the search mode for the allocator.
  ///
  /// This can be changed at any time and will affect subsequent allocations.
  /// Note: Changing to [`SearchMode::NextFit`] resets the `last_search` pointer
  /// to the beginning of the list.
  ///
  /// # Arguments
  ///
  /// * `mode` - The new search mode to use.
  ///
  /// # Example
  ///
  /// ```rust,ignore
  /// use rallocator::{BumpAllocator, SearchMode};
  ///
  /// let mut allocator = BumpAllocator::new(); // Default: FirstFit
  /// allocator.set_search_mode(SearchMode::BestFit);
  /// ```
  pub fn set_search_mode(&mut self, mode: SearchMode) {
    self.search_mode = mode;
    // Reset last_search when changing modes to avoid stale pointers
    if mode != SearchMode::NextFit {
      self.last_search = ptr::null_mut();
    }
  }

  /// Searches the block list for a free block of sufficient size.
  ///
  /// This method uses the configured [`SearchMode`] to find a suitable block:
  ///
  /// - [`SearchMode::FirstFit`]: Returns the first free block that fits
  /// - [`SearchMode::NextFit`]: Starts from last allocation, wraps around
  /// - [`SearchMode::BestFit`]: Returns the smallest block that fits
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
  ///
  ///   FirstFit: Returns Block 2 (128 >= 100, first match)
  ///   BestFit:  Returns Block 2 (128 is closest to 100)
  ///   NextFit:  Depends on last_search position
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
    &mut self,
    size: usize,
  ) -> *mut Block {
    // SAFETY: All called functions are unsafe but maintain the same invariants
    // as this function - they require valid internal state and no concurrent access.
    unsafe {
      match self.search_mode {
        SearchMode::FirstFit => self.find_free_block_first_fit(size),
        SearchMode::NextFit => self.find_free_block_next_fit(size),
        SearchMode::BestFit => self.find_free_block_best_fit(size),
      }
    }
  }

  /// First Fit: Returns the first free block that is large enough.
  ///
  /// Searches from the beginning of the block list.
  ///
  /// # Time Complexity
  ///
  /// O(n) worst case, but typically faster as it stops at the first match.
  unsafe fn find_free_block_first_fit(
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

  /// Next Fit: Like First Fit, but starts where the last search ended.
  ///
  /// This strategy distributes allocations more evenly across the heap,
  /// reducing fragmentation that tends to cluster at the beginning.
  ///
  /// # Algorithm
  ///
  /// ```text
  ///   1. Start from last_search (or first if null)
  ///   2. Search forward until end of list
  ///   3. If not found, wrap around and search from first to last_search
  ///   4. Update last_search to the found block (or leave unchanged if not found)
  /// ```
  ///
  /// # Time Complexity
  ///
  /// O(n) worst case - may need to traverse entire list.
  unsafe fn find_free_block_next_fit(
    &mut self,
    size: usize,
  ) -> *mut Block {
    unsafe {
      // Start from last_search position, or from the beginning if null
      let start = if self.last_search.is_null() {
        self.first
      } else {
        self.last_search
      };

      // First pass: search from start to end
      let mut current = start;
      while !current.is_null() {
        if (*current).is_free && (*current).size >= size {
          self.last_search = current;
          return current;
        }
        current = (*current).next;
      }

      // Second pass: wrap around, search from first to start
      current = self.first;
      while !current.is_null() && current != start {
        if (*current).is_free && (*current).size >= size {
          self.last_search = current;
          return current;
        }
        current = (*current).next;
      }

      ptr::null_mut()
    }
  }

  /// Best Fit: Returns the smallest free block that is large enough.
  ///
  /// Searches the entire list to find the block that minimizes wasted space.
  ///
  /// # Algorithm
  ///
  /// ```text
  ///   Example: Looking for 100 bytes
  ///
  ///   [128,free] → [256,free] → [110,free] → [64,free]
  ///       ↓            ↓            ↓            ↓
  ///   candidate    candidate    candidate    too small
  ///    (128)        (256)        (110)
  ///
  ///   Best = 110 (closest to 100 without being smaller)
  /// ```
  ///
  /// # Time Complexity
  ///
  /// Always O(n) - must check all blocks to find the best fit.
  unsafe fn find_free_block_best_fit(
    &self,
    size: usize,
  ) -> *mut Block {
    unsafe {
      let mut best: *mut Block = ptr::null_mut();
      let mut best_size: usize = usize::MAX;
      let mut current: *mut Block = self.first;

      while !current.is_null() {
        let block_size = (*current).size;
        // Check if this block is free, large enough, and better than current best
        if (*current).is_free && block_size >= size && block_size < best_size {
          best = current;
          best_size = block_size;

          // Perfect fit - no need to continue searching
          if block_size == size {
            return best;
          }
        }
        current = (*current).next;
      }

      best
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

  // ═══════════════════════════════════════════════════════════════════════════
  // SearchMode Tests
  // ═══════════════════════════════════════════════════════════════════════════

  #[test]
  fn search_mode_default_is_first_fit() {
    let allocator = BumpAllocator::new();
    assert_eq!(allocator.search_mode(), SearchMode::FirstFit);
  }

  #[test]
  fn with_search_mode_sets_mode_correctly() {
    let allocator_first = BumpAllocator::with_search_mode(SearchMode::FirstFit);
    let allocator_next = BumpAllocator::with_search_mode(SearchMode::NextFit);
    let allocator_best = BumpAllocator::with_search_mode(SearchMode::BestFit);

    assert_eq!(allocator_first.search_mode(), SearchMode::FirstFit);
    assert_eq!(allocator_next.search_mode(), SearchMode::NextFit);
    assert_eq!(allocator_best.search_mode(), SearchMode::BestFit);
  }

  #[test]
  fn set_search_mode_changes_mode() {
    let mut allocator = BumpAllocator::new();
    assert_eq!(allocator.search_mode(), SearchMode::FirstFit);

    allocator.set_search_mode(SearchMode::BestFit);
    assert_eq!(allocator.search_mode(), SearchMode::BestFit);

    allocator.set_search_mode(SearchMode::NextFit);
    assert_eq!(allocator.search_mode(), SearchMode::NextFit);

    allocator.set_search_mode(SearchMode::FirstFit);
    assert_eq!(allocator.search_mode(), SearchMode::FirstFit);
  }

  /// Helper to create an allocator with multiple blocks and free some of them.
  /// Returns the allocator and the pointers to all allocated blocks.
  ///
  /// Creates blocks with sizes: [64, 128, 32, 256, 64] bytes
  /// Marks blocks at indices in `free_indices` as free.
  unsafe fn setup_allocator_with_blocks(
    search_mode: SearchMode,
    free_indices: &[usize],
  ) -> (BumpAllocator, Vec<*mut u8>) {
    unsafe {
      let mut allocator = BumpAllocator::with_search_mode(search_mode);
      let sizes = [64usize, 128, 32, 256, 64];
      let mut ptrs = Vec::new();

      // Allocate all blocks
      for &size in &sizes {
        let layout = Layout::from_size_align(size, 8).unwrap();
        let ptr = allocator.allocate(layout);
        assert!(!ptr.is_null());
        ptrs.push(ptr);
      }

      // Mark specified blocks as free
      for &idx in free_indices {
        let block = allocator.find_block(ptrs[idx]);
        (*block).is_free = true;
      }

      (allocator, ptrs)
    }
  }

  #[test]
  fn first_fit_returns_first_matching_block() {
    unsafe {
      // Setup: blocks [64, 128, 32, 256, 64], free indices [1, 3] (sizes 128 and 256)
      let (mut allocator, ptrs) = setup_allocator_with_blocks(SearchMode::FirstFit, &[1, 3]);

      // Looking for 100 bytes: should return block 1 (128 bytes) - first free that fits
      let found = allocator.find_free_block(100);
      assert!(!found.is_null());

      // The found block should be the one at index 1 (128 bytes)
      let expected_block = allocator.find_block(ptrs[1]);
      assert_eq!(found, expected_block);
      assert_eq!((*found).size, 128);
    }
  }

  #[test]
  fn first_fit_returns_null_when_no_block_fits() {
    unsafe {
      // Setup: blocks [64, 128, 32, 256, 64], free indices [0, 2] (sizes 64 and 32)
      let (mut allocator, _ptrs) = setup_allocator_with_blocks(SearchMode::FirstFit, &[0, 2]);

      // Looking for 100 bytes: no free block is large enough
      let found = allocator.find_free_block(100);
      assert!(found.is_null());
    }
  }

  #[test]
  fn best_fit_returns_smallest_adequate_block() {
    unsafe {
      // Setup: blocks [64, 128, 32, 256, 64], free indices [1, 3] (sizes 128 and 256)
      let (mut allocator, ptrs) = setup_allocator_with_blocks(SearchMode::BestFit, &[1, 3]);

      // Looking for 100 bytes: should return block 1 (128 bytes) - smallest that fits
      let found = allocator.find_free_block(100);
      assert!(!found.is_null());

      let expected_block = allocator.find_block(ptrs[1]);
      assert_eq!(found, expected_block);
      assert_eq!((*found).size, 128);
    }
  }

  #[test]
  fn best_fit_chooses_smaller_block_over_earlier_larger_block() {
    unsafe {
      // Setup: blocks [64, 128, 32, 256, 64], free indices [1, 3, 4] (sizes 128, 256, 64)
      let (mut allocator, ptrs) = setup_allocator_with_blocks(SearchMode::BestFit, &[1, 3, 4]);

      // Looking for 50 bytes: should return block 4 (64 bytes) even though block 1 (128) comes first
      let found = allocator.find_free_block(50);
      assert!(!found.is_null());

      let expected_block = allocator.find_block(ptrs[4]);
      assert_eq!(found, expected_block);
      assert_eq!((*found).size, 64);
    }
  }

  #[test]
  fn best_fit_returns_perfect_fit_immediately() {
    unsafe {
      // Setup: blocks [64, 128, 32, 256, 64], free all
      let (mut allocator, ptrs) = setup_allocator_with_blocks(SearchMode::BestFit, &[0, 1, 2, 3, 4]);

      // Looking for exactly 128 bytes: should return block 1 (perfect fit)
      let found = allocator.find_free_block(128);
      assert!(!found.is_null());

      let expected_block = allocator.find_block(ptrs[1]);
      assert_eq!(found, expected_block);
      assert_eq!((*found).size, 128);
    }
  }

  #[test]
  fn next_fit_starts_from_last_search_position() {
    unsafe {
      // Setup: blocks [64, 128, 32, 256, 64], free indices [0, 1, 4] (sizes 64, 128, 64)
      let (mut allocator, ptrs) = setup_allocator_with_blocks(SearchMode::NextFit, &[0, 1, 4]);

      // First search for 50 bytes: should find block 0 (64 bytes) and update last_search
      let found1 = allocator.find_free_block(50);
      assert!(!found1.is_null());
      let block0 = allocator.find_block(ptrs[0]);
      assert_eq!(found1, block0);

      // Mark block 0 as used
      (*found1).is_free = false;

      // Second search for 50 bytes: should start from block 0, find block 1 (128 bytes)
      let found2 = allocator.find_free_block(50);
      assert!(!found2.is_null());
      let block1 = allocator.find_block(ptrs[1]);
      assert_eq!(found2, block1);

      // Mark block 1 as used
      (*found2).is_free = false;

      // Third search for 50 bytes: should continue from block 1, find block 4 (64 bytes)
      let found3 = allocator.find_free_block(50);
      assert!(!found3.is_null());
      let block4 = allocator.find_block(ptrs[4]);
      assert_eq!(found3, block4);
    }
  }

  #[test]
  fn next_fit_wraps_around_to_beginning() {
    unsafe {
      // Setup: blocks [64, 128, 32, 256, 64], free indices [0, 4] (sizes 64, 64)
      let (mut allocator, ptrs) = setup_allocator_with_blocks(SearchMode::NextFit, &[0, 4]);

      // First search: find block 0
      let found1 = allocator.find_free_block(50);
      assert!(!found1.is_null());
      (*found1).is_free = false;

      // Second search: find block 4 (continues from block 0)
      let found2 = allocator.find_free_block(50);
      assert!(!found2.is_null());
      let block4 = allocator.find_block(ptrs[4]);
      assert_eq!(found2, block4);

      // Free block 0 again, keep block 4 as used
      let block0 = allocator.find_block(ptrs[0]);
      (*block0).is_free = true;
      (*found2).is_free = false;

      // Third search: should wrap around and find block 0
      let found3 = allocator.find_free_block(50);
      assert!(!found3.is_null());
      assert_eq!(found3, block0);
    }
  }

  #[test]
  fn next_fit_returns_null_when_no_block_fits() {
    unsafe {
      // Setup: blocks [64, 128, 32, 256, 64], free indices [2] (size 32 only)
      let (mut allocator, _ptrs) = setup_allocator_with_blocks(SearchMode::NextFit, &[2]);

      // Looking for 100 bytes: no free block is large enough
      let found = allocator.find_free_block(100);
      assert!(found.is_null());
    }
  }

  #[test]
  fn all_modes_return_null_on_empty_allocator() {
    for mode in [SearchMode::FirstFit, SearchMode::NextFit, SearchMode::BestFit] {
      let mut allocator = BumpAllocator::with_search_mode(mode);

      unsafe {
        let found = allocator.find_free_block(100);
        assert!(found.is_null(), "Mode {:?} should return null on empty allocator", mode);
      }
    }
  }

  #[test]
  fn all_modes_return_null_when_all_blocks_in_use() {
    for mode in [SearchMode::FirstFit, SearchMode::NextFit, SearchMode::BestFit] {
      unsafe {
        // Setup with no free blocks
        let (mut allocator, _ptrs) = setup_allocator_with_blocks(mode, &[]);

        let found = allocator.find_free_block(32);
        assert!(found.is_null(), "Mode {:?} should return null when no blocks are free", mode);
      }
    }
  }
}
