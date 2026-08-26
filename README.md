# twicbook-builder

A single-pass, multi-threaded Rust tool for turning a huge, messy collection
of PGN databases (TWIC, Lichess, Caissa, FIDE archives...) into a clean, opening-focused "gold" pgn.

## Why this replaces the pgn-extract two-stage plan

This tool uses [`pgn-reader`](https://crates.io/crates/pgn-reader) (the same
streaming, visitor-based PGN parser lichess itself uses) paired with
[`shakmaty`](https://crates.io/crates/shakmaty) for legal-move validation
and Zobrist hashing. Because `pgn-reader`'s `Visitor` can inspect every
header tag and then tell the reader to **skip straight past the movetext**
before any move parsing happens, header filtering (Elo / variant /
non-standard start) and move-level work (legality, truncation, dedup) live
in one pass:

```
for each game in file:
    header()      -> capture only the tags we actually need
    end_headers()  -> Elo / variant / SetUp check -> Skip(true) if rejected
                       (movetext is never touched for rejected games)
    san()         -> apply + validate moves via shakmaty, up to max_ply
    end_game()    -> ply-count corruption check, dedup, render, write
```

Since the ≥2400 Elo floor alone typically eliminates the large majority of
a TWIC-era dump, most games never get their movetext parsed at
all — the single biggest cost saving available.

## Build

```sh
cargo build --release
```

Tested against `pgn-reader 0.26` / `shakmaty 0.27` / `clap 4.3` /
`dashmap 6.2` / `zstd 0.13`, which is what `Cargo.toml` pins. These are a
few point releases behind the latest (as of writing, `pgn-reader` is at
0.29 / `shakmaty` at 0.30), and you're welcome to bump them — just note
that `shakmaty` 0.29 moved `zobrist_hash()` from a separate `ZobristHash`
trait onto `Position` directly, so `dedup.rs`'s import line is the one
thing to adjust if you do.

## Usage

```sh
./target/release/twicbook-builder \
    /path/to/huge.pgn \
    /path/to/twic_dir \
    /path/to/lichess \
    -o cleaned.pgn.zst \
    --min-elo 2400 \
    --max-ply 24 \
    --dedup-mode position \
    --roster tagorder.txt \
    --jobs 16
```

Inputs can be individual `.pgn` / `.pgn.zst` files or directories (searched
recursively). Both file arguments and directories can be mixed freely.

### Key flags

| Flag | Default | Meaning |
|---|---|---|
| `--min-elo` | 2400 | Both White and Black must meet this. Games missing either Elo tag are always rejected — no historical Elo reconstruction is attempted, per spec. |
| `--max-ply` | 24 | Truncate every kept game to its first N ply (24 = 12 full moves). Unlike `pgn-extract --maxply`, this *does* truncate longer games rather than only filtering games already under the limit. |
| `--min-ply` | 1 | Discard degenerate near-empty games after truncation. |
| `--dedup-mode` | `position` | `position` = Zobrist hash of the resulting board after truncation (transpositions collapse into one entry — `1.Nf3 d5 2.d4` and `1.d4 d5 2.Nf3` are treated as duplicates). `moves` = hash of the literal SAN sequence (order-sensitive, keeps transpositions separate). |
| `--roster` | (built-in) | Path to a tagorder file — one PGN tag name per line, in output order. This *is* pgn-extract's `--xroster` + `-R` combined into one option: tags not listed are dropped, tags listed are kept and ordered exactly as given. |
| `--validate-full-game` | off | Also legality-check moves past `max_ply` (slower; only useful if you care about corruption deep in games you're truncating away anyway). |
| `--jobs` | # logical CPUs | Worker threads, each owning its own input file at a time. |
| `--zstd-level` | 19 | Passed straight to zstd. |
| `--no-compress` | off | Write plain `.pgn` instead. |

Run `--help` for the full list.

## What gets rejected, and why (stats reported at the end)

- **missing Elo** — either `WhiteElo`/`BlackElo` tag absent, or a
  placeholder value (`?`, `-`, `0`, empty string) —  historical
  games like the Lasker/McBride example from the spec land here.
- **below Elo floor** — both tags present but below `--min-elo`.
- **non-standard variant** — `[Variant "..."]` present and not
  "Standard"/"Normal" (Chess960, Crazyhouse, Atomic, Horde, Antichess, etc).
- **non-standard start** — `[SetUp "1"]` present (custom `FEN`).
- **illegal / corrupt** — a syntactically valid SAN token that shakmaty's
  legal-move generator rejects (wrong piece, blocked path, moving into
  check, etc). Genuinely caught at the semantic level.
- **ply-count mismatch** — see **Known limitation** below.
- **too short** — zero moves, or shorter than `--min-ply` after truncation.
- **duplicate** — the dedup key (see `--dedup-mode`) was already seen.

## Known limitation: malformed move tokens

`pgn-reader` is intentionally permissive: if a move token isn't even
syntactically valid SAN (e.g. an off-board square like `Nf9`, or binary
garbage from an encoding problem), it is **silently dropped** rather than
raising a parse error — the visitor's `san()` callback is just never
invoked for that token, and the game continues as if it had one fewer
move. This is a deliberate tradeoff in the library (built for lichess,
where "keep going" matters more than "catch every garbled token") — but it
means a corrupted mid-game token, on its own, won't trip the "illegal
move" legality check, since that check only ever sees tokens the tokenizer
successfully parsed in the first place.

**Mitigation implemented here:** when a game finishes *before* `--max-ply`
(i.e. we saw its entire mainline) and it has a `[PlyCount "N"]` tag —
which TWIC games reliably do — the tool cross-checks the
declared ply count against the number of moves it actually parsed. A
mismatch means a token was dropped, and the game is rejected as
`PlyCountMismatch`. `PlyCount` is captured during header scanning purely
for this check; it is never written to the output roster.

This closes the gap for the common case (short games, or games where the
corruption happens before the truncation point) but not for a corrupted
token occurring in the untouched tail of a long game that gets truncated
anyway — since we don't parse past `max_ply` there by default. Pass
`--validate-full-game` if you want the ply-count check to cover those too
(at the cost of legality-checking moves you're going to discard).

