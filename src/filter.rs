/// Reasons a game can be rejected purely from its header tags, before any
/// movetext is parsed. Kept as an enum (rather than a bool) so stats can
/// report *why* the data shrank, which matters when tuning the pipeline
/// against a new source (ChessBase vs TWIC vs Lichess all fail differently).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RejectReason {
    MissingElo,
    LowElo,
    NonStandardVariant,
    NonStandardStart,
    IllegalMove,
    /// The game's declared `[PlyCount "N"]` disagrees with the number of
    /// moves we actually parsed, for a game that finished before
    /// `max_ply`. This catches a real gap in pgn-reader's tokenizer: a
    /// single malformed move token (e.g. an off-board square) is silently
    /// *dropped* rather than raising a parse error, so a corrupted game can
    /// otherwise sail through legality checking with a truncated mainline.
    /// PlyCount is intentionally still captured during header parsing for
    /// this check even though it is dropped from the output roster.
    PlyCountMismatch,
    TooShort,
    Duplicate,
}

/// Parses a PGN Elo tag value. ChessBase/TWIC sometimes use "?", "0", or an
/// empty string for unrated/unknown players -- all of those are treated as
/// "no Elo", never as Elo 0, so they are rejected rather than silently
/// passing a `>= min_elo` check against a nonsense value.
pub fn parse_elo(raw: &[u8]) -> Option<i32> {
    let s = std::str::from_utf8(raw).ok()?.trim();
    if s.is_empty() || s == "?" || s == "-" {
        return None;
    }
    let v: i32 = s.parse().ok()?;
    if v <= 0 {
        None
    } else {
        Some(v)
    }
}

/// Returns true if a `[Variant "..."]` value counts as standard chess.
/// Absence of the tag also counts as standard (the vast majority of
/// standard-chess games never emit this tag at all).
pub fn is_standard_variant(raw: &[u8]) -> bool {
    match std::str::from_utf8(raw) {
        Ok(s) => {
            let s = s.trim();
            s.is_empty() || s.eq_ignore_ascii_case("standard") || s.eq_ignore_ascii_case("normal")
        }
        Err(_) => false,
    }
}

/// Returns true if `[SetUp "1"]` is present, i.e. the game does not start
/// from the normal initial position.
pub fn is_custom_start(setup_value: Option<&[u8]>) -> bool {
    matches!(setup_value, Some(v) if trim_bytes(v) == b"1")
}

fn trim_bytes(s: &[u8]) -> &[u8] {
    let start = s
        .iter()
        .position(|b| !b.is_ascii_whitespace())
        .unwrap_or(s.len());
    let end = s
        .iter()
        .rposition(|b| !b.is_ascii_whitespace())
        .map_or(start, |p| p + 1);
    &s[start..end]
}
