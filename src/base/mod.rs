//! What the bot's first life in the original left in its numbers: the same random draws, float sums, rounding and text
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

/// A struct and its `Default`, each field's default beside it: `pub hits: i64 = 1,` (no `= ...`: the type's own default).
#[macro_export]
macro_rules! defaults {
    (@or) => { Default::default() };
    (@or $default:expr) => { $default };
    ($(#[$meta:meta])* $vis:vis struct $name:ident { $($(#[$fmeta:meta])* $fvis:vis $field:ident: $ty:ty $(= $default:expr)?),* $(,)? }) => {
        $(#[$meta])* $vis struct $name { $($(#[$fmeta])* $fvis $field: $ty),* }
        impl Default for $name {
            fn default() -> Self {
                $name { $($field: $crate::defaults!(@or $($default)?)),* }
            }
        }
    };
}
