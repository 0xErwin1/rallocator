pub struct Block {
  pub size: usize,
  pub is_free: bool,
  pub next: *mut Block,
}

impl Block {
  pub fn new(
    size: usize,
    is_free: bool,
    next: *mut Block,
  ) -> Self {
    Self { size, is_free, next }
  }
}
