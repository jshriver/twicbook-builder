mod cli;
mod dedup;
mod filter;
mod render;
mod roster;
mod stats;
mod visitor;

use std::collections::VecDeque;
use std::fs::File;
use std::io::{BufReader, BufWriter, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::mpsc::sync_channel;
use std::sync::{Arc, Mutex};
use std::thread;

use anyhow::{Context, Result};
use clap::Parser;
use dashmap::DashSet;
use pgn_reader::BufferedReader;

use cli::Args;
use dedup::dedup_key;
use filter::RejectReason;
use roster::Roster;
use stats::Stats;
use visitor::{Config, GameOutcome, GameVisitor};

fn main() -> Result<()> {
    let args = Args::parse();
    run(args)
}

fn run(args: Args) -> Result<()> {
    let roster = Roster::load(args.roster.as_deref())?;
    let files = discover_input_files(&args.inputs)?;
    if files.is_empty() {
        anyhow::bail!("no *.pgn / *.pgn.zst files found under the given inputs");
    }
    eprintln!("found {} input file(s)", files.len());

    let jobs = args.jobs.unwrap_or_else(|| {
        thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(4)
    });

    let cfg = Arc::new(Config {
        roster: Arc::new(roster),
        min_elo: args.min_elo,
        max_ply: args.max_ply,
        min_ply: args.min_ply,
        validate_full_game: args.validate_full_game,
    });
    let stats = Arc::new(Stats::default());
    let dedup_set: Arc<DashSet<u64>> = Arc::new(DashSet::new());
    let queue: Arc<Mutex<VecDeque<PathBuf>>> = Arc::new(Mutex::new(files.into_iter().collect()));

    // Bounded channel: workers block on send once the writer falls behind,
    // which caps peak memory instead of buffering unbounded output text
    // for a dataset that may total hundreds of GB.
    let (tx, rx) = sync_channel::<Vec<u8>>(4096);

    let writer_output = args.output.clone();
    let writer_compress = !args.no_compress;
    let writer_level = args.zstd_level;
    let writer_handle = thread::spawn(move || -> Result<()> {
        let file = File::create(&writer_output)
            .with_context(|| format!("creating output file {}", writer_output.display()))?;
        let buffered = BufWriter::with_capacity(1 << 20, file);
        let mut sink: Box<dyn Write> = if writer_compress {
            Box::new(zstd::stream::write::Encoder::new(buffered, writer_level)?.auto_finish())
        } else {
            Box::new(buffered)
        };
        while let Ok(bytes) = rx.recv() {
            sink.write_all(&bytes)?;
        }
        sink.flush()?;
        Ok(())
    });

    let mut worker_handles = Vec::with_capacity(jobs);
    for worker_id in 0..jobs {
        let queue = Arc::clone(&queue);
        let cfg = Arc::clone(&cfg);
        let stats = Arc::clone(&stats);
        let dedup_set = Arc::clone(&dedup_set);
        let tx = tx.clone();
        let dedup_mode = args.dedup_mode;
        let report_interval = args.report_interval;
        worker_handles.push(thread::spawn(move || {
            worker_loop(
                worker_id,
                queue,
                cfg,
                stats,
                dedup_set,
                tx,
                dedup_mode,
                report_interval,
            )
        }));
    }
    drop(tx); // the writer's recv loop ends once every worker's clone is dropped

    for h in worker_handles {
        if let Err(e) = h.join().expect("worker thread panicked") {
            eprintln!("worker error: {e:#}");
        }
    }
    writer_handle.join().expect("writer thread panicked")?;

    stats.report();
    eprintln!("wrote {}", args.output.display());
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn worker_loop(
    _worker_id: usize,
    queue: Arc<Mutex<VecDeque<PathBuf>>>,
    cfg: Arc<Config>,
    stats: Arc<Stats>,
    dedup_set: Arc<DashSet<u64>>,
    tx: std::sync::mpsc::SyncSender<Vec<u8>>,
    dedup_mode: cli::DedupMode,
    report_interval: u64,
) -> Result<()> {
    let mut visitor = GameVisitor::new(cfg.clone());

    loop {
        let path = {
            let mut q = queue.lock().unwrap();
            q.pop_front()
        };
        let Some(path) = path else { break };

        if let Err(e) = process_file(
            &path,
            &mut visitor,
            &stats,
            &dedup_set,
            &tx,
            dedup_mode,
            report_interval,
        ) {
            eprintln!("error processing {}: {e:#}", path.display());
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn process_file(
    path: &Path,
    visitor: &mut GameVisitor,
    stats: &Stats,
    dedup_set: &DashSet<u64>,
    tx: &std::sync::mpsc::SyncSender<Vec<u8>>,
    dedup_mode: cli::DedupMode,
    report_interval: u64,
) -> Result<()> {
    let file = File::open(path).with_context(|| format!("opening {}", path.display()))?;
    let reader: Box<dyn Read> = if is_zst(path) {
        Box::new(zstd::stream::read::Decoder::new(BufReader::new(file))?)
    } else {
        Box::new(BufReader::with_capacity(1 << 20, file))
    };

    let mut pgn_reader = BufferedReader::new(reader);
    loop {
        let outcome = match pgn_reader.read_game(visitor)? {
            Some(o) => o,
            None => break,
        };

        match outcome {
            GameOutcome::Rejected(reason) => stats.record_reject(reason),
            GameOutcome::Accepted(game) => {
                let key = dedup_key(dedup_mode, &game.final_pos, &game.sans);
                if dedup_set.insert(key) {
                    let bytes = render::render_game(&game);
                    stats.record_written();
                    if tx.send(bytes).is_err() {
                        // Writer thread gone (should only happen on a hard
                        // error elsewhere); nothing more we can usefully do.
                        anyhow::bail!("writer channel closed unexpectedly");
                    }
                } else {
                    stats.record_reject(RejectReason::Duplicate);
                }
            }
        }

        let seen = stats.seen.load(std::sync::atomic::Ordering::Relaxed);
        if report_interval != 0 && seen % report_interval == 0 {
            stats.report();
        }
    }
    Ok(())
}

fn is_zst(path: &Path) -> bool {
    path.extension().and_then(|e| e.to_str()) == Some("zst")
}

fn discover_input_files(inputs: &[PathBuf]) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    for input in inputs {
        if input.is_file() {
            files.push(input.clone());
            continue;
        }
        for entry in walkdir::WalkDir::new(input)
            .into_iter()
            .filter_map(|e| e.ok())
        {
            if !entry.file_type().is_file() {
                continue;
            }
            let name = entry.file_name().to_string_lossy();
            if name.ends_with(".pgn") || name.ends_with(".pgn.zst") {
                files.push(entry.into_path());
            }
        }
    }
    Ok(files)
}
