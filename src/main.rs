mod bench;
mod cache;
mod rpc;
mod tpu_sender;
mod tx_builder;
mod types;

use anyhow::{anyhow, Result};
use bench::{run_bench, BenchConfig, SendMode};
use cache::{CacheConfig, LeaderCache};
use clap::{Parser, ValueEnum};
use rpc::RpcClient;
use std::{env, time::Duration};
use tracing::info;

#[derive(Clone, Debug, ValueEnum)]
enum ModeArg {
    Rpc,
    TpuUdp,
}

#[derive(Parser, Debug)]
#[command(name = "solana_leader_cache_bench")]
struct Args {
    #[arg(long, default_value = "https://api.mainnet-beta.solana.com")]
    rpc_url: String,

    /// If not provided, we try KEYPAIR_PATH env var.
    #[arg(long)]
    keypair: Option<String>,

    /// Recipient pubkey (must be different from payer).
    #[arg(long)]
    to: String,

    /// How many lamports to transfer each iteration (default 1).
    #[arg(long, default_value_t = 1)]
    lamports: u64,

    #[arg(long, value_enum, default_value = "tpu-udp")]
    mode: ModeArg,

    #[arg(long, default_value_t = 50)]
    iters: usize,

    /// How many NEXT leaders to also send to (0 = only current leader)
    #[arg(long, default_value_t = 2)]
    fanout: usize,

    #[arg(long, default_value = "confirmed")]
    commitment: String,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter("info")
        .init();

    let args = Args::parse();

    let keypair = match args.keypair {
        Some(p) => p,
        None => env::var("KEYPAIR_PATH")
            .map_err(|_| anyhow!("keypair not provided. Use --keypair <path> or set KEYPAIR_PATH env var"))?,
    };

    info!(
        "rpc_url={} mode={:?} iters={} fanout={} commitment={} to={} lamports={}",
        args.rpc_url, args.mode, args.iters, args.fanout, args.commitment, args.to, args.lamports
    );

    let rpc = RpcClient::new(args.rpc_url.clone());
    let cache = LeaderCache::new(rpc.clone());

    let cache_cfg = CacheConfig {
        leaders_window: 200,
        refresh_leaders_every: Duration::from_secs(2),
        refresh_nodes_every: Duration::from_secs(45),
    };

    cache.warmup(&cache_cfg).await?;
    cache.clone().spawn_background_tasks(cache_cfg);

    let mode = match args.mode {
        ModeArg::Rpc => SendMode::Rpc,
        ModeArg::TpuUdp => SendMode::TpuUdp,
    };

    let bench_cfg = BenchConfig {
        iters: args.iters,
        mode,
        keypair_path: keypair,
        to_pubkey: args.to,
        lamports: args.lamports,

        min_commitment: args.commitment,
        poll_every: Duration::from_millis(200),
        timeout: Duration::from_secs(20),
        leaders_fanout: args.fanout,

        // ВАЖНО: resend для UDP
        resend_every: Duration::from_millis(400),
    };

    run_bench(&rpc, &cache, bench_cfg).await?;
    Ok(())
}
