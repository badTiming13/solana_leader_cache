use anyhow::Result;
use base64::{engine::general_purpose, Engine as _};
use std::time::Instant;
use tokio::time::{sleep, Duration};
use tracing::{info, warn};

use crate::cache::LeaderCache;
use crate::rpc::RpcClient;
use crate::tpu_sender::send_udp_tx_multi;
use crate::tx_builder::build_transfer_tx_with_priority;

#[derive(Debug, Clone)]
pub enum SendMode {
    Rpc,
    TpuUdp,
}

#[derive(Debug, Clone)]
pub struct BenchConfig {
    pub iters: usize,
    pub mode: SendMode,
    pub keypair_path: String,
    pub to_pubkey: String,
    pub lamports: u64,

    pub min_commitment: String, // "confirmed"
    pub poll_every: Duration,
    pub timeout: Duration,

    pub leaders_fanout: usize,  // how many next leaders (0 => only current)
    pub resend_every: Duration, // resend interval for TPU

    // NEW: compute budget / priority fee
    pub cu_limit: u32,
    pub cu_price_micro_lamports: u64,
}

#[derive(Debug, Clone)]
pub struct BenchResult {
    pub ok: bool,
    pub slot_delta: Option<i64>,
    pub ms: u128,
    pub send_slot: u64,
    pub landed_slot: Option<u64>,
}

fn percentile(sorted: &[u128], p: f64) -> u128 {
    if sorted.is_empty() {
        return 0;
    }
    let idx = ((sorted.len() as f64 - 1.0) * p).round() as usize;
    sorted[idx]
}

fn percentile_i64(sorted: &[i64], p: f64) -> i64 {
    if sorted.is_empty() {
        return 0;
    }
    let idx = ((sorted.len() as f64 - 1.0) * p).round() as usize;
    sorted[idx]
}

fn is_commitment_ok(status: &Option<String>, min_commitment: &str) -> bool {
    // order: processed < confirmed < finalized
    let rank = |s: &str| match s {
        "processed" => 0,
        "confirmed" => 1,
        "finalized" => 2,
        _ => -1,
    };
    let got = status.as_deref().unwrap_or("processed");
    rank(got) >= rank(min_commitment)
}

/// Build fresh list of TPU addresses for CURRENT + NEXT leaders.
/// We include both tpu and tpuForwards (forward path helps a lot).
async fn current_tpu_addrs(cache: &LeaderCache, fanout: usize) -> Vec<String> {
    let endpoints = cache.leader_endpoints(fanout).await;

    let mut addrs = Vec::new();
    for ep in endpoints {
        if let Some(tpu) = ep.tpu_udp.clone() {
            addrs.push(tpu);
        }
        if let Some(fwd) = ep.forwards_udp.clone() {
            addrs.push(fwd);
        }
    }

    addrs.sort();
    addrs.dedup();
    addrs
}

