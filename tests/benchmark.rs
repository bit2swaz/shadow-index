use clickhouse::Client;
use eyre::Result;
use shadow_index::db::models::{BlockRow, TransactionRow};
use shadow_index::db::writer::ClickHouseWriter;
use shadow_index::exex::Batcher;
use shadow_index::utils::CursorManager;
use std::time::{Duration, Instant};
use testcontainers::{clients::Cli, core::WaitFor, Container, GenericImage};
use tokio::fs;

// ========================================================================
// TESTCONTAINER SETUP
// ========================================================================

struct ClickHouseContainer<'a> {
    #[allow(dead_code)]
    container: Container<'a, GenericImage>,
}

impl<'a> ClickHouseContainer<'a> {
    async fn start(docker: &'a Cli) -> Self {
        let image = GenericImage::new("clickhouse/clickhouse-server", "23.8")
            .with_exposed_port(8123);

        let container = docker.run(image);

        tokio::time::sleep(Duration::from_secs(15)).await;

        Self { container }
    }

    fn client(&self) -> Client {
        let port = self.container.get_host_port_ipv4(8123);
        let url = format!("http://localhost:{}", port);
        Client::default()
            .with_url(url)
            .with_database("default")
    }
}

// ========================================================================
// MOCK DATA GENERATION
// ========================================================================

fn create_mock_block_row(block_number: u64, sign: i8) -> BlockRow {
    BlockRow {
        block_number,
        sign,
        hash: vec![block_number as u8; 32],
        parent_hash: vec![(block_number - 1) as u8; 32],
        timestamp: 1700000000 + (block_number as i64 * 12),
        gas_used: 15_000_000,
        base_fee: 20_000_000_000,
        miner: vec![0x42; 20],
        state_root: vec![0xAB; 32],
        extra_data: format!("block_{}", block_number),
    }
}

fn create_mock_transaction_rows(block_number: u64, tx_count: usize, sign: i8) -> Vec<TransactionRow> {
    (0..tx_count)
        .map(|tx_index| {
            let mut tx_hash = vec![0u8; 32];
            tx_hash[0] = block_number as u8;
            tx_hash[1] = tx_index as u8;
            
            TransactionRow {
                tx_hash,
                block_number,
                tx_index: tx_index as u32,
                sign,
                from: vec![0x11; 20],
                to: Some(vec![0x22; 20]),
                value: "1000000000000000000".to_string(),
                input: "0x".to_string(),
                status: 1,
                nonce: tx_index as u64,
                gas_limit: 21000,
                gas_price: 20_000_000_000,
                max_fee_per_gas: Some(25_000_000_000),
                max_priority_fee_per_gas: Some(2_000_000_000),
                chain_id: 1,
            }
        })
        .collect()
}

// ========================================================================
// DATABASE SCHEMA SETUP
// ========================================================================

async fn setup_schema(client: &Client) -> Result<()> {
    let schema = r#"
        CREATE TABLE IF NOT EXISTS blocks (
            block_number UInt64,
            sign Int8,
            hash String,
            parent_hash String,
            timestamp Int64,
            gas_used UInt64,
            base_fee UInt128,
            miner String,
            state_root String,
            extra_data String
        ) ENGINE = CollapsingMergeTree(sign)
        ORDER BY (block_number, sign);
    "#;

    client.query(schema).execute().await?;

    let tx_schema = r#"
        CREATE TABLE IF NOT EXISTS transactions (
            tx_hash String,
            block_number UInt64,
            tx_index UInt32,
            sign Int8,
            from String,
            to Nullable(String),
            value String,
            input String,
            status UInt8,
            nonce UInt64,
            gas_limit UInt64,
            gas_price UInt128,
            max_fee_per_gas Nullable(UInt128),
            max_priority_fee_per_gas Nullable(UInt128),
            chain_id UInt64
        ) ENGINE = CollapsingMergeTree(sign)
        ORDER BY (block_number, tx_index, sign);
    "#;

    client.query(tx_schema).execute().await?;

    Ok(())
}

