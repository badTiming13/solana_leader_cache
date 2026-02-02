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

/// Build System Program Transfer instruction data:
/// layout = [u32_le discriminator (2)] + [u64_le lamports]
fn system_transfer_ix(from: &Pubkey, to: &Pubkey, lamports: u64) -> Instruction {
    // System Program ID
    let system_program_id = Pubkey::from_str("11111111111111111111111111111111")
        .expect("system program id must be valid");

    // discriminator for SystemInstruction::Transfer is 2
    let mut data = Vec::with_capacity(4 + 8);
    data.extend_from_slice(&2u32.to_le_bytes());
    data.extend_from_slice(&lamports.to_le_bytes());

    Instruction {
        program_id: system_program_id,
        accounts: vec![
            AccountMeta::new(*from, true),  // from: signer + writable
            AccountMeta::new(*to, false),   // to: writable (not signer)
        ],
        data,
    }
}

pub fn build_transfer_tx(
    keypair_path: &str,
    to_pubkey: &str,
    lamports: u64,
    recent_blockhash: &str,
) -> Result<BuiltTx> {
    let payer: Keypair = read_keypair_file(keypair_path)
        .map_err(|e| anyhow::anyhow!("failed to read keypair file: {e}"))?;

    let bh: Hash = recent_blockhash
        .parse()
        .map_err(|e| anyhow::anyhow!("failed to parse blockhash: {e}"))?;

    let from = payer.pubkey();
    let to = Pubkey::from_str(to_pubkey)
        .map_err(|e| anyhow::anyhow!("failed to parse --to pubkey: {e}"))?;

    if from == to {
        return Err(anyhow::anyhow!("--to must be different from payer pubkey (avoid self-transfer for this bench)"));
    }

    let ix = system_transfer_ix(&from, &to, lamports);
    let msg = Message::new(&[ix], Some(&from));
    let tx = Transaction::new(&[&payer], msg, bh);

    let sig = tx
        .signatures
        .get(0)
        .ok_or_else(|| anyhow::anyhow!("no signature in tx"))?
        .to_string();

    let tx_bytes = bincode::serialize(&tx)
        .map_err(|e| anyhow::anyhow!("failed to serialize tx (bincode): {e}"))?;

    Ok(BuiltTx {
        tx_bytes,
        signature: sig,
    })
}
