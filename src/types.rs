use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Deserialize)]
pub struct ClusterNode {
    pub pubkey: String,
    pub gossip: Option<String>,
    pub rpc: Option<String>,
    pub pubsub: Option<String>,

    pub tpu: Option<String>,
    #[serde(rename = "tpuQuic")]
    pub tpu_quic: Option<String>,

    #[serde(rename = "tpuForwards")]
    pub tpu_forwards: Option<String>,
    #[serde(rename = "tpuForwardsQuic")]
    pub tpu_forwards_quic: Option<String>,

    pub version: Option<String>,
    #[serde(rename = "featureSet")]
    pub feature_set: Option<u64>,
    #[serde(rename = "shredVersion")]
    pub shred_version: Option<u16>,
}

#[derive(Debug, Clone)]
pub struct NodeEndpoints {
    pub pubkey: String,
    pub tpu_udp: Option<String>,
    pub tpu_quic: Option<String>,
    pub forwards_udp: Option<String>,
    pub forwards_quic: Option<String>,
}

impl From<ClusterNode> for NodeEndpoints {
    fn from(n: ClusterNode) -> Self {
        Self {
            pubkey: n.pubkey,
            tpu_udp: n.tpu,
            tpu_quic: n.tpu_quic,
            forwards_udp: n.tpu_forwards,
            forwards_quic: n.tpu_forwards_quic,
        }
    }
}

pub type NodeMap = HashMap<String, NodeEndpoints>;

#[derive(Debug, Serialize)]
pub struct JsonRpcRequest<'a, T> {
    pub jsonrpc: &'static str,
    pub id: u32,
    pub method: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<T>,
}

#[derive(Debug, Deserialize)]
pub struct JsonRpcResponse<T> {
    pub jsonrpc: String,
    pub id: u32,
    pub result: T,
}

#[derive(Debug, Deserialize)]
pub struct SlotLeadersResult(pub Vec<String>);


#[derive(Debug, Deserialize)]
pub struct LatestBlockhashResp {
    pub value: LatestBlockhashValue,
}

#[derive(Debug, Deserialize)]
pub struct LatestBlockhashValue {
    pub blockhash: String,
    #[serde(rename = "lastValidBlockHeight")]
    pub last_valid_block_height: u64,
}

#[derive(Debug, Deserialize)]
pub struct SignatureStatusesResp {
    pub value: Vec<Option<SignatureStatus>>,
}

#[derive(Debug, Deserialize)]
pub struct SignatureStatus {
    pub slot: u64,
    pub err: Option<serde_json::Value>,
    #[serde(rename = "confirmationStatus")]
    pub confirmation_status: Option<String>, // "processed" | "confirmed" | "finalized"
}

#[derive(Debug, Serialize)]
pub struct SendTxConfig {
    pub encoding: String, // "base64"
    #[serde(rename = "skipPreflight")]
    pub skip_preflight: bool,
    #[serde(rename = "preflightCommitment")]
    pub preflight_commitment: String, // "processed" etc
    #[serde(rename = "maxRetries")]
    pub max_retries: Option<u64>,
}