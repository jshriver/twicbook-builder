use std::collections::HashMap;
use std::sync::Arc;

use pgn_reader::{RawHeader, SanPlus, Skip, Visitor};
use shakmaty::{Chess, Position};

use crate::filter::{is_custom_start, is_standard_variant, parse_elo, RejectReason};
use crate::roster::Roster;

pub struct AcceptedGame {
    /// Tags present in the output, already in roster order.
    pub tags: Vec<(String, String)>,
    pub sans: Vec<SanPlus>,
    pub final_pos: Chess,
}

pub enum GameOutcome {
    Accepted(AcceptedGame),
    Rejected(RejectReason),
}

pub struct Config {
    pub roster: Arc<Roster>,
    pub min_elo: i32,
    pub max_ply: usize,
    pub min_ply: usize,
    pub validate_full_game: bool,
}

/// A single reusable Visitor instance per worker thread. `begin_game` resets
/// all per-game state, so the same allocations (header map, move buffer)
/// are recycled across the whole file instead of reallocating per game --
/// this matters a lot at hundreds-of-millions-of-games scale.
pub struct GameVisitor {
    cfg: Arc<Config>,
    headers: HashMap<Vec<u8>, Vec<u8>>,
    pos: Chess,
    sans: Vec<SanPlus>,
    ply: usize,
    reject: Option<RejectReason>,
    illegal: bool,
}

impl GameVisitor {
    pub fn new(cfg: Arc<Config>) -> Self {
        GameVisitor {
            cfg,
            headers: HashMap::new(),
            pos: Chess::default(),
            sans: Vec::new(),
            ply: 0,
            reject: None,
            illegal: false,
        }
    }
}

impl Visitor for GameVisitor {
    type Result = GameOutcome;

    fn begin_game(&mut self) {
        self.headers.clear();
        self.pos = Chess::default();
        self.sans.clear();
        self.ply = 0;
        self.reject = None;
        self.illegal = false;
    }

    fn header(&mut self, key: &[u8], value: RawHeader<'_>) {
        if self.cfg.roster.wants(key) {
            self.headers.insert(key.to_vec(), value.decode().into_owned());
        }
    }

    fn end_headers(&mut self) -> Skip {
        // Cheap checks first: variant / custom start-position tags, which
        // are just a handful of byte comparisons.
        if let Some(variant) = self.headers.get(b"Variant".as_slice()) {
            if !is_standard_variant(variant) {
                self.reject = Some(RejectReason::NonStandardVariant);
                return Skip(true);
            }
        }
        if is_custom_start(self.headers.get(b"SetUp".as_slice()).map(|v| v.as_slice())) {
            self.reject = Some(RejectReason::NonStandardStart);
            return Skip(true);
        }

        let white_elo = self.headers.get(b"WhiteElo".as_slice()).and_then(|v| parse_elo(v));
        let black_elo = self.headers.get(b"BlackElo".as_slice()).and_then(|v| parse_elo(v));

        match (white_elo, black_elo) {
            (Some(w), Some(b)) => {
                if w < self.cfg.min_elo || b < self.cfg.min_elo {
                    self.reject = Some(RejectReason::LowElo);
                }
            }
            _ => {
                self.reject = Some(RejectReason::MissingElo);
            }
        }

        // If already rejected on headers alone, tell pgn-reader to skip
        // straight past the movetext to end_game -- this is the whole
        // point of doing header filtering and move parsing in one pass
        // instead of two: rejected games (the overwhelming majority once
        // the 2400 Elo floor is applied) cost almost nothing beyond the
        // header lines themselves.
        Skip(self.reject.is_some())
    }

    fn san(&mut self, san_plus: SanPlus) {
        if self.reject.is_some() || self.illegal {
            return;
        }

        if self.ply < self.cfg.max_ply || self.cfg.validate_full_game {
            match san_plus.san.to_move(&self.pos) {
                Ok(mv) => match self.pos.clone().play(&mv) {
                    Ok(new_pos) => self.pos = new_pos,
                    Err(_) => {
                        self.illegal = true;
                        return;
                    }
                },
                Err(_) => {
                    self.illegal = true;
                    return;
                }
            }
            if self.ply < self.cfg.max_ply {
                self.sans.push(san_plus);
            }
        }

        self.ply += 1;
    }

    fn begin_variation(&mut self) -> Skip {
        // Never descend into side lines: only the mainline matters for an
        // opening-training dataset, and skipping keeps the reader fast even
        // on heavily annotated ChessBase games.
        Skip(true)
    }

    fn end_game(&mut self) -> GameOutcome {
        if let Some(reason) = self.reject {
            return GameOutcome::Rejected(reason);
        }
        if self.illegal {
            return GameOutcome::Rejected(RejectReason::IllegalMove);
        }
        if self.ply < self.cfg.min_ply || self.sans.is_empty() {
            return GameOutcome::Rejected(RejectReason::TooShort);
        }

        // Corruption cross-check: only meaningful when the game ended
        // before max_ply, i.e. we actually parsed its entire mainline. If
        // the declared PlyCount disagrees, a move token was very likely
        // dropped by the tokenizer (see RejectReason::PlyCountMismatch).
        if self.ply < self.cfg.max_ply {
            if let Some(declared) = self
                .headers
                .get(b"PlyCount".as_slice())
                .and_then(|v| std::str::from_utf8(v).ok())
                .and_then(|s| s.trim().parse::<usize>().ok())
            {
                if declared != self.ply {
                    return GameOutcome::Rejected(RejectReason::PlyCountMismatch);
                }
            }
        }

        let tags = self
            .cfg
            .roster
            .order
            .iter()
            .filter_map(|tag| {
                self.headers
                    .get(tag.as_bytes())
                    .map(|v| (tag.clone(), String::from_utf8_lossy(v).into_owned()))
            })
            .collect();

        GameOutcome::Accepted(AcceptedGame {
            tags,
            sans: std::mem::take(&mut self.sans),
            final_pos: self.pos.clone(),
        })
    }
}