pub async fn run_bench(rpc: &RpcClient, cache: &LeaderCache, cfg: BenchConfig) -> Result<()> {
    let mut results: Vec<BenchResult> = Vec::with_capacity(cfg.iters);

    info!(
        "bench start: mode={:?} iters={} fanout={} min_commitment={} resend_every={:?} cu_limit={} cu_price={}µLamports/CU",
        cfg.mode,
        cfg.iters,
        cfg.leaders_fanout,
        cfg.min_commitment,
        cfg.resend_every,
        cfg.cu_limit,
        cfg.cu_price_micro_lamports
    );

    for i in 0..cfg.iters {
        // 1) snapshot slot + blockhash
        let send_slot = rpc.get_slot().await?;
        let bh = rpc.get_latest_blockhash().await?;
        let blockhash = bh.value.blockhash.clone();
        let last_valid_bh = bh.value.last_valid_block_height;

        // 2) build tx (WITH priority fee)
        let built = build_transfer_tx_with_priority(
            &cfg.keypair_path,
            &cfg.to_pubkey,
            cfg.lamports,
            &blockhash,
            cfg.cu_limit,
            cfg.cu_price_micro_lamports,
        )?;
        let sig = built.signature.clone();

        let t0 = Instant::now();

        // 3) initial send
        match cfg.mode {
            SendMode::Rpc => {
                let tx_b64 = general_purpose::STANDARD.encode(&built.tx_bytes);
                let _ = rpc.send_transaction_base64(tx_b64).await?;
            }
            SendMode::TpuUdp => {
                let addrs = current_tpu_addrs(cache, cfg.leaders_fanout).await;
                if addrs.is_empty() {
                    warn!(
                        "[{}/{}] no TPU addrs available (tpu/tpuForwards). Skipping.",
                        i + 1,
                        cfg.iters
                    );
                    results.push(BenchResult {
                        ok: false,
                        slot_delta: None,
                        ms: t0.elapsed().as_millis(),
                        send_slot,
                        landed_slot: None,
                    });
                    continue;
                }

                info!(
                    "[{}/{}] TPU addrs (count={}): {:?}",
                    i + 1,
                    cfg.iters,
                    addrs.len(),
                    addrs
                );

                send_udp_tx_multi(&built.tx_bytes, &addrs).await?;
            }
        }

        // 4) wait for inclusion with RESEND that follows leader rotation
        let mut landed_slot: Option<u64> = None;
        let mut ok = false;

        let mut next_resend_at = Instant::now() + cfg.resend_every;

        loop {
            if t0.elapsed() > cfg.timeout {
                break;
            }

            // blockhash expired?
            let bh_now = rpc.get_block_height().await?;
            if bh_now > last_valid_bh {
                break;
            }

            // status?
            let st = rpc.get_signature_status(&sig).await?;
            if let Some(st) = st {
                if st.err.is_none() && is_commitment_ok(&st.confirmation_status, &cfg.min_commitment) {
                    landed_slot = Some(st.slot);
                    ok = true;
                    break;
                }
            }

            // resend (TPU mode only) — IMPORTANT: recalc addrs each time
            if matches!(cfg.mode, SendMode::TpuUdp) && Instant::now() >= next_resend_at {
                let addrs = current_tpu_addrs(cache, cfg.leaders_fanout).await;
                if !addrs.is_empty() {
                    let _ = send_udp_tx_multi(&built.tx_bytes, &addrs).await;
                }
                next_resend_at = Instant::now() + cfg.resend_every;
            }

            sleep(cfg.poll_every).await;
        }

        let ms = t0.elapsed().as_millis();
        let slot_delta = landed_slot.map(|ls| ls as i64 - send_slot as i64);

        info!(
            "[{}/{}] ok={} sig={} send_slot={} landed_slot={:?} slot_delta={:?} ms={}",
            i + 1,
            cfg.iters,
            ok,
            sig,
            send_slot,
            landed_slot,
            slot_delta,
            ms
        );

        results.push(BenchResult {
            ok,
            slot_delta,
            ms,
            send_slot,
            landed_slot,
        });

        // small pause
        sleep(Duration::from_millis(120)).await;
    }

    // --- summary
    let total = results.len();
    let ok_count = results.iter().filter(|r| r.ok).count();
    let fail_count = total - ok_count;

    let mut ms_ok: Vec<u128> = results.iter().filter(|r| r.ok).map(|r| r.ms).collect();
    ms_ok.sort_unstable();

    let mut slots_ok: Vec<i64> = results
        .iter()
        .filter(|r| r.ok)
        .filter_map(|r| r.slot_delta)
        .collect();
    slots_ok.sort_unstable();

    info!("--- summary ---");
    info!(
        "total={} ok={} fail={} success_rate={:.2}%",
        total,
        ok_count,
        fail_count,
        (ok_count as f64) * 100.0 / (total as f64)
    );

    if !ms_ok.is_empty() {
        info!(
            "latency_ms median={} p90={} p95={}",
            percentile(&ms_ok, 0.50),
            percentile(&ms_ok, 0.90),
            percentile(&ms_ok, 0.95),
        );
    }
    if !slots_ok.is_empty() {
        info!(
            "slot_delta median={} p90={} p95={}",
            percentile_i64(&slots_ok, 0.50),
            percentile_i64(&slots_ok, 0.90),
            percentile_i64(&slots_ok, 0.95),
        );
    }

    Ok(())
}