// ========================================================================
// LATENCY METRICS HELPERS
// ========================================================================

struct LatencyStats {
    latencies: Vec<Duration>,
}

impl LatencyStats {
    fn new() -> Self {
        Self {
            latencies: Vec::new(),
        }
    }

    fn record(&mut self, latency: Duration) {
        self.latencies.push(latency);
    }

    fn p50(&self) -> Duration {
        self.percentile(50.0)
    }

    fn p95(&self) -> Duration {
        self.percentile(95.0)
    }

    fn p99(&self) -> Duration {
        self.percentile(99.0)
    }

    fn percentile(&self, p: f64) -> Duration {
        if self.latencies.is_empty() {
            return Duration::ZERO;
        }

        let mut sorted = self.latencies.clone();
        sorted.sort();

        let index = ((p / 100.0) * sorted.len() as f64).floor() as usize;
        let index = index.min(sorted.len() - 1);

        sorted[index]
    }

    fn avg(&self) -> Duration {
        if self.latencies.is_empty() {
            return Duration::ZERO;
        }

        let total: Duration = self.latencies.iter().sum();
        total / self.latencies.len() as u32
    }

    fn max(&self) -> Duration {
        self.latencies.iter().max().copied().unwrap_or(Duration::ZERO)
    }

    fn min(&self) -> Duration {
        self.latencies.iter().min().copied().unwrap_or(Duration::ZERO)
    }
}

// ========================================================================
// THROUGHPUT BENCHMARK
// ========================================================================

async fn run_throughput_benchmark() -> Result<()> {
    println!("\n{}", "=".repeat(80));
    println!("THROUGHPUT BENCHMARK: Historical Backfill Simulation");
    println!("{}\n", "=".repeat(80));

    let docker = Cli::default();
    let container = ClickHouseContainer::start(&docker).await;
    let client = container.client();

    setup_schema(&client).await?;

    let cursor_path = "/tmp/shadow-benchmark-throughput-cursor";
    fs::write(cursor_path, "0").await?;
    let mut cursor = CursorManager::new(cursor_path)?;

    let writer = ClickHouseWriter::new(client.clone());

    let mut batcher = Batcher::with_thresholds(5_000, Duration::from_millis(50));

    const TOTAL_BLOCKS: usize = 10_000;
    const TX_PER_BLOCK: usize = 10;

    println!("Configuration:");
    println!("  - Total blocks: {}", TOTAL_BLOCKS);
    println!("  - Transactions per block: {}", TX_PER_BLOCK);
    println!("  - Buffer size: 5,000 rows");
    println!("  - Flush interval: 50ms");
    println!();

    println!("Generating mock data...\n");

    let start = Instant::now();

    for block_number in 1..=(TOTAL_BLOCKS as u64) {
        let block_row = create_mock_block_row(block_number, 1);
        batcher.push_block(block_row);

        let tx_rows = create_mock_transaction_rows(block_number, TX_PER_BLOCK, 1);
        batcher.push_transactions(tx_rows);

        if batcher.should_flush() {
            writer.flush(&mut batcher).await?;
            cursor.update_cursor(block_number)?;
        }
    }

    if !batcher.is_empty() {
        writer.flush(&mut batcher).await?;
        cursor.update_cursor(TOTAL_BLOCKS as u64)?;
    }

    let elapsed = start.elapsed();

    let blocks_per_sec = TOTAL_BLOCKS as f64 / elapsed.as_secs_f64();
    let total_txs = TOTAL_BLOCKS * TX_PER_BLOCK;
    let txs_per_sec = total_txs as f64 / elapsed.as_secs_f64();

    println!("\n{}", "=".repeat(80));
    println!("THROUGHPUT RESULTS");
    println!("{}", "=".repeat(80));
    println!("  Total blocks processed: {}", TOTAL_BLOCKS);
    println!("  Total transactions: {}", total_txs);
    println!("  Elapsed time: {:.2}s", elapsed.as_secs_f64());
    println!();
    println!("  Throughput:");
    println!("    - Blocks/sec: {:.2}", blocks_per_sec);
    println!("    - Transactions/sec: {:.2}", txs_per_sec);
    println!("{}\n", "=".repeat(80));

    let count: u64 = client
        .query("SELECT count(*) FROM blocks WHERE sign = 1")
        .fetch_one()
        .await?;
    assert_eq!(count, TOTAL_BLOCKS as u64);

    fs::remove_file(cursor_path).await?;

    Ok(())
}

