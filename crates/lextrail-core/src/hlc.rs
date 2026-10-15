//! Hybrid logical clock timestamps (ARCHITECTURE.md 6.2, D039).
//!
//! Text form: `<12 hex physical millis>-<4 hex counter>-<device UUID>`, all lowercase.
//! Fixed width, so comparing the strings gives the same order as comparing [`Hlc`] values.

use std::fmt;
use std::str::FromStr;

use uuid::Uuid;

use crate::clock::Clock;
use crate::id::parse_canonical_uuid;

/// Largest physical time that fits in 12 hex digits (48 bits).
pub const MAX_MILLIS: u64 = 0xffff_ffff_ffff;

const MILLIS_LEN: usize = 12;
const COUNTER_LEN: usize = 4;
const COUNTER_START: usize = MILLIS_LEN + 1;
const NODE_START: usize = COUNTER_START + COUNTER_LEN + 1;
/// Length of the text form in bytes.
pub const ENCODED_LEN: usize = NODE_START + 36;

/// One HLC timestamp. Field order gives the ordering: physical time, counter, node.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Hlc {
    millis: u64,
    counter: u16,
    node: Uuid,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum HlcError {
    #[error("HLC physical time exceeds {MAX_MILLIS:#x} ms")]
    Overflow,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("invalid HLC timestamp {0:?}")]
pub struct HlcParseError(String);

impl Hlc {
    pub fn new(millis: u64, counter: u16, node: Uuid) -> Result<Self, HlcError> {
        if millis > MAX_MILLIS {
            return Err(HlcError::Overflow);
        }
        Ok(Self {
            millis,
            counter,
            node,
        })
    }

    pub fn millis(&self) -> u64 {
        self.millis
    }

    pub fn counter(&self) -> u16 {
        self.counter
    }

    pub fn node(&self) -> Uuid {
        self.node
    }
}

impl fmt::Display for Hlc {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{:012x}-{:04x}-{}",
            self.millis,
            self.counter,
            self.node.hyphenated()
        )
    }
}

impl FromStr for Hlc {
    type Err = HlcParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let err = || HlcParseError(s.to_owned());
        let bytes = s.as_bytes();
        if bytes.len() != ENCODED_LEN
            || !s.is_ascii()
            || bytes[MILLIS_LEN] != b'-'
            || bytes[NODE_START - 1] != b'-'
        {
            return Err(err());
        }
        let millis = parse_lower_hex(&s[..MILLIS_LEN]).ok_or_else(err)?;
        let counter = parse_lower_hex(&s[COUNTER_START..NODE_START - 1]).ok_or_else(err)?;
        let node = parse_canonical_uuid(&s[NODE_START..]).ok_or_else(err)?;
        // Both fit by construction: 12 hex digits <= MAX_MILLIS, 4 hex digits <= u16::MAX.
        Ok(Self {
            millis,
            counter: counter as u16,
            node,
        })
    }
}

fn parse_lower_hex(s: &str) -> Option<u64> {
    s.bytes()
        .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
        .then(|| u64::from_str_radix(s, 16).ok())
        .flatten()
}

/// Issues HLC timestamps for one device.
///
/// The caller persists [`HlcClock::last`] together with the data it stamped (same
/// transaction) and passes it back to [`HlcClock::new`] on the next start, so timestamps
/// keep increasing even if the system clock is moved back.
#[derive(Debug, Clone)]
pub struct HlcClock<C> {
    clock: C,
    node: Uuid,
    last: Option<Hlc>,
}

impl<C: Clock> HlcClock<C> {
    pub fn new(clock: C, node: Uuid, last: Option<Hlc>) -> Self {
        Self { clock, node, last }
    }

    pub fn node(&self) -> Uuid {
        self.node
    }

    /// The most recent timestamp issued or restored.
    pub fn last(&self) -> Option<Hlc> {
        self.last
    }

    pub fn clock(&self) -> &C {
        &self.clock
    }

    /// Resets the last issued timestamp, e.g. after a transaction that used it rolled back.
    pub fn restore(&mut self, last: Option<Hlc>) {
        self.last = last;
    }

    /// A new timestamp for a local change, greater than every earlier one.
    pub fn now(&mut self) -> Result<Hlc, HlcError> {
        self.tick(None)
    }

    /// A new timestamp after seeing `remote` from another device: greater than both the
    /// local last timestamp and `remote`.
    pub fn receive(&mut self, remote: &Hlc) -> Result<Hlc, HlcError> {
        self.tick(Some(remote))
    }

