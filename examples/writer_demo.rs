use clickhouse::Row;
use serde::Serialize;
use shadow_index::db::{create_client, writer::ClickHouseWriter, SchemaManager};

#[derive(Row, Serialize, Debug)]
struct BlockRow {
    block_number: u64,
    sign: i8,
    hash: Vec<u8>,
    parent_hash: Vec<u8>,
    timestamp: i64,
    gas_used: u64,
    base_fee: u128,
    miner: Vec<u8>,
    state_root: Vec<u8>,
    extra_data: String,
}

impl BlockRow {
    fn new(block_number: u64, sign: i8) -> Self {
        Self {
            block_number,
            sign,
            hash: vec![block_number as u8; 32],
            parent_hash: vec![(block_number - 1) as u8; 32],
            timestamp: 1640000000000 + (block_number as i64 * 12000),
            gas_used: 5000000 + (block_number * 1000),
            base_fee: 15000000000 + (block_number as u128 * 1000000),
            miner: vec![0xAA; 20],
            state_root: vec![0xFF; 32],
            extra_data: format!("Block {}", block_number),
        }
    }
}

#[tokio::main]
async fn main() -> eyre::Result<()> {
    tracing_subscriber::fmt::init();

    let clickhouse_url =
        std::env::var("CLICKHOUSE_URL").unwrap_or_else(|_| "http://localhost:8123".to_string());

    println!("connecting to ClickHouse at: {}", clickhouse_url);
    let client = create_client(&clickhouse_url);

    println!("initializing database schema...");
    let schema_manager = SchemaManager::new(client.clone());
    schema_manager.initialize_schema().await?;
    schema_manager.verify_schema().await?;

    let writer = ClickHouseWriter::new(client.clone());

    println!("\ninserting 10 committed blocks...");
    let mut commit_rows = Vec::new();
    for i in 1000..1010 {
        commit_rows.push(BlockRow::new(i, 1));
    }
    writer.insert_batch("blocks", &commit_rows).await?;
    println!("inserted {} blocks", commit_rows.len());

    println!("\nsimulating reorg: reverting blocks 1008-1009...");
    let mut revert_rows = Vec::new();
    for i in 1008..1010 {
        revert_rows.push(BlockRow::new(i, -1));
    }
    writer.insert_batch("blocks", &revert_rows).await?;
    println!("reverted {} blocks", revert_rows.len());

    tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;

    println!("\nquerying inserted data...");

    let total_count: u64 = client
        .query("SELECT count(*) FROM blocks WHERE block_number >= 1000 AND block_number < 1010")
        .fetch_one()
        .await?;
    println!("total rows in table: {}", total_count);

    let commit_count: u64 = client
        .query("SELECT count(*) FROM blocks WHERE block_number >= 1000 AND block_number < 1010 AND sign = 1")
        .fetch_one()
        .await?;
    println!("rows with sign=+1 (commits): {}", commit_count);

    let revert_count: u64 = client
        .query("SELECT count(*) FROM blocks WHERE block_number >= 1000 AND block_number < 1010 AND sign = -1")
        .fetch_one()
        .await?;
    println!("rows with sign=-1 (reverts): {}", revert_count);

    let sign_sum: i64 = client
        .query("SELECT sum(sign) FROM blocks WHERE block_number >= 1000 AND block_number < 1010")
        .fetch_one()
        .await?;
    println!("effective block count (sum of signs): {}", sign_sum);

    println!("\nwriter demo complete");
    println!("\nnote: ClickHouse's CollapsingMergeTree will eventually collapse");
    println!("rows with sign=+1 and sign=-1, effectively removing reverted blocks.");

    Ok(())
}
