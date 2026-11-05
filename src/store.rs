//! The samples of the last `retention`, in memory only.

use std::collections::VecDeque;
use std::time::Duration;

use crate::model::Sample;

pub struct Store {
    samples: VecDeque<Sample>,
    capacity: usize,
}

impl Store {
    /// Room for `retention` worth of samples taken every `interval` (and one more, so a full window is never short by one).
    pub fn new(interval: Duration, retention: Duration) -> Self {
        let per = interval.as_millis().max(1);
        let capacity = (retention.as_millis() / per) as usize + 1;
        Store { samples: VecDeque::with_capacity(capacity.min(4096)), capacity: capacity.max(1) }
    }

    pub fn push(&mut self, sample: Sample) {
        if self.samples.len() == self.capacity {
            self.samples.pop_front();
        }
        self.samples.push_back(sample);
    }

    pub fn latest(&self) -> Option<&Sample> {
        self.samples.back()
    }

    /// The samples taken at or after `since`, oldest first.
    pub fn since(&self, since: time::OffsetDateTime) -> Vec<&Sample> {
        self.samples.iter().filter(|sample| sample.at >= since).collect()
    }

    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.samples.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::OffsetDateTime;

    fn sample(seconds: i64) -> Sample {
        Sample { at: OffsetDateTime::from_unix_timestamp(1_000_000 + seconds).unwrap(), adapters: Vec::new() }
    }

    #[test]
    fn keeps_only_the_retention() {
        let mut store = Store::new(Duration::from_secs(2), Duration::from_secs(10)); // capacity 6
        for second in 0..20 {
            store.push(sample(second * 2));
        }
        assert_eq!(store.len(), 6);
        assert_eq!(store.latest().unwrap().at.unix_timestamp(), 1_000_000 + 38);
    }

    #[test]
    fn since_gives_the_newer_samples_oldest_first() {
        let mut store = Store::new(Duration::from_secs(2), Duration::from_secs(60));
        for second in [0, 2, 4, 6] {
            store.push(sample(second));
        }
        let cut = OffsetDateTime::from_unix_timestamp(1_000_000 + 3).unwrap();
        let seconds: Vec<i64> = store.since(cut).iter().map(|s| s.at.unix_timestamp() - 1_000_000).collect();
        assert_eq!(seconds, vec![4, 6]);
    }

    #[test]
    fn an_empty_store_has_no_latest() {
        assert!(Store::new(Duration::from_secs(1), Duration::from_secs(1)).latest().is_none());
    }
}
