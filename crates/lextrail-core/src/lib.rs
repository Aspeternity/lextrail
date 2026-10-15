//! LexTrail core: data model, op-log, sync merge logic and FSRS wrapper.
//!
//! Pure logic only: no SQLite (D037) and no direct OS access, so the crate can later be
//! compiled to WebAssembly for the PWA. Wall-clock time comes in through [`Clock`].

mod clock;
mod hlc;
mod id;
pub mod lookup;
pub mod text;

pub use clock::{Clock, ManualClock, SystemClock};
pub use hlc::{ENCODED_LEN as HLC_ENCODED_LEN, Hlc, HlcClock, HlcError, HlcParseError, MAX_MILLIS};
pub use id::parse_canonical_uuid;
pub use uuid::Uuid;
