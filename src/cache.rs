use anyhow::Result;
use dashmap::DashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tokio::time::{Duration, Instant};
use tracing::{info, warn};

use crate::rpc::RpcClient;
use crate::types::{NodeEndpoints, NodeMap};

#[derive(Clone)]
pub struct LeaderCache {
    rpc: RpcClient,

    // pubkey -> endpoints
    nodes: Arc<DashMap<String, NodeEndpoints>>,

    // leaders window (ordered)
    leaders: Arc<RwLock<Vec<String>>>,

    // latest start slot for leaders window
    leaders_start_slot: Arc<RwLock<u64>>,
}

#[derive(Debug, Clone)]
pub struct CacheConfig {
    pub leaders_window: u64,          // how many slots ahead we store (e.g. 200)
    pub refresh_leaders_every: Duration, // e.g. 1s-2s
    pub refresh_nodes_every: Duration,   // e.g. 30s-60s
}

impl Default for CacheConfig {
    fn default() -> Self {
        Self {
            leaders_window: 200,
            refresh_leaders_every: Duration::from_secs(2),
            refresh_nodes_every: Duration::from_secs(45),
        }
    }
}

impl LeaderCache {
    pub fn new(rpc: RpcClient) -> Self {
        Self {
            rpc,
            nodes: Arc::new(DashMap::new()),
            leaders: Arc::new(RwLock::new(Vec::new())),
            leaders_start_slot: Arc::new(RwLock::new(0)),
        }
    }

    pub async fn warmup(&self, cfg: &CacheConfig) -> Result<()> {
        self.refresh_nodes().await?;
        self.refresh_leaders(cfg.leaders_window).await?;
        Ok(())
    }

    pub fn spawn_background_tasks(self, cfg: CacheConfig) {
        let a = self.clone();
        tokio::spawn(async move {
            loop {
                let t0 = Instant::now();
                if let Err(e) = a.refresh_nodes().await {
                    warn!("refresh_nodes failed: {e}");
                } else {
                    info!("nodes refreshed in {:?}", t0.elapsed());
                }
                tokio::time::sleep(cfg.refresh_nodes_every).await;
            }
        });

        let b = self.clone();
        tokio::spawn(async move {
            loop {
                let t0 = Instant::now();
                if let Err(e) = b.refresh_leaders(cfg.leaders_window).await {
                    warn!("refresh_leaders failed: {e}");
                } else {
                    info!("leaders refreshed in {:?}", t0.elapsed());
                }
                tokio::time::sleep(cfg.refresh_leaders_every).await;
            }
        });
    }

    async fn refresh_nodes(&self) -> Result<()> {
        let nodes = self.rpc.get_cluster_nodes().await?;
        self.nodes.clear();
        for n in nodes {
            let ep: NodeEndpoints = n.into();
            self.nodes.insert(ep.pubkey.clone(), ep);
        }
        Ok(())
    }

    async fn refresh_leaders(&self, window: u64) -> Result<()> {
        let slot = self.rpc.get_slot().await?;
        let leaders = self.rpc.get_slot_leaders(slot, window).await?;

        {
            let mut ls = self.leaders.write().await;
            *ls = leaders;
        }
        {
            let mut ss = self.leaders_start_slot.write().await;
            *ss = slot;
        }
        Ok(())
    }

    /// Возвращает pubkey лидера на offset слотов от start_slot:
    /// offset=0 => текущий лидер (по данным кеша)
    pub async fn leader_pubkey_at_offset(&self, offset: usize) -> Option<String> {
        let ls = self.leaders.read().await;
        ls.get(offset).cloned()
    }

    /// Возвращает endpoints для лидеров: current + next_count
    pub async fn leader_endpoints(&self, next_count: usize) -> Vec<NodeEndpoints> {
        let ls = self.leaders.read().await;
        let mut out = Vec::new();

        for pk in ls.iter().take(next_count + 1) {
            if let Some(v) = self.nodes.get(pk) {
                out.push(v.value().clone());
            }
        }
        out
    }

    /// Для дебага: какой слот является "началом" нашего окна лидеров
    pub async fn leaders_start_slot(&self) -> u64 {
        *self.leaders_start_slot.read().await
    }
}
