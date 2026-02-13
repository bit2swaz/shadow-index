use crate::db::models::{BlockRow, TransactionRow, LogRow};
use reth_primitives::{SealedBlock, Receipt, TransactionSigned, Log};
use alloy_consensus::transaction::Transaction;
use alloy_consensus::transaction::SignerRecoverable;

pub mod state;
pub use state::transform_state;

pub fn transform_block(block: &SealedBlock, sign: i8) -> (BlockRow, Vec<TransactionRow>) {
    let header = block.header();
    let block_number = block.number;
    
    let block_row = BlockRow {
        block_number,
        sign,
        hash: block.hash().as_slice().to_vec(),
        parent_hash: header.parent_hash.as_slice().to_vec(),
        timestamp: header.timestamp as i64,
        gas_used: header.gas_used,
        base_fee: header.base_fee_per_gas.unwrap_or(0) as u128,
        miner: header.beneficiary.as_slice().to_vec(),
        state_root: header.state_root.as_slice().to_vec(),
        extra_data: hex::encode(&header.extra_data),
    };
    
    let transactions: Vec<TransactionRow> = block
        .body()
        .transactions
        .iter()
        .enumerate()
        .map(|(tx_index, tx)| transform_transaction(tx, block_number, tx_index as u32, sign))
        .collect();
    
    (block_row, transactions)
}

fn transform_transaction(
    tx: &TransactionSigned,
    block_number: u64,
    tx_index: u32,
    sign: i8,
) -> TransactionRow {
    let tx_hash = tx.hash();
    
    let from = tx.recover_signer()
        .map(|addr| addr.as_slice().to_vec())
        .unwrap_or_else(|_| vec![0u8; 20]);
    
    let to = tx.to().map(|addr| addr.as_slice().to_vec());
    
    let value = tx.value().to_string();
    
    let input = hex::encode(tx.input());
    
    let nonce = tx.nonce();
    
    let gas_limit = tx.gas_limit();
    
    let gas_price = tx.gas_price().unwrap_or(0) as u128;
    
    let max_fee_per_gas = Some(tx.max_fee_per_gas());
    let max_priority_fee_per_gas = tx.max_priority_fee_per_gas();
    
    let chain_id = tx.chain_id().unwrap_or(1);
    
    TransactionRow {
        tx_hash: tx_hash.as_slice().to_vec(),
        block_number,
        tx_index,
        sign,
        from,
        to,
        value,
        input,
        status: 0,
        nonce,
        gas_limit,
        gas_price,
        max_fee_per_gas,
        max_priority_fee_per_gas,
        chain_id,
    }
}

pub fn transform_logs(
    receipts: &[Receipt],
    block_number: u64,
    sign: i8,
) -> Vec<LogRow> {
    let mut log_rows = Vec::new();
    let mut global_log_index = 0u32;
    
    for (tx_index, receipt) in receipts.iter().enumerate() {
        for log in &receipt.logs {
            log_rows.push(transform_log(
                log,
                block_number,
                tx_index as u32,
                global_log_index,
                sign,
            ));
            global_log_index += 1;
        }
    }
    
    log_rows
}

fn transform_log(
    log: &Log,
    block_number: u64,
    tx_index: u32,
    log_index: u32,
    sign: i8,
) -> LogRow {
    let tx_hash = vec![0u8; 32];
    
    let topics = log.topics();
    let (topic0, remaining_topics) = if topics.is_empty() {
        (vec![0u8; 32], Vec::new())
    } else {
        let topic0 = topics[0].as_slice().to_vec();
        let remaining: Vec<Vec<u8>> = topics[1..]
            .iter()
            .map(|t| t.as_slice().to_vec())
            .collect();
        (topic0, remaining)
    };
    
    let data_bytes = &log.data.data;
    
    LogRow {
        block_number,
        sign,
        tx_hash,
        tx_index,
        log_index,
        address: log.address.as_slice().to_vec(),
        topic0,
        topics: remaining_topics,
        data: hex::encode(data_bytes),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_transform_logs_empty() {
        let log_rows = transform_logs(&[], 100, 1);
        assert_eq!(log_rows.len(), 0);
    }
}
