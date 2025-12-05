# rallocator

A simple bump allocator implementation in Rust using `sbrk`.

## Overview

```
  Bump Allocator:
  
  ┌─────┬─────┬─────┬─────┬──────────────────────┐
  │ A1  │ A2  │ A3  │ A4  │     Free Space       │
  └─────┴─────┴─────┴─────┴──────────────────────┘
                          ▲                      ▲
                    Bump Pointer           Program Break
```

Each allocation "bumps" the pointer forward. Deallocation only releases memory when freeing the last block.

## Usage

```rust
use std::alloc::Layout;
use rallocator::BumpAllocator;

fn main() {
    let mut allocator = BumpAllocator::new();

    unsafe {
        let layout = Layout::new::<u64>();
        let ptr = allocator.allocate(layout) as *mut u64;

        *ptr = 42;
        println!("Value: {}", *ptr);

        allocator.deallocate(ptr as *mut u8);
    }
}
```

## Run Example

```bash
cargo run --example bump
```

## Run Tests

```bash
cargo test
```

## Roadmap

- [x] Bump allocator with `sbrk`
- [ ] `mmap` backend (WIP)

## License

MIT