    fn tick(&mut self, remote: Option<&Hlc>) -> Result<Hlc, HlcError> {
        let wall = self.clock.now_millis();
        let latest = self
            .last
            .iter()
            .chain(remote)
            .max_by_key(|h| (h.millis, h.counter));
        let (millis, counter) = match latest {
            Some(h) if h.millis >= wall => {
                if h.counter == u16::MAX {
                    // Counter overflow: borrow one millisecond.
                    (h.millis + 1, 0)
                } else {
                    (h.millis, h.counter + 1)
                }
            }
            _ => (wall, 0),
        };
        let next = Hlc::new(millis, counter, self.node)?;
        self.last = Some(next);
        Ok(next)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clock::ManualClock;

    const NODE: &str = "019a1b2c-3d4e-7abc-8def-0123456789ab";
    const OTHER: &str = "019a1b2c-3d4e-7abc-8def-ffffffffffff";

    fn node() -> Uuid {
        NODE.parse().unwrap()
    }

    fn other() -> Uuid {
        OTHER.parse().unwrap()
    }

    fn hlc_clock(millis: u64) -> (ManualClock, HlcClock<ManualClock>) {
        let clock = ManualClock::new(millis);
        (clock.clone(), HlcClock::new(clock, node(), None))
    }

    #[test]
    fn formats_fixed_width_lowercase() {
        let h = Hlc::new(0x019a_1b2c_3d4e, 0xab, node()).unwrap();
        let s = h.to_string();
        assert_eq!(s, format!("019a1b2c3d4e-00ab-{NODE}"));
        assert_eq!(s.len(), ENCODED_LEN);
    }

    #[test]
    fn round_trips() {
        for h in [
            Hlc::new(0, 0, node()).unwrap(),
            Hlc::new(1_760_000_000_000, 7, node()).unwrap(),
            Hlc::new(MAX_MILLIS, u16::MAX, other()).unwrap(),
        ] {
            assert_eq!(h.to_string().parse::<Hlc>().unwrap(), h);
        }
    }

    #[test]
    fn rejects_malformed_text() {
        let valid = format!("019a1b2c3d4e-00ab-{NODE}");
        assert!(valid.parse::<Hlc>().is_ok());
        let cases = [
            valid.to_uppercase(),
            valid.replacen("019a1b2c3d4e", "019A1B2C3D4E", 1),
            valid.replacen("00ab", "00AB", 1),
            valid.replacen(NODE, &NODE.to_uppercase(), 1),
            valid.replacen("00ab", "00ag", 1),
            valid.replacen("00ab", "+0ab", 1),
            valid.replacen('-', "_", 1),
            valid[1..].to_owned(),
            format!("{valid}0"),
            format!("19a1b2c3d4e-00ab-{NODE}0"),
            valid.replacen("019a1b2c3d4e", "019a1b2c3d4é", 1),
            String::new(),
        ];
        for s in cases {
            assert!(s.parse::<Hlc>().is_err(), "accepted {s:?}");
        }
    }

    #[test]
    fn string_order_matches_value_order() {
        let mut values = [
            Hlc::new(2, 0, node()).unwrap(),
            Hlc::new(1, 0xffff, node()).unwrap(),
            Hlc::new(1, 0x10, other()).unwrap(),
            Hlc::new(1, 0x10, node()).unwrap(),
            Hlc::new(1, 0x9, other()).unwrap(),
            Hlc::new(0x10, 0, node()).unwrap(),
        ];
        let mut strings: Vec<String> = values.iter().map(Hlc::to_string).collect();
        values.sort();
        strings.sort();
        let sorted: Vec<String> = values.iter().map(Hlc::to_string).collect();
        assert_eq!(strings, sorted);
    }

    #[test]
    fn rejects_millis_beyond_48_bits() {
        assert_eq!(Hlc::new(MAX_MILLIS + 1, 0, node()), Err(HlcError::Overflow));
    }

    #[test]
    fn follows_wall_clock_when_it_moves_forward() {
        let (clock, mut hlc) = hlc_clock(1000);
        assert_eq!(hlc.now().unwrap(), Hlc::new(1000, 0, node()).unwrap());
        clock.set(1005);
        assert_eq!(hlc.now().unwrap(), Hlc::new(1005, 0, node()).unwrap());
    }

    #[test]
    fn counts_within_the_same_millisecond() {
        let (_clock, mut hlc) = hlc_clock(1000);
        let a = hlc.now().unwrap();
        let b = hlc.now().unwrap();
        let c = hlc.now().unwrap();
        assert_eq!((b.millis(), b.counter()), (1000, 1));
        assert_eq!((c.millis(), c.counter()), (1000, 2));
        assert!(a < b && b < c);
    }

    #[test]
    fn stays_monotonic_when_wall_clock_goes_back() {
        let (clock, mut hlc) = hlc_clock(5000);
        let a = hlc.now().unwrap();
        clock.set(1000);
        let b = hlc.now().unwrap();
        assert_eq!((b.millis(), b.counter()), (5000, 1));
        assert!(b > a);
        clock.set(6000);
        assert_eq!(hlc.now().unwrap(), Hlc::new(6000, 0, node()).unwrap());
    }

    #[test]
    fn counter_overflow_borrows_one_millisecond() {
        let clock = ManualClock::new(1000);
        let last = Hlc::new(1000, u16::MAX, node()).unwrap();
        let mut hlc = HlcClock::new(clock, node(), Some(last));
        let next = hlc.now().unwrap();
        assert_eq!((next.millis(), next.counter()), (1001, 0));
        assert!(next > last);
        // The wall clock is still behind: keep counting from the borrowed millisecond.
        let after = hlc.now().unwrap();
        assert_eq!((after.millis(), after.counter()), (1001, 1));
    }

    #[test]
    fn counter_overflow_at_max_millis_is_an_error() {
        let clock = ManualClock::new(0);
        let last = Hlc::new(MAX_MILLIS, u16::MAX, node()).unwrap();
        let mut hlc = HlcClock::new(clock, node(), Some(last));
        assert_eq!(hlc.now(), Err(HlcError::Overflow));
        assert_eq!(hlc.last(), Some(last));
    }

    #[test]
    fn wall_clock_beyond_48_bits_is_an_error() {
        let (_clock, mut hlc) = hlc_clock(MAX_MILLIS + 1);
        assert_eq!(hlc.now(), Err(HlcError::Overflow));
        assert_eq!(hlc.last(), None);
    }

    #[test]
    fn continues_after_restored_last() {
        let clock = ManualClock::new(1000);
        let last = Hlc::new(9000, 3, node()).unwrap();
        let mut hlc = HlcClock::new(clock, node(), Some(last));
        assert_eq!(hlc.now().unwrap(), Hlc::new(9000, 4, node()).unwrap());
    }

    #[test]
    fn receive_moves_past_a_remote_timestamp_ahead_of_us() {
        let (_clock, mut hlc) = hlc_clock(1000);
        hlc.now().unwrap();
        let remote = Hlc::new(4000, 7, other()).unwrap();
        let got = hlc.receive(&remote).unwrap();
        assert_eq!(got, Hlc::new(4000, 8, node()).unwrap());
        assert!(got > remote);
        // Later local timestamps stay ahead of the remote one.
        assert!(hlc.now().unwrap() > remote);
    }

    #[test]
    fn receive_keeps_local_time_when_remote_is_behind() {
        let (_clock, mut hlc) = hlc_clock(1000);
        let local = hlc.now().unwrap();
        let remote = Hlc::new(500, 9, other()).unwrap();
        let got = hlc.receive(&remote).unwrap();
        assert_eq!((got.millis(), got.counter()), (1000, 1));
        assert!(got > local);
    }

    #[test]
    fn receive_uses_wall_clock_when_it_is_ahead_of_both() {
        let (clock, mut hlc) = hlc_clock(1000);
        hlc.now().unwrap();
        clock.set(8000);
        let got = hlc.receive(&Hlc::new(4000, 2, other()).unwrap()).unwrap();
        assert_eq!(got, Hlc::new(8000, 0, node()).unwrap());
    }

    #[test]
    fn receive_on_the_same_millisecond_takes_the_larger_counter() {
        let (_clock, mut hlc) = hlc_clock(1000);
        hlc.now().unwrap(); // (1000, 0)
        let got = hlc.receive(&Hlc::new(1000, 5, other()).unwrap()).unwrap();
        assert_eq!((got.millis(), got.counter()), (1000, 6));
    }

    #[test]
    fn receive_overflow_borrows_one_millisecond() {
        let (_clock, mut hlc) = hlc_clock(1000);
        let got = hlc
            .receive(&Hlc::new(1000, u16::MAX, other()).unwrap())
            .unwrap();
        assert_eq!((got.millis(), got.counter()), (1001, 0));
    }

    #[test]
    fn restore_rolls_back_last() {
        let (_clock, mut hlc) = hlc_clock(1000);
        let before = hlc.last();
        hlc.now().unwrap();
        hlc.restore(before);
        assert_eq!(hlc.last(), None);
    }
}