#[tokio::test]
#[ignore] // requires docker
async fn benchmark_throughput() -> Result<()> {
    run_throughput_benchmark().await
}

// Quick throughput test with fewer blocks for faster iteration
#[tokio::test]
#[ignore] // requires docker
async fn benchmark_throughput_quick() -> Result<()> {
    println!("\n{}", "=".repeat(80));
    println!("QUICK THROUGHPUT TEST: 1000 blocks");
    println!("{}\n", "=".repeat(80));

    let docker = Cli::default();
    let container = ClickHouseContainer::start(&docker).await;
    let client = container.client();

    setup_schema(&client).await?;

    let cursor_path = "/tmp/shadow-benchmark-throughput-quick-cursor";
    fs::write(cursor_path, "0").await?;
    let mut cursor = CursorManager::new(cursor_path)?;

    let writer = ClickHouseWriter::new(client.clone());
    let mut batcher = Batcher::with_thresholds(5_000, Duration::from_millis(50));

    const TOTAL_BLOCKS: usize = 1_000;
    const TX_PER_BLOCK: usize = 10;

    println!("Configuration:");
    println!("  - Total blocks: {}", TOTAL_BLOCKS);
    println!("  - Transactions per block: {}", TX_PER_BLOCK);
    println!("  - Buffer size: 5,000 rows");
    println!("  - Flush interval: 50ms");
    println!();

    println!("Starting quick throughput test...\n");

    let start = Instant::now();

    for block_number in 1..=(TOTAL_BLOCKS as u64) {
        let block_row = create_mock_block_row(block_number, 1);
        batcher.push_block(block_row);

        let tx_rows = create_mock_transaction_rows(block_number, TX_PER_BLOCK, 1);
        batcher.push_transactions(tx_rows);

        if batcher.should_flush() {
            writer.flush(&mut batcher).await?;
            cursor.update_cursor(block_number)?;
        }
    }

    if !batcher.is_empty() {
        writer.flush(&mut batcher).await?;
        cursor.update_cursor(TOTAL_BLOCKS as u64)?;
    }

    let elapsed = start.elapsed();

    let blocks_per_sec = TOTAL_BLOCKS as f64 / elapsed.as_secs_f64();
    let total_txs = TOTAL_BLOCKS * TX_PER_BLOCK;
    let txs_per_sec = total_txs as f64 / elapsed.as_secs_f64();

    println!("\n{}", "=".repeat(80));
    println!("QUICK THROUGHPUT RESULTS");
    println!("{}", "=".repeat(80));
    println!("  Total blocks processed: {}", TOTAL_BLOCKS);
    println!("  Total transactions: {}", total_txs);
    println!("  Elapsed time: {:.2}s", elapsed.as_secs_f64());
    println!();
    println!("  Throughput:");
    println!("    - Blocks/sec: {:.2}", blocks_per_sec);
    println!("    - Transactions/sec: {:.2}", txs_per_sec);
    println!("{}\n", "=".repeat(80));

    let count: u64 = client
        .query("SELECT count(*) FROM blocks WHERE sign = 1")
        .fetch_one()
        .await?;
    assert_eq!(count, TOTAL_BLOCKS as u64);

    fs::remove_file(cursor_path).await?;

    Ok(())
}

// ========================================================================
// LATENCY BENCHMARK
// ========================================================================

