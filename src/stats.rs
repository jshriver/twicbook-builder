use std::sync::atomic::{AtomicU64, Ordering};

use crate::filter::RejectReason;

#[derive(Default)]
pub struct Stats {
    pub seen: AtomicU64,
    pub missing_elo: AtomicU64,
    pub low_elo: AtomicU64,
    pub non_standard_variant: AtomicU64,
    pub non_standard_start: AtomicU64,
    pub illegal_move: AtomicU64,
    pub ply_count_mismatch: AtomicU64,
    pub too_short: AtomicU64,
    pub duplicate: AtomicU64,
    pub written: AtomicU64,
}

impl Stats {
    pub fn record_reject(&self, reason: RejectReason) {
        self.seen.fetch_add(1, Ordering::Relaxed);
        let counter = match reason {
            RejectReason::MissingElo => &self.missing_elo,
            RejectReason::LowElo => &self.low_elo,
            RejectReason::NonStandardVariant => &self.non_standard_variant,
            RejectReason::NonStandardStart => &self.non_standard_start,
            RejectReason::IllegalMove => &self.illegal_move,
            RejectReason::PlyCountMismatch => &self.ply_count_mismatch,
            RejectReason::TooShort => &self.too_short,
            RejectReason::Duplicate => &self.duplicate,
        };
        counter.fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_written(&self) {
        self.seen.fetch_add(1, Ordering::Relaxed);
        self.written.fetch_add(1, Ordering::Relaxed);
    }

    pub fn report(&self) {
        let seen = self.seen.load(Ordering::Relaxed);
        eprintln!(
            "--- {seen} games seen ---\n\
             written             : {}\n\
             missing Elo         : {}\n\
             below Elo floor     : {}\n\
             non-standard variant: {}\n\
             non-standard start  : {}\n\
             illegal / corrupt   : {}\n\
             ply-count mismatch  : {}\n\
             too short           : {}\n\
             duplicate           : {}",
            self.written.load(Ordering::Relaxed),
            self.missing_elo.load(Ordering::Relaxed),
            self.low_elo.load(Ordering::Relaxed),
            self.non_standard_variant.load(Ordering::Relaxed),
            self.non_standard_start.load(Ordering::Relaxed),
            self.illegal_move.load(Ordering::Relaxed),
            self.ply_count_mismatch.load(Ordering::Relaxed),
            self.too_short.load(Ordering::Relaxed),
            self.duplicate.load(Ordering::Relaxed),
        );
    }
}
