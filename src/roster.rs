use std::collections::HashSet;
use std::path::Path;

use anyhow::{Context, Result};

use crate::cli::DEFAULT_ROSTER;

/// Header tags that must be inspected for *filtering* purposes even if they
/// are not part of the output roster (e.g. Variant/SetUp/FEN are never
/// written out, but we must see them to reject non-standard games).
const FILTER_ONLY_TAGS: &[&str] = &["Variant", "SetUp", "FEN", "WhiteElo", "BlackElo", "PlyCount"];

#[derive(Debug, Clone)]
pub struct Roster {
    /// Output tag order, exactly as it should appear in the written PGN.
    pub order: Vec<String>,
    /// Union of `order` and FILTER_ONLY_TAGS: every header key worth
    /// capturing while scanning `[Tag "value"]` lines.
    pub needed: HashSet<Vec<u8>>,
}

impl Roster {
    pub fn load(path: Option<&Path>) -> Result<Roster> {
        let order: Vec<String> = match path {
            Some(p) => {
                let text = std::fs::read_to_string(p)
                    .with_context(|| format!("reading roster file {}", p.display()))?;
                text.lines()
                    .map(str::trim)
                    .filter(|l| !l.is_empty() && !l.starts_with('#'))
                    .map(str::to_string)
                    .collect()
            }
            None => DEFAULT_ROSTER.iter().map(|s| s.to_string()).collect(),
        };

        if order.is_empty() {
            anyhow::bail!("roster is empty; need at least one output tag");
        }

        let mut needed: HashSet<Vec<u8>> = order.iter().map(|s| s.as_bytes().to_vec()).collect();
        for tag in FILTER_ONLY_TAGS {
            needed.insert(tag.as_bytes().to_vec());
        }

        Ok(Roster { order, needed })
    }

    #[inline]
    pub fn wants(&self, key: &[u8]) -> bool {
        self.needed.contains(key)
    }
}
