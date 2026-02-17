use crate::db::models::{BlockRow, LogRow, StorageDiffRow, TransactionRow};
use std::time::{Duration, Instant};

#[derive(Debug)]
pub struct Batcher {
    pub blocks: Vec<BlockRow>,

    pub transactions: Vec<TransactionRow>,

    pub logs: Vec<LogRow>,

    pub storage_diffs: Vec<StorageDiffRow>,

    last_flush: Instant,

    size_threshold: usize,

    time_threshold: Duration,
}

impl Batcher {
    pub fn new() -> Self {
        Self::with_thresholds(10_000, Duration::from_millis(100))
    }

    pub fn with_thresholds(size_threshold: usize, time_threshold: Duration) -> Self {
        Self {
            blocks: Vec::new(),
            transactions: Vec::new(),
            logs: Vec::new(),
            storage_diffs: Vec::new(),
            last_flush: Instant::now(),
            size_threshold,
            time_threshold,
        }
    }

    pub fn push_block(&mut self, block: BlockRow) {
        self.blocks.push(block);
    }

    pub fn push_transactions(&mut self, txs: Vec<TransactionRow>) {
        self.transactions.extend(txs);
    }

    pub fn push_logs(&mut self, logs: Vec<LogRow>) {
        self.logs.extend(logs);
    }

    pub fn push_storage_diffs(&mut self, diffs: Vec<StorageDiffRow>) {
        self.storage_diffs.extend(diffs);
    }

    pub fn total_rows(&self) -> usize {
        self.blocks.len() + self.transactions.len() + self.logs.len() + self.storage_diffs.len()
    }

    pub fn is_empty(&self) -> bool {
        self.total_rows() == 0
    }

    pub fn should_flush(&self) -> bool {
        let size_exceeded = self.total_rows() >= self.size_threshold;

        let time_exceeded = self.last_flush.elapsed() >= self.time_threshold;

        size_exceeded || time_exceeded
    }

    pub fn clear(&mut self) {
        self.blocks.clear();
        self.transactions.clear();
        self.logs.clear();
        self.storage_diffs.clear();
        self.last_flush = Instant::now();
    }

    pub fn time_since_flush(&self) -> Duration {
        self.last_flush.elapsed()
    }
}

impl Default for Batcher {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;

    fn dummy_block(block_number: u64, sign: i8) -> BlockRow {
        BlockRow {
            block_number,
            sign,
            hash: vec![0u8; 32],
            parent_hash: vec![0u8; 32],
            timestamp: 0,
            gas_used: 0,
            base_fee: 0,
            miner: vec![0u8; 20],
            state_root: vec![0u8; 32],
            extra_data: String::new(),
        }
    }

    fn dummy_tx(block_number: u64, tx_index: u32, sign: i8) -> TransactionRow {
        TransactionRow {
            tx_hash: vec![0u8; 32],
            block_number,
            tx_index,
            sign,
            from: vec![0u8; 20],
            to: None,
            value: "0".to_string(),
            input: String::new(),
            status: 0,
            nonce: 0,
            gas_limit: 0,
            gas_price: 0,
            max_fee_per_gas: None,
            max_priority_fee_per_gas: None,
            chain_id: 1,
        }
    }

    #[test]
    fn test_new_batcher_is_empty() {
        let batcher = Batcher::new();
        assert!(batcher.is_empty());
        assert_eq!(batcher.total_rows(), 0);
    }

    #[test]
    fn test_push_block() {
        let mut batcher = Batcher::new();
        batcher.push_block(dummy_block(100, 1));

        assert_eq!(batcher.blocks.len(), 1);
        assert_eq!(batcher.total_rows(), 1);
        assert!(!batcher.is_empty());
    }

    #[test]
    fn test_push_transactions() {
        let mut batcher = Batcher::new();
        let txs = vec![
            dummy_tx(100, 0, 1),
            dummy_tx(100, 1, 1),
            dummy_tx(100, 2, 1),
        ];

        batcher.push_transactions(txs);

        assert_eq!(batcher.transactions.len(), 3);
        assert_eq!(batcher.total_rows(), 3);
    }

