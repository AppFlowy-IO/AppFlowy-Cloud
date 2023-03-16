use std::ops::{Range, RangeInclusive, RangeToInclusive};

#[derive(Clone)]
pub struct RevRange {
    pub(crate) start: i64,
    pub(crate) end: i64,
}

impl RevRange {
    /// Construct a new `RevRange` representing the range [start..end).
    /// It is an invariant that `start <= end`.
    pub fn new(start: i64, end: i64) -> RevRange {
        debug_assert!(start <= end);
        RevRange { start, end }
    }
}

impl From<RangeInclusive<i64>> for RevRange {
    fn from(src: RangeInclusive<i64>) -> RevRange {
        RevRange::new(*src.start(), src.end().saturating_add(1))
    }
}

impl From<RangeToInclusive<i64>> for RevRange {
    fn from(src: RangeToInclusive<i64>) -> RevRange {
        RevRange::new(0, src.end.saturating_add(1))
    }
}

impl From<Range<i64>> for RevRange {
    fn from(src: Range<i64>) -> RevRange {
        let Range { start, end } = src;
        RevRange { start, end }
    }
}

impl Iterator for RevRange {
    type Item = i64;

    fn next(&mut self) -> Option<i64> {
        if self.start > self.end {
            return None;
        }
        let val = self.start;
        self.start += 1;
        Some(val)
    }
}
