//! What the bot's first life in Python left in its numbers: the same random draws, float sums, rounding and text
//! forms, so that a seeded model run decides exactly as it did there (`sim trace`).

pub mod num;
pub mod random;
pub mod re;
pub mod text;
pub mod value;

pub use num::*;
pub use random::Random;
pub use text::*;
pub use value::JsonExt;