    #[test]
    fn test_total_rows_counts_all_types() {
        let mut batcher = Batcher::new();

        batcher.push_block(dummy_block(100, 1));
        batcher.push_transactions(vec![dummy_tx(100, 0, 1), dummy_tx(100, 1, 1)]);

        assert_eq!(batcher.total_rows(), 3);
    }

    #[test]
    fn test_should_flush_size_threshold() {
        let mut batcher = Batcher::new();
        let size_threshold = 10_000;

        for i in 0..size_threshold {
            batcher.push_transactions(vec![dummy_tx(100, i as u32, 1)]);
        }

        assert_eq!(batcher.total_rows(), size_threshold);
        assert!(
            batcher.should_flush(),
            "should flush when size threshold is reached"
        );
    }

    #[test]
    fn test_should_not_flush_below_thresholds() {
        let batcher = Batcher::new();

        assert!(
            !batcher.should_flush(),
            "should not flush empty buffer immediately"
        );
    }

    #[test]
    fn test_should_flush_time_threshold() {
        let batcher = Batcher::new();

        thread::sleep(Duration::from_millis(101));

        assert!(
            batcher.should_flush(),
            "should flush after time threshold even with no data"
        );
    }

    #[test]
    fn test_should_flush_mixed_row_types() {
        let mut batcher = Batcher::new();

        for i in 0..5000 {
            batcher.push_block(dummy_block(i, 1));
            batcher.push_transactions(vec![dummy_tx(i, 0, 1)]);
        }

        assert_eq!(batcher.total_rows(), 10_000);
        assert!(
            batcher.should_flush(),
            "should flush with mixed row types at threshold"
        );
    }

    #[test]
    fn test_clear_resets_buffers() {
        let mut batcher = Batcher::new();

        batcher.push_block(dummy_block(100, 1));
        batcher.push_transactions(vec![dummy_tx(100, 0, 1)]);

        assert_eq!(batcher.total_rows(), 2);

        batcher.clear();

        assert!(batcher.is_empty());
        assert_eq!(batcher.total_rows(), 0);
        assert_eq!(batcher.blocks.len(), 0);
        assert_eq!(batcher.transactions.len(), 0);
    }

    #[test]
    fn test_clear_resets_flush_timer() {
        let mut batcher = Batcher::new();

        thread::sleep(Duration::from_millis(50));

        let time_before_clear = batcher.time_since_flush();
        assert!(time_before_clear >= Duration::from_millis(50));

        batcher.clear();

        let time_after_clear = batcher.time_since_flush();
        assert!(
            time_after_clear < Duration::from_millis(10),
            "timer should be reset after clear"
        );
    }

    #[test]
    fn test_time_threshold_exact_boundary() {
        let batcher = Batcher::new();

        thread::sleep(Duration::from_millis(100));

        assert!(
            batcher.should_flush(),
            "should flush at exact time threshold"
        );
    }

    #[test]
    fn test_size_threshold_one_below() {
        let mut batcher = Batcher::new();
        let size_threshold = 10_000;

        for i in 0..(size_threshold - 1) {
            batcher.push_transactions(vec![dummy_tx(100, i as u32, 1)]);
        }

        assert_eq!(batcher.total_rows(), size_threshold - 1);
        assert!(
            !batcher.should_flush(),
            "should not flush one row below threshold"
        );

        batcher.push_block(dummy_block(100, 1));
        assert!(
            batcher.should_flush(),
            "should flush when threshold is reached"
        );
    }

    #[test]
    fn test_composite_trigger_size_wins() {
        let mut batcher = Batcher::new();
        let size_threshold = 10_000;

        for i in 0..size_threshold {
            batcher.push_transactions(vec![dummy_tx(100, i as u32, 1)]);
        }

        assert!(
            batcher.should_flush(),
            "size trigger should activate immediately"
        );
    }

    #[test]
    fn test_composite_trigger_time_wins() {
        let mut batcher = Batcher::new();

        batcher.push_block(dummy_block(100, 1));

        assert!(!batcher.should_flush());

        thread::sleep(Duration::from_millis(101));

        assert!(
            batcher.should_flush(),
            "time trigger should activate with minimal data"
        );
    }
}
