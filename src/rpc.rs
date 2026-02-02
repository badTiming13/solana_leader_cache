use anyhow::{Result, anyhow};
use reqwest::Client;
use serde::de::DeserializeOwned;
use crate::types::{LatestBlockhashResp, SendTxConfig, SignatureStatusesResp, SignatureStatus};

use crate::types::{JsonRpcRequest, JsonRpcResponse};

#[derive(Clone)]
pub struct RpcClient {
    http: Client,
    url: String,
}

impl RpcClient {
    pub fn new(url: impl Into<String>) -> Self {
        Self {
            http: Client::new(),
            url: url.into(),
        }
    }

    pub async fn get_latest_blockhash(&self) -> anyhow::Result<LatestBlockhashResp> {
        self.call::<LatestBlockhashResp, ()>("getLatestBlockhash", None::<()>)
            .await
    }

    pub async fn get_block_height(&self) -> anyhow::Result<u64> {
        self.call::<u64, ()>("getBlockHeight", None::<()>).await
    }

    pub async fn send_transaction_base64(&self, tx_b64: String) -> anyhow::Result<String> {
        let cfg = SendTxConfig {
            encoding: "base64".to_string(),
            skip_preflight: true,
            preflight_commitment: "processed".to_string(),
            max_retries: None,
        };

        // sendTransaction expects [tx, config]
        self.call::<String, (String, SendTxConfig)>("sendTransaction", Some((tx_b64, cfg)))
            .await
    }

    pub async fn get_signature_status(
        &self,
        sig: &str,
    ) -> anyhow::Result<Option<crate::types::SignatureStatus>> {
        // getSignatureStatuses expects [[sig], {searchTransactionHistory:false}]
        #[derive(serde::Serialize)]
        struct Cfg {
            #[serde(rename = "searchTransactionHistory")]
            search_transaction_history: bool,
        }

        let resp = self
            .call::<SignatureStatusesResp, (Vec<String>, Cfg)>(
                "getSignatureStatuses",
                Some((
                    vec![sig.to_string()],
                    Cfg {
                        search_transaction_history: false,
                    },
                )),
            )
            .await?;

        Ok(resp.value.into_iter().next().flatten())
    }

    pub async fn call<R, P>(&self, method: &str, params: Option<P>) -> Result<R>
    where
        R: DeserializeOwned,
        P: serde::Serialize,
    {
        let req = JsonRpcRequest {
            jsonrpc: "2.0",
            id: 1,
            method,
            params,
        };

        let resp = self
            .http
            .post(&self.url)
            .json(&req)
            .send()
            .await
            .map_err(|e| anyhow!("rpc send error: {e}"))?;

        if !resp.status().is_success() {
            return Err(anyhow!("rpc http status: {}", resp.status()));
        }

        let body: serde_json::Value = resp.json().await?;
        // Solana иногда отдаёт error вместо result
        if body.get("error").is_some() {
            return Err(anyhow!("rpc error: {}", body));
        }

        let parsed: JsonRpcResponse<R> = serde_json::from_value(body)?;
        Ok(parsed.result)
    }

    pub async fn get_slot(&self) -> Result<u64> {
        self.call::<u64, ()>("getSlot", None::<()>).await
    }

    pub async fn get_slot_leaders(&self, start_slot: u64, limit: u64) -> Result<Vec<String>> {
        self.call::<Vec<String>, (u64, u64)>("getSlotLeaders", Some((start_slot, limit)))
            .await
    }

    pub async fn get_cluster_nodes(&self) -> Result<Vec<crate::types::ClusterNode>> {
        self.call::<Vec<crate::types::ClusterNode>, ()>("getClusterNodes", None::<()>)
            .await
    }
}
