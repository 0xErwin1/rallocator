//! # rallocator - A Custom Memory Allocator Library
//!
//! This crate provides a simple **bump allocator** (also known as an arena allocator)
//! implementation in Rust that manages memory using the `sbrk` system call.
//!
//! ## Overview
//!
//! A bump allocator is one of the simplest memory allocation strategies:
//!
//! ```text
//!   Bump Allocator Concept:
//!
//!   ┌──────────────────────────────────────────────────────────────────────┐
//!   │                         HEAP MEMORY                                  │
//!   │                                                                      │
//!   │   ┌─────┬─────┬─────┬─────┬───────────────────────────────────────┐  │
//!   │   │ A1  │ A2  │ A3  │ A4  │            Free Space                 │  │
//!   │   └─────┴─────┴─────┴─────┴───────────────────────────────────────┘  │
//!   │                           ▲                                     ▲    │
//!   │                           │                                     │    │
//!   │                       Bump Pointer                         Program   │
//!   │                       (next alloc)                          Break    │
//!   │                                                                      │
//!   └──────────────────────────────────────────────────────────────────────┘
//!
//!   Each allocation "bumps" the pointer forward.
//!   Fast allocation: O(1) - just move the pointer.
//! ```
//!
//! ## Crate Structure
//!
//! ```text
//!   rallocator
//!   ├── align      - Alignment macros (align!, align_to!)
//!   ├── block      - Block metadata structure (internal)
//!   └── bump       - BumpAllocator implementation
//! ```
//!
//! ## Quick Start
//!
//! ```rust,ignore
//! use std::alloc::Layout;
//! use rallocator::BumpAllocator;
//!
//! fn main() {
//!     let mut allocator = BumpAllocator::new();
//!
//!     unsafe {
//!         // Allocate memory for a u64
//!         let layout = Layout::new::<u64>();
//!         let ptr = allocator.allocate(layout) as *mut u64;
//!
//!         // Use the memory
//!         *ptr = 42;
//!         println!("Value: {}", *ptr);
//!
//!         // Free the memory
//!         allocator.deallocate(ptr as *mut u8);
//!     }
//! }
//! ```
//!
//! ## How It Works
//!
//! The allocator uses `sbrk(2)` to extend the program's data segment:
//!
//! ```text
//!   Program Memory Layout:
//!
//!   High Address ┌─────────────────────┐
//!                │       Stack         │ ↓ grows down
//!                │         │           │
//!                │         ▼           │
//!                │                     │
//!                │         ▲           │
//!                │         │           │
//!                │       Heap          │ ↑ grows up (sbrk)
//!                ├─────────────────────┤ ← Program Break
//!                │   Uninitialized     │
//!                │       Data          │
//!                ├─────────────────────┤
//!                │   Initialized       │
//!                │       Data          │
//!                ├─────────────────────┤
//!                │       Text          │
//!   Low Address  └─────────────────────┘
//! ```
//!
//! Each allocation creates a block with metadata:
//!
//! ```text
//!   Single Allocation:
//!   ┌───────────────────────┬────────────────────────────────┐
//!   │    Block Header       │         User Data              │
//!   │  ┌─────────────────┐  │                                │
//!   │  │ size: N         │  │  ┌──────────────────────────┐  │
//!   │  │ is_free: false  │  │  │                          │  │
//!   │  │ next: null/ptr  │  │  │     N bytes usable       │  │
//!   │  └─────────────────┘  │  │                          │  │
//!   │      24 bytes         │  └──────────────────────────┘  │
//!   └───────────────────────┴────────────────────────────────┘
//!                           ▲
//!                           └── Pointer returned to user
//! ```
//!
//! ## Features
//!
//! - **Simple implementation**: Easy to understand and modify
//! - **Direct OS interaction**: Uses `sbrk` for memory management
//! - **Proper alignment**: Respects layout alignment requirements
//! - **Linked list tracking**: Maintains metadata for all allocations
//!
//! ## Limitations
//!
//! - **Single-threaded only**: No synchronization primitives
//! - **Limited deallocation**: Only the last block can be freed to the OS
//! - **No block reuse**: Currently doesn't reuse freed middle blocks
//! - **Unix-only**: Requires `libc` and `sbrk` (POSIX systems)
//!
//! ## Safety
//!
//! This crate is inherently unsafe as it deals with raw memory management.
//! All allocation and deallocation operations require `unsafe` blocks.

pub mod align;
mod block;
mod bump;

pub use bump::{BumpAllocator, SearchMode, print_alloc};
