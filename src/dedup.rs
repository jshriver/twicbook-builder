use shakmaty::san::SanPlus;
use shakmaty::zobrist::{Zobrist64, ZobristHash};
use shakmaty::{Chess, EnPassantMode};

use crate::cli::DedupMode;

/// Computes the dedup key for a (truncated) opening line.
///
/// `Position` mode hashes only the resulting board position (pieces, side
/// to move, castling rights, en passant square) -- exactly the fields a
/// Zobrist hash covers -- so transpositions such as
/// `1.Nf3 d5 2.d4` and `1.d4 d5 2.Nf3` collapse to the same key.
///
/// `Moves` mode instead hashes the literal SAN sequence, which is
/// move-order sensitive and cheaper to reason about, but will keep
/// transposed duplicates.
pub fn dedup_key(mode: DedupMode, final_pos: &Chess, sans: &[SanPlus]) -> u64 {
    match mode {
        DedupMode::Position => {
            let Zobrist64(h) = final_pos.zobrist_hash::<Zobrist64>(EnPassantMode::Legal);
            h
        }
        DedupMode::Moves => {
            use std::hash::{Hash, Hasher};
            // FNV-1a: fast, deterministic across runs (unlike SipHash's
            // random per-process seed), which matters because this hash is
            // also useful later as a stable join key in opening_training.db.
            let mut hasher = Fnv1a::new();
            for san in sans {
                san.to_string().hash(&mut hasher);
            }
            hasher.finish()
        }
    }
}

struct Fnv1a(u64);

impl Fnv1a {
    fn new() -> Self {
        Fnv1a(0xcbf29ce484222325)
    }
}

impl std::hash::Hasher for Fnv1a {
    fn write(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.0 ^= b as u64;
            self.0 = self.0.wrapping_mul(0x100000001b3);
        }
    }
    fn finish(&self) -> u64 {
        self.0
    }
}
