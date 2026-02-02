use anyhow::Result;
use solana_sdk::{
    hash::Hash,
    instruction::{AccountMeta, Instruction},
    message::Message,
    pubkey::Pubkey,
    signature::{read_keypair_file, Keypair, Signer},
    transaction::Transaction,
};
use std::str::FromStr;

pub struct BuiltTx {
    pub tx_bytes: Vec<u8>,
    pub signature: String,
}

#[derive(serde::Serialize)]
enum ComputeBudgetIx {
    RequestHeapFrame(u32),
    RequestUnitsDeprecated { units: u32, additional_fee: u32 },
    SetComputeUnitLimit(u32),
    SetComputeUnitPrice(u64),
    SetLoadedAccountsDataSizeLimit(u32),
}

fn compute_budget_program_id() -> Pubkey {
    // Compute Budget Program
    Pubkey::from_str("ComputeBudget111111111111111111111111111111").expect("compute budget program id must be valid")
}

fn compute_budget_ixs(cu_limit: u32, cu_price_micro_lamports: u64) -> [Instruction; 2] {
    let program_id = compute_budget_program_id();

    let data_limit = bincode::serialize(&ComputeBudgetIx::SetComputeUnitLimit(cu_limit))
        .expect("bincode serialize ComputeBudgetIx::SetComputeUnitLimit");
    let data_price = bincode::serialize(&ComputeBudgetIx::SetComputeUnitPrice(cu_price_micro_lamports))
        .expect("bincode serialize ComputeBudgetIx::SetComputeUnitPrice");

    [
        Instruction {
            program_id,
            accounts: vec![],
            data: data_limit,
        },
        Instruction {
            program_id,
            accounts: vec![],
            data: data_price,
        },
    ]
}

/// Build System Program Transfer instruction data:
/// layout = [u32_le discriminator (2)] + [u64_le lamports]
fn system_transfer_ix(from: &Pubkey, to: &Pubkey, lamports: u64) -> Instruction {
    // System Program ID
    let system_program_id =
        Pubkey::from_str("11111111111111111111111111111111").expect("system program id must be valid");

    // discriminator for SystemInstruction::Transfer is 2
    let mut data = Vec::with_capacity(4 + 8);
    data.extend_from_slice(&2u32.to_le_bytes());
    data.extend_from_slice(&lamports.to_le_bytes());

    Instruction {
        program_id: system_program_id,
        accounts: vec![
            AccountMeta::new(*from, true), // from: signer + writable
            AccountMeta::new(*to, false),  // to: writable (not signer)
        ],
        data,
    }
}



/// Backwards-compatible builder: keeps the same signature.
/// Uses a "normal/competitive" default priority for a simple System transfer.
///
/// For a plain transfer, CU usage is low, so we keep CU limit modest to avoid overpaying:
/// - CU limit: 50_000
/// - CU price: 25_000 microLamports/CU
pub fn build_transfer_tx(
    keypair_path: &str,
    to_pubkey: &str,
    lamports: u64,
    recent_blockhash: &str,
) -> Result<BuiltTx> {
    // Defaults tuned for a plain transfer
    const DEFAULT_CU_LIMIT: u32 = 250_000;
    const DEFAULT_CU_PRICE_MICRO_LAMPORTS: u64 = 135_000;

    build_transfer_tx_with_priority(
        keypair_path,
        to_pubkey,
        lamports,
        recent_blockhash,
        DEFAULT_CU_LIMIT,
        DEFAULT_CU_PRICE_MICRO_LAMPORTS,
    )
}

/// Explicit builder: lets caller control compute-unit limit and price.
/// Recommended for swaps / heavy programs where you may want CU limit ~200k-400k.
pub fn build_transfer_tx_with_priority(
    keypair_path: &str,
    to_pubkey: &str,
    lamports: u64,
    recent_blockhash: &str,
    cu_limit: u32,
    cu_price_micro_lamports: u64,
) -> Result<BuiltTx> {
    let payer: Keypair = read_keypair_file(keypair_path)
        .map_err(|e| anyhow::anyhow!("failed to read keypair file: {e}"))?;

    let bh: Hash = recent_blockhash
        .parse()
        .map_err(|e| anyhow::anyhow!("failed to parse blockhash: {e}"))?;

    let from = payer.pubkey();
    let to = Pubkey::from_str(to_pubkey).map_err(|e| anyhow::anyhow!("failed to parse --to pubkey: {e}"))?;

    if from == to {
        return Err(anyhow::anyhow!(
            "--to must be different from payer pubkey (avoid self-transfer for this bench)"
        ));
    }

    // Build instructions: ComputeBudget FIRST, then actual transfer.
    let [ix_limit, ix_price] = compute_budget_ixs(cu_limit, cu_price_micro_lamports);
    let ix_transfer = system_transfer_ix(&from, &to, lamports);

    let ixs = vec![ix_limit, ix_price, ix_transfer];
    let msg = Message::new(&ixs, Some(&from));
    let tx = Transaction::new(&[&payer], msg, bh);

    let sig = tx
        .signatures
        .get(0)
        .ok_or_else(|| anyhow::anyhow!("no signature in tx"))?
        .to_string();

    let tx_bytes =
        bincode::serialize(&tx).map_err(|e| anyhow::anyhow!("failed to serialize tx (bincode): {e}"))?;

    Ok(BuiltTx {
        tx_bytes,
        signature: sig,
    })
}