async fn run_latency_benchmark() -> Result<()> {
    println!("\n{}", "=".repeat(80));
    println!("LATENCY BENCHMARK: Real-Time Sync Simulation");
    println!("{}\n", "=".repeat(80));

    let docker = Cli::default();
    let container = ClickHouseContainer::start(&docker).await;
    let client = container.client();

    setup_schema(&client).await?;

    let cursor_path = "/tmp/shadow-benchmark-latency-cursor";
    fs::write(cursor_path, "0").await?;
    let mut cursor = CursorManager::new(cursor_path)?;

    let writer = ClickHouseWriter::new(client.clone());

    let mut batcher = Batcher::with_thresholds(1, Duration::from_millis(10));

    const TOTAL_BLOCKS: usize = 100;
    const TX_PER_BLOCK: usize = 50;
    const BLOCK_INTERVAL_MS: u64 = 100;

    println!("Configuration:");
    println!("  - Total blocks: {}", TOTAL_BLOCKS);
    println!("  - Transactions per block: {}", TX_PER_BLOCK);
    println!("  - Block interval: {}ms", BLOCK_INTERVAL_MS);
    println!("  - Buffer size: 1 row (immediate flush)");
    println!();

    let mut stats = LatencyStats::new();

    println!("Starting latency test (this will take ~{}s)...\n", 
             (TOTAL_BLOCKS as u64 * BLOCK_INTERVAL_MS) / 1000);

    for block_number in 1..=(TOTAL_BLOCKS as u64) {
        let block_start = Instant::now();

        let block_row = create_mock_block_row(block_number, 1);
        batcher.push_block(block_row);

        let tx_rows = create_mock_transaction_rows(block_number, TX_PER_BLOCK, 1);
        batcher.push_transactions(tx_rows);

        writer.flush(&mut batcher).await?;
        cursor.update_cursor(block_number)?;

        let latency = block_start.elapsed();
        stats.record(latency);

        if block_number % 10 == 0 {
            println!("  Processed {} blocks...", block_number);
        }

        tokio::time::sleep(Duration::from_millis(BLOCK_INTERVAL_MS)).await;
    }

    println!("\n{}", "=".repeat(80));
    println!("LATENCY RESULTS");
    println!("{}", "=".repeat(80));
    println!("  Total blocks measured: {}", TOTAL_BLOCKS);
    println!();
    println!("  End-to-End Latency (block received -> data flushed):");
    println!("    - p50 (median): {:.2}ms", stats.p50().as_secs_f64() * 1000.0);
    println!("    - p95: {:.2}ms", stats.p95().as_secs_f64() * 1000.0);
    println!("    - p99: {:.2}ms", stats.p99().as_secs_f64() * 1000.0);
    println!("    - average: {:.2}ms", stats.avg().as_secs_f64() * 1000.0);
    println!("    - min: {:.2}ms", stats.min().as_secs_f64() * 1000.0);
    println!("    - max: {:.2}ms", stats.max().as_secs_f64() * 1000.0);
    println!("{}\n", "=".repeat(80));

    let count: u64 = client
        .query("SELECT count(*) FROM blocks WHERE sign = 1")
        .fetch_one()
        .await?;
    assert_eq!(count, TOTAL_BLOCKS as u64);

    fs::remove_file(cursor_path).await?;

    Ok(())
}

#[tokio::test]
#[ignore] // requires docker
async fn benchmark_latency() -> Result<()> {
    run_latency_benchmark().await
}

// ========================================================================
// COMBINED BENCHMARK (RUN BOTH)
// ========================================================================

#[tokio::test]
#[ignore] // requires docker
async fn benchmark_all() -> Result<()> {
    println!("\n{}", "#".repeat(80));
    println!("SHADOW-INDEX COMPREHENSIVE BENCHMARK SUITE");
    println!("{}", "#".repeat(80));

    run_throughput_benchmark().await?;
    run_latency_benchmark().await?;

    println!("\n{}", "#".repeat(80));
    println!("ALL BENCHMARKS COMPLETED SUCCESSFULLY");
    println!("{}\n", "#".repeat(80));

    Ok(())
}
