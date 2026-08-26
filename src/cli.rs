use std::path::PathBuf;

use clap::Parser;

#[derive(Clone, Copy, Debug, PartialEq, Eq, clap::ValueEnum)]
pub enum DedupMode {
    /// Zobrist hash of the resulting position after `max_ply` (or end of
    /// game, if shorter). Transpositions collapse into one game. This is
    /// the recommended mode for opening-training corpora.
    Position,
    /// Hash of the literal move sequence (start position + SAN list).
    /// Move-order-sensitive: `1.Nf3 d5 2.d4` and `1.d4 d5 2.Nf3` are *not*
    /// considered duplicates in this mode.
    Moves,
}

/// Streaming, single-pass PGN cleaner for building an opening-focused
/// "gold" training dataset from very large chess databases.
///
/// Combines header-only filtering (Elo / variant / non-standard start),
/// legal-move validation, ply truncation, and transposition-aware
/// deduplication in one pass, so multi-hundred-million-game inputs never
/// need a second full read.
#[derive(Parser, Debug)]
#[command(name = "twicbook-builder", version, about)]
pub struct Args {
    /// Input files or directories (searched recursively for *.pgn / *.pgn.zst).
    #[arg(required = true)]
    pub inputs: Vec<PathBuf>,

    /// Output path. Written as PGN, zstd-compressed unless --no-compress is set.
    #[arg(short, long, default_value = "clean.pgn.zst")]
    pub output: PathBuf,

    /// Minimum Elo required for BOTH players. Games missing either Elo tag
    /// are always rejected regardless of this value (no historical-Elo
    /// reconstruction is attempted).
    #[arg(long, default_value_t = 2400)]
    pub min_elo: i32,

    /// Keep only the first N ply (half-moves). 24 ply = 12 full moves.
    #[arg(long, default_value_t = 24)]
    pub max_ply: usize,

    /// Discard games shorter than this many ply once truncated.
    #[arg(long, default_value_t = 1)]
    pub min_ply: usize,

    /// Deduplication strategy.
    #[arg(long, value_enum, default_value_t = DedupMode::Position)]
    pub dedup_mode: DedupMode,

    /// Continue applying and legal-checking moves past `max_ply` (slower,
    /// catches corruption deeper in the game). Off by default because the
    /// output only ever contains the first `max_ply` moves anyway.
    #[arg(long, default_value_t = false)]
    pub validate_full_game: bool,

    /// Path to a text file listing output PGN tags, one per line, in the
    /// desired output order (this replaces pgn-extract's -R/--xroster).
    /// If omitted, a sensible default roster is used.
    #[arg(long)]
    pub roster: Option<PathBuf>,

    /// Number of worker threads reading/filtering games in parallel.
    /// Defaults to the number of logical CPUs.
    #[arg(short = 'j', long)]
    pub jobs: Option<usize>,

    /// Disable zstd compression of the output file.
    #[arg(long, default_value_t = false)]
    pub no_compress: bool,

    /// zstd compression level (1-22). Higher = smaller, slower.
    #[arg(long, default_value_t = 19)]
    pub zstd_level: i32,

    /// Print progress stats every N games processed per worker.
    #[arg(long, default_value_t = 500_000)]
    pub report_interval: u64,
}

pub const DEFAULT_ROSTER: &[&str] = &[
    "Event", "Site", "Date", "Round", "White", "Black", "WhiteElo", "BlackElo", "Result", "ECO",
];
