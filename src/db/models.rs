use clickhouse::Row;
use serde::Serialize;

#[derive(Debug, Clone, Row, Serialize)]
pub struct BlockRow {
    pub block_number: u64,

    pub sign: i8,

    pub hash: Vec<u8>,

    pub parent_hash: Vec<u8>,

    pub timestamp: i64,

    pub gas_used: u64,

    pub base_fee: u128,

    pub miner: Vec<u8>,

    pub state_root: Vec<u8>,

    pub extra_data: String,
}

impl BlockRow {
    #[allow(dead_code)]
    pub fn new(block_number: u64, sign: i8) -> Self {
        Self {
            block_number,
            sign,
            hash: Vec::new(),
            parent_hash: Vec::new(),
            timestamp: 0,
            gas_used: 0,
            base_fee: 0,
            miner: Vec::new(),
            state_root: Vec::new(),
            extra_data: String::new(),
        }
    }
}

#[derive(Debug, Clone, Row, Serialize)]
pub struct TransactionRow {
    pub tx_hash: Vec<u8>,

    pub block_number: u64,

    pub tx_index: u32,

    pub sign: i8,

    pub from: Vec<u8>,

    pub to: Option<Vec<u8>>,

    pub value: String,

    pub input: String,

    pub status: u8,

    pub nonce: u64,

    pub gas_limit: u64,

    pub gas_price: u128,

    pub max_fee_per_gas: Option<u128>,

    pub max_priority_fee_per_gas: Option<u128>,

    pub chain_id: u64,
}

impl TransactionRow {
    #[allow(dead_code)]
    pub fn new(tx_hash: Vec<u8>, block_number: u64, tx_index: u32, sign: i8) -> Self {
        Self {
            tx_hash,
            block_number,
            tx_index,
            sign,
            from: Vec::new(),
            to: None,
            value: "0".to_string(),
            input: String::new(),
            status: 0,
            nonce: 0,
            gas_limit: 0,
            gas_price: 0,
            max_fee_per_gas: None,
            max_priority_fee_per_gas: None,
            chain_id: 0,
        }
    }
}

#[derive(Debug, Clone, Row, Serialize)]
pub struct LogRow {
    pub block_number: u64,

    pub sign: i8,

    pub tx_hash: Vec<u8>,

    pub tx_index: u32,

    pub log_index: u32,

    pub address: Vec<u8>,

    pub topic0: Vec<u8>,

    pub topics: Vec<Vec<u8>>,

    pub data: String,
}

impl LogRow {
    #[allow(dead_code)]
    pub fn new(
        block_number: u64,
        tx_hash: Vec<u8>,
        tx_index: u32,
        log_index: u32,
        sign: i8,
    ) -> Self {
        Self {
            block_number,
            sign,
            tx_hash,
            tx_index,
            log_index,
            address: Vec::new(),
            topic0: Vec::new(),
            topics: Vec::new(),
            data: String::new(),
        }
    }
}

#[derive(Debug, Clone, Row, Serialize)]
pub struct StorageDiffRow {
    pub block_number: u64,

    pub sign: i8,

    pub address: Vec<u8>,

    pub slot: Vec<u8>,

    pub value: Vec<u8>,
}

impl StorageDiffRow {
    #[allow(dead_code)]
    pub fn new(
        block_number: u64,
        address: Vec<u8>,
        slot: Vec<u8>,
        value: Vec<u8>,
        sign: i8,
    ) -> Self {
        Self {
            block_number,
            sign,
            address,
            slot,
            value,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_block_row_creation() {
        let block = BlockRow::new(100, 1);
        assert_eq!(block.block_number, 100);
        assert_eq!(block.sign, 1);
        assert_eq!(block.gas_used, 0);
    }

    #[test]
    fn test_transaction_row_creation() {
        let tx_hash = vec![1u8; 32];
        let tx = TransactionRow::new(tx_hash.clone(), 100, 0, 1);
        assert_eq!(tx.tx_hash, tx_hash);
        assert_eq!(tx.block_number, 100);
        assert_eq!(tx.tx_index, 0);
        assert_eq!(tx.sign, 1);
    }

    #[test]
    fn test_log_row_creation() {
        let tx_hash = vec![2u8; 32];
        let log = LogRow::new(100, tx_hash.clone(), 0, 0, 1);
        assert_eq!(log.block_number, 100);
        assert_eq!(log.tx_hash, tx_hash);
        assert_eq!(log.log_index, 0);
        assert_eq!(log.sign, 1);
    }

    #[test]
    fn test_storage_diff_row_creation() {
        let address = vec![3u8; 20];
        let slot = vec![4u8; 32];
        let value = vec![5u8; 32];
        let diff = StorageDiffRow::new(100, address.clone(), slot.clone(), value.clone(), 1);
        assert_eq!(diff.block_number, 100);
        assert_eq!(diff.address, address);
        assert_eq!(diff.slot, slot);
        assert_eq!(diff.value, value);
        assert_eq!(diff.sign, 1);
    }

    #[test]
    fn test_sign_values() {
        let block_commit = BlockRow::new(100, 1);
        assert_eq!(block_commit.sign, 1);

        let block_revert = BlockRow::new(100, -1);
        assert_eq!(block_revert.sign, -1);
    }

    #[test]
    fn test_transaction_nullable_fields() {
        let tx_hash = vec![1u8; 32];
        let mut tx = TransactionRow::new(tx_hash, 100, 0, 1);

        assert_eq!(tx.to, None);

        tx.to = Some(vec![0xAA; 20]);
        assert!(tx.to.is_some());

        assert_eq!(tx.max_fee_per_gas, None);
        assert_eq!(tx.max_priority_fee_per_gas, None);

        tx.max_fee_per_gas = Some(1000000000);
        tx.max_priority_fee_per_gas = Some(1000000);
        assert!(tx.max_fee_per_gas.is_some());
        assert!(tx.max_priority_fee_per_gas.is_some());
    }

    #[test]
    fn test_value_as_string() {
        let tx_hash = vec![1u8; 32];
        let mut tx = TransactionRow::new(tx_hash, 100, 0, 1);

        tx.value = "1000000000000000000".to_string();
        assert_eq!(tx.value, "1000000000000000000");
    }
}
