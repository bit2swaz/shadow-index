# single source of truth (ssot)

comprehensive technical reference for shadow-index: a zero-latency ethereum indexer built as a reth execution extension.

**version**: 0.1.0  
**last updated**: february 2026  
**rust version**: 1.85+  
**target chains**: ethereum mainnet, sepolia testnet

---

## table of contents

- [project overview](#project-overview)
- [architecture](#architecture)
- [core components](#core-components)
- [data models](#data-models)
- [database schema](#database-schema)
- [configuration](#configuration)
- [metrics and observability](#metrics-and-observability)
- [api and interfaces](#api-and-interfaces)
- [deployment](#deployment)
- [performance characteristics](#performance-characteristics)
- [testing](#testing)
- [development workflow](#development-workflow)
- [dependencies](#dependencies)
- [file structure](#file-structure)

---

## project overview

### purpose

shadow-index eliminates the traditional rpc polling model used by blockchain indexers by running in-process with reth. it provides:
- **sub-10ms end-to-end latency** (block committed → database flushed)
- **7,000+ blocks/sec throughput** during historical backfill
- **native reorg handling** via clickhouse collapsingmergetree
- **zero network overhead** through in-process data access

### core problem solved

traditional ethereum indexers suffer from:
1. **polling overhead**: continuous http requests to check for new blocks (200-500ms latency per poll)
2. **rpc bottlenecks**: json serialization, network hops, eventual consistency
3. **incomplete data**: missing storage slot changes, internal transactions require trace apis
4. **reorg complexity**: manual state rollback logic prone to bugs

shadow-index solves this by hooking directly into reth's `canonstateonotification` stream, accessing committed blocks with zero serialization overhead.

### key differentiators

| feature | traditional indexers | shadow-index |
|---------|---------------------|--------------|
| architecture | external (rpc client) | in-process (exex) |
| latency | 2-30 seconds | 4-6ms (p50-p99) |
| throughput | 50-200 blocks/sec | 7,159 blocks/sec |
| reorg handling | manual rollback | native (collapsingmergetree) |
| storage slots | requires trace api | direct access |
| network overhead | high (json-rpc) | none (zero-copy) |

---

## architecture

### high-level architecture

```
┌──────────────────────────────────────────────────────────────┐
│                      reth node process                        │
│  ┌────────────┐                                               │
│  │    evm     │ block execution                               │
│  │ execution  │────────────┐                                  │
│  └────────────┘            │                                  │
│                            ▼                                  │
│                  ┌─────────────────┐                          │
│                  │  canonstate     │                          │
│                  │  notification   │                          │
│                  └────────┬────────┘                          │
│                           │ arc<chainevent>                   │
│                           ▼                                  │
│          ┌────────────────────────────────┐                  │
│          │   shadow-index exex            │                  │
│          │  ┌──────────────────────────┐  │                  │
│          │  │  1. transform            │  │                  │
│          │  │     blocks → blockrow     │  │                  │
│          │  │     txs → transactionrow  │  │                  │
│          │  │     logs → logrow         │  │                  │
│          │  │     diffs → storagediffrow│  │                  │
│          │  └──────────┬───────────────┘  │                  │
│          │             ▼                   │                  │
│          │  ┌──────────────────────────┐  │                  │
│          │  │  2. composite batcher    │  │                  │
│          │  │     buffer_size: 10k     │  │                  │
│          │  │     flush_interval: 100ms│  │                  │
│          │  └──────────┬───────────────┘  │                  │
│          │             ▼                   │                  │
│          │  ┌──────────────────────────┐  │                  │
│          │  │  3. clickhouse writer    │  │                  │
│          │  │     circuit breaker      │  │                  │
│          │  │     exponential backoff  │  │                  │
│          │  │     retry: 5 attempts    │  │                  │
│          │  └──────────┬───────────────┘  │                  │
│          │             ▼                   │                  │
│          │  ┌──────────────────────────┐  │                  │
│          │  │  4. cursor manager       │  │                  │
│          │  │     atomic file updates  │  │                  │
│          │  └──────────────────────────┘  │                  │
│          └────────────────────────────────┘                  │
│                           │                                   │
└───────────────────────────┼───────────────────────────────────┘
                            │ native protocol (9000)
                            ▼
                   ┌─────────────────┐
                   │   clickhouse    │
                   │   (port 8123)   │
                   └─────────────────┘
```

### data flow sequence

1. **block execution**: reth evm executes block, updates state trie
2. **commit notification**: reth emits `chaincommitted` with `arc<chainevent>`
3. **transform**: exex extracts primitives, converts to typed row structs
4. **buffering**: composite batcher accumulates rows until threshold (size or time)
5. **write**: clickhouse writer flushes batch via native protocol
6. **error handling**: transient errors trigger retry with exponential backoff
7. **cursor update**: on success, cursor manager persists last processed block
8. **reorg handling**: on `chainreverted`, replay blocks with sign=-1

### threading model

- **main thread**: tokio runtime, event loop processing notifications
- **database thread**: async tasks for clickhouse writes (non-blocking)
- **metrics thread**: prometheus exporter on dedicated port (9001)
- **cursor thread**: atomic file operations for persistence

### concurrency strategy

- **async/await**: tokio runtime with work-stealing scheduler
- **channels**: mpsc for passing batches between components
- **arc/mutex**: shared state for cursor manager
- **lock-free**: notification stream uses arc cloning (zero-copy)

---

## core components

### 1. shadowexex (src/exex/mod.rs)

**purpose**: main execution extension that integrates with reth

**key responsibilities**:
- receive `canonstateonotification` from reth
- orchestrate transform → buffer → write pipeline
- handle chain reorgs by replaying reverted blocks
- manage cursor for resume-on-restart

**struct definition**:
```rust
pub struct ShadowExEx<Node: FullNodeComponents> {
    ctx: ExExContext<Node>,
    writer: ClickHouseWriter,
    cursor: CursorManager,
    batcher: Batcher,
}
```

**lifecycle**:
1. `new()`: initialize with reth context, writer, cursor, batcher
2. `run()`: main event loop consuming notifications
3. `handle_commit()`: process committed blocks
4. `handle_reorg()`: process reverted blocks (sign=-1)
5. `poll()`: implement future trait for tokio runtime

### 2. clickhousewriter (src/db/writer.rs)

**purpose**: handle all database interactions with retry logic and circuit breaker

**key responsibilities**:
- establish clickhouse connection via native protocol
- execute insert statements with batched rows
- implement exponential backoff for transient failures
- track connection health with circuit breaker
- expose insert methods for each table

**struct definition**:
```rust
pub struct ClickHouseWriter {
    client: Client,
    buffer_size: usize,
    flush_interval: Duration,
}
```

**error handling**:
- **transient errors** (network, timeouts): retry with backoff (1s → 2s → 4s → 8s → 16s)
- **permanent errors** (syntax, schema): fail immediately with error log
- **circuit breaker**: open after 5 consecutive failures, close after 1 success

**write methods**:
- `insert_blocks(&self, rows: Vec<BlockRow>)`: insert block records
- `insert_transactions(&self, rows: Vec<TransactionRow>)`: insert transaction records
- `insert_logs(&self, rows: Vec<LogRow>)`: insert event log records
- `insert_storage_diffs(&self, rows: Vec<StorageDiffRow>)`: insert storage change records

### 3. composite batcher (src/exex/buffer.rs)

**purpose**: accumulate rows in memory until threshold (size or time) before flushing

**key responsibilities**:
- buffer block, transaction, log, and storage diff rows
- track buffer size and last flush timestamp
- trigger flush when buffer_size reached or flush_interval exceeded
- expose metrics for buffer saturation

**struct definition**:
```rust
pub struct Batcher {
    block_buffer: Vec<BlockRow>,
    tx_buffer: Vec<TransactionRow>,
    log_buffer: Vec<LogRow>,
    storage_buffer: Vec<StorageDiffRow>,
    buffer_size: usize,
    flush_interval: Duration,
    last_flush: Instant,
}
```

**thresholds**:
- **buffer_size**: 10,000 rows (configurable via `exex.buffer_size`)
- **flush_interval**: 100ms (configurable via `exex.flush_interval_ms`)
- **logic**: flush when `total_rows >= buffer_size OR elapsed >= flush_interval`

### 4. cursor manager (src/utils/cursor.rs)

**purpose**: persist last processed block number for crash recovery

**key responsibilities**:
- read cursor file on startup
- atomically update cursor after successful batch write
- ensure durability with fsync
- provide idempotent recovery mechanism

**struct definition**:
```rust
pub struct CursorManager {
    file_path: PathBuf,
    last_block: Arc<Mutex<u64>>,
}
```

**file format**: plain text file containing single u64 integer (e.g., `"12345678"`)

**atomic update**:
1. write new block number to temporary file (`shadow-index.cursor.tmp`)
2. fsync temporary file to disk
3. atomic rename temporary file to cursor file
4. update in-memory cache

### 5. transform engine (src/transform/mod.rs)

**purpose**: convert reth primitives to shadow-index row structs

**key responsibilities**:
- extract block metadata (hash, timestamp, gas, base fee)
- parse transactions (from, to, value, input, gas)
- decode event logs (address, topics, data)
- track storage slot changes (address, slot, value)
- handle eip-1559 transactions (max fee, priority fee)

**transform functions**:
- `block_to_row(block: &SealedBlock) -> BlockRow`
- `transaction_to_row(tx: &TransactionSigned, index: u32) -> TransactionRow`
- `receipt_to_logs(receipt: &Receipt) -> Vec<LogRow>`
- `state_changes_to_diffs(changes: &StateChanges) -> Vec<StorageDiffRow>`

### 6. metrics exporter (src/main.rs)

**purpose**: expose prometheus metrics on dedicated http endpoint

**key responsibilities**:
- initialize prometheus exporter on port 9001
- register all counter, gauge, and histogram metrics
- serve metrics in openmetrics text format
- update metrics throughout pipeline execution

**exposed metrics**:
- `shadow_index_blocks_processed_total`: counter
- `shadow_index_events_captured_total`: counter
- `shadow_index_buffer_saturation`: gauge
- `shadow_index_db_latency_seconds`: histogram
- `shadow_index_reorgs_handled_total`: counter
- `shadow_index_circuit_breaker_trips_total`: counter
- `shadow_index_db_retries_total`: counter

---

## data models

### blockrow

represents a single ethereum block with metadata.

```rust
pub struct BlockRow {
    pub block_number: u64,        // block height (primary key)
    pub sign: i8,                  // +1 for commit, -1 for revert (collapsingtree)
    pub hash: Vec<u8>,             // keccak256 block hash (32 bytes)
    pub parent_hash: Vec<u8>,      // previous block hash (32 bytes)
    pub timestamp: i64,            // unix timestamp (seconds since epoch)
    pub gas_used: u64,             // total gas consumed by transactions
    pub base_fee: u128,            // eip-1559 base fee per gas (wei)
    pub miner: Vec<u8>,            // coinbase address (20 bytes)
    pub state_root: Vec<u8>,       // merkle root of state trie (32 bytes)
    pub extra_data: String,        // arbitrary data included by miner
}
```

**size**: ~180 bytes per row (uncompressed)

### transactionrow

represents a single ethereum transaction with full metadata.

```rust
pub struct TransactionRow {
    pub tx_hash: Vec<u8>,                    // keccak256 tx hash (32 bytes)
    pub block_number: u64,                   // block containing this tx
    pub tx_index: u32,                       // position within block
    pub sign: i8,                            // +1 for commit, -1 for revert
    pub from: Vec<u8>,                       // sender address (20 bytes)
    pub to: Option<Vec<u8>>,                 // recipient (none for contract creation)
    pub value: String,                       // ether transferred (wei, as string for precision)
    pub input: String,                       // calldata (hex-encoded)
    pub status: u8,                          // 1 for success, 0 for failure
    pub nonce: u64,                          // sender nonce
    pub gas_limit: u64,                      // max gas allowed
    pub gas_price: u128,                     // legacy gas price (wei)
    pub max_fee_per_gas: Option<u128>,       // eip-1559 max fee (wei)
    pub max_priority_fee_per_gas: Option<u128>, // eip-1559 tip (wei)
    pub chain_id: u64,                       // network identifier
}
```

**size**: ~300-2000 bytes per row (varies with input length)

### logrow

represents a single event log emitted by smart contract execution.

```rust
pub struct LogRow {
    pub tx_hash: Vec<u8>,          // transaction that emitted this log (32 bytes)
    pub block_number: u64,         // block containing this log
    pub log_index: u32,            // position within block's logs
    pub sign: i8,                  // +1 for commit, -1 for revert
    pub address: Vec<u8>,          // contract that emitted log (20 bytes)
    pub topic0: Option<Vec<u8>>,   // event signature (keccak256 of signature)
    pub topic1: Option<Vec<u8>>,   // indexed parameter 1
    pub topic2: Option<Vec<u8>>,   // indexed parameter 2
    pub topic3: Option<Vec<u8>>,   // indexed parameter 3
    pub data: String,              // non-indexed parameters (abi-encoded, hex)
}
```

**size**: ~200-500 bytes per row (varies with data length)

### storagediffrow

represents a change to a contract's storage slot.

```rust
pub struct StorageDiffRow {
    pub block_number: u64,         // block where change occurred
    pub tx_hash: Vec<u8>,          // transaction that modified storage (32 bytes)
    pub sign: i8,                  // +1 for commit, -1 for revert
    pub address: Vec<u8>,          // contract address (20 bytes)
    pub slot: Vec<u8>,             // storage slot key (32 bytes)
    pub value: Vec<u8>,            // new value (32 bytes, 0-padded)
    pub previous_value: Option<Vec<u8>>, // old value (optional, for auditing)
}
```

**size**: ~130 bytes per row

---

## database schema

### clickhouse table definitions

shadow-index uses clickhouse with the **collapsingmergetree** engine for native reorg handling.

#### blocks table

```sql
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
ORDER BY (block_number)
PARTITION BY toYYYYMM(toDateTime(timestamp))
SETTINGS index_granularity = 8192;
```

**partitioning strategy**: monthly partitions by timestamp for efficient range queries and data lifecycle management.

#### transactions table

```sql
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
ORDER BY (block_number, tx_index)
PARTITION BY intDiv(block_number, 100000)
SETTINGS index_granularity = 8192;
```

**partitioning strategy**: every 100,000 blocks (~15 days on mainnet) for balanced partition sizes.

#### logs table

```sql
CREATE TABLE IF NOT EXISTS logs (
    tx_hash String,
    block_number UInt64,
    log_index UInt32,
    sign Int8,
    address String,
    topic0 Nullable(String),
    topic1 Nullable(String),
    topic2 Nullable(String),
    topic3 Nullable(String),
    data String
) ENGINE = CollapsingMergeTree(sign)
ORDER BY (block_number, log_index)
PARTITION BY intDiv(block_number, 100000)
SETTINGS index_granularity = 8192;
```

#### storage_diffs table

```sql
CREATE TABLE IF NOT EXISTS storage_diffs (
    block_number UInt64,
    tx_hash String,
    sign Int8,
    address String,
    slot String,
    value String,
    previous_value Nullable(String)
) ENGINE = CollapsingMergeTree(sign)
ORDER BY (address, slot, block_number)
PARTITION BY intDiv(block_number, 100000)
SETTINGS index_granularity = 8192;
```

**special indexing**: ordered by (address, slot, block_number) for efficient storage slot history queries.

### collapsingmergetree mechanics

**how reorgs work**:
1. block 100 committed → insert row with `sign=1`
2. block 100 reverted → insert row with `sign=-1` (identical data)
3. clickhouse merges rows: `1 + (-1) = 0` → row "collapses" (logically deleted)
4. new block 100' committed → insert row with `sign=1` (new data)

**query behavior**:
- simple `SELECT * FROM blocks` returns uncollapsed rows (includes reverted blocks)
- use `FINAL` modifier to collapse rows: `SELECT * FROM blocks FINAL WHERE block_number = 100`
- or use `GROUP BY` with `sum(sign)`: `SELECT *, sum(sign) as net FROM blocks GROUP BY * HAVING net > 0`

**advantages**:
- no delete operations required (inserts only)
- no transaction rollback logic in application code
- atomic reorg handling (single insert operation)
- preserves full history (reverted blocks remain queryable)

---

## configuration

### configuration hierarchy

shadow-index uses a layered configuration system (lowest to highest priority):

1. **default values** (hardcoded in `src/config.rs`)
2. **config.toml file** (optional, in working directory)
3. **environment variables** (prefix: `SHADOW_INDEX__`, separator: `__`)

### configuration structure

```toml
[clickhouse]
url = "http://localhost:8123"          # clickhouse http endpoint
username = "default"                    # database username
password = ""                           # database password (empty for default)
database = "shadow_index"               # target database name

[exex]
buffer_size = 10000                     # rows to buffer before flush
flush_interval_ms = 100                 # max time between flushes (milliseconds)

[backfill]
enabled = true                          # enable historical backfill on startup
batch_size = 100                        # blocks per backfill batch

[cursor]
file_path = "shadow-index.cursor"       # cursor persistence file
```

### environment variable examples

```bash
# clickhouse configuration
export SHADOW_INDEX__CLICKHOUSE__URL="http://prod-db:8123"
export SHADOW_INDEX__CLICKHOUSE__PASSWORD="secure_password"
export SHADOW_INDEX__CLICKHOUSE__DATABASE="mainnet_shadow"

# exex tuning
export SHADOW_INDEX__EXEX__BUFFER_SIZE=50000
export SHADOW_INDEX__EXEX__FLUSH_INTERVAL_MS=50

# disable backfill
export SHADOW_INDEX__BACKFILL__ENABLED=false
```

### validation rules

- `clickhouse.url`: must be non-empty, valid http/https url
- `clickhouse.database`: must be non-empty
- `exex.buffer_size`: must be > 0
- `exex.flush_interval_ms`: must be > 0
- `backfill.batch_size`: must be > 0 if backfill enabled
- `cursor.file_path`: must be non-empty, writable path

---

## metrics and observability

### prometheus metrics

shadow-index exposes metrics on `http://localhost:9001/metrics` in openmetrics text format.

#### counters

**shadow_index_blocks_processed_total**
- type: counter
- description: cumulative count of blocks successfully processed and written
- usage: `rate(shadow_index_blocks_processed_total[1m])` for blocks/sec throughput

**shadow_index_events_captured_total**
- type: counter
- description: cumulative count of all events (transactions + logs + storage diffs)
- usage: indicates total data volume processed

**shadow_index_reorgs_handled_total**
- type: counter
- description: number of chain reorganizations detected and handled
- usage: spikes indicate network instability

**shadow_index_circuit_breaker_trips_total**
- type: counter
- description: circuit breaker activations due to database failures
- usage: non-zero values indicate connectivity issues

**shadow_index_db_retries_total**
- type: counter
- description: total database operation retries
- usage: persistent growth indicates database performance issues

#### gauges

**shadow_index_buffer_saturation**
- type: gauge
- description: current number of rows buffered in memory (0 to buffer_size)
- usage: values near buffer_size indicate efficient batching
- alert threshold: > 90% sustained indicates backpressure

#### histograms

**shadow_index_db_latency_seconds**
- type: histogram
- description: distribution of database write latency measurements
- buckets: 0.001, 0.002, 0.005, 0.010, 0.025, 0.050, 0.100, 0.250, 0.500, 1.000, 2.500
- usage: `histogram_quantile(0.95, rate(shadow_index_db_latency_seconds_bucket[5m]))` for p95 latency

### grafana dashboards

shadow-index includes a pre-built grafana dashboard with 8 panels:

1. **block processing throughput**: timeseries showing blocks/sec and events/sec
2. **database write latency**: timeseries with p50, p95, p99 percentiles
3. **buffer saturation**: gauge with color-coded thresholds
4. **error & recovery metrics**: timeseries tracking reorgs, circuit breaker, retries
5. **total blocks processed**: stat panel with cumulative counter
6. **total events captured**: stat panel with cumulative counter
7. **circuit breaker status**: stat panel showing healthy/open state
8. **current p95 latency**: stat panel with 5-minute snapshot

**dashboard provisioning**: auto-loaded from `docker/grafana/provisioning/dashboards/shadow-index.json`

### logging

shadow-index uses structured logging via `tracing` crate:

**log levels**:
- **error**: database failures, cursor corruption, panic recovery
- **warn**: retry attempts, circuit breaker trips, configuration issues
- **info**: startup messages, milestone blocks (every 10k), shutdown
- **debug**: per-block processing, buffer flush events, cursor updates
- **trace**: raw notification details, transform operations (very verbose)

**configuration**:
```bash
export RUST_LOG="info,shadow_index=debug,reth_exex=info"
```

---

## api and interfaces

### reth exex interface

shadow-index implements the `ExEx` trait from reth-exex:

```rust
impl<Node> Future for ShadowExEx<Node>
where
    Node: FullNodeComponents<Types: reth_node_api::NodeTypes<Primitives = EthPrimitives>>,
{
    type Output = eyre::Result<()>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        // process notifications from reth
    }
}
```

**notification types handled**:
- `ChainCommitted`: new block(s) committed to canonical chain
- `ChainReverted`: block(s) reverted due to reorg
- `ChainReorged`: combined revert + commit in single notification

### clickhouse native protocol

shadow-index uses clickhouse native protocol (port 9000) for optimal performance:

**connection string**: `tcp://localhost:9000?database=shadow_index`

**batch insert format**:
```rust
client.insert("blocks").write(&rows).await?
```

**advantages over http**:
- 30-40% faster insert performance
- native type serialization (no string conversion)
- connection pooling with keepalive
- binary wire format (lower bandwidth)

### sql query interface

users query shadow-index data via standard sql:

**example queries**:

```sql
-- get latest 100 blocks
SELECT * FROM blocks FINAL
ORDER BY block_number DESC
LIMIT 100;

-- transaction count per block
SELECT block_number, count(*) as tx_count
FROM transactions FINAL
GROUP BY block_number
ORDER BY block_number DESC;

-- event logs for specific contract
SELECT * FROM logs FINAL
WHERE address = '0x...'
  AND block_number >= 18000000
ORDER BY block_number, log_index;

-- storage slot history
SELECT block_number, value
FROM storage_diffs FINAL
WHERE address = '0x...'
  AND slot = '0x...'
ORDER BY block_number;
```

---

## deployment

### docker compose deployment

**production configuration** (`docker-compose.prod.yml`):

```yaml
version: '3.8'

services:
  clickhouse:
    image: clickhouse/clickhouse-server:23.8
    ports:
      - "8123:8123"
      - "9000:9000"
    volumes:
      - clickhouse_data:/var/lib/clickhouse
    environment:
      CLICKHOUSE_DB: shadow_index
    deploy:
      resources:
        limits:
          cpus: '4'
          memory: 8G

  shadow-index:
    build: .
    depends_on:
      - clickhouse
    environment:
      CLICKHOUSE_URL: http://clickhouse:8123
      RUST_LOG: info,shadow_index=debug
    ports:
      - "9001:9001"  # prometheus metrics
    volumes:
      - reth_data:/root/.local/share/reth
      - ./shadow-index.cursor:/app/shadow-index.cursor
    deploy:
      resources:
        limits:
          cpus: '8'
          memory: 16G
    restart: unless-stopped

  prometheus:
    image: prom/prometheus:latest
    ports:
      - "9090:9090"
    volumes:
      - ./docker/prometheus/prometheus.yml:/etc/prometheus/prometheus.yml:ro

  grafana:
    image: grafana/grafana:latest
    ports:
      - "3000:3000"
    environment:
      GF_SECURITY_ADMIN_PASSWORD: <change-me>
    volumes:
      - ./docker/grafana/provisioning:/etc/grafana/provisioning:ro
```

### dockerfile (multi-stage build)

```dockerfile
FROM rust:1.85-bookworm AS builder
WORKDIR /app
COPY Cargo.toml Cargo.lock ./
COPY src ./src
RUN cargo build --release

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y ca-certificates && rm -rf /var/lib/apt/lists/*
COPY --from=builder /app/target/release/shadow-index /usr/local/bin/
CMD ["shadow-index"]
```

### system requirements

**minimum**:
- cpu: 4 cores (8 threads)
- ram: 16 gb
- disk: 500 gb nvme ssd
- network: 100 mbps

**recommended (mainnet production)**:
- cpu: 16 cores (32 threads) - amd epyc or intel xeon
- ram: 64 gb ecc
- disk: 2 tb nvme ssd (raid 0 for performance)
- network: 1 gbps

### operational considerations

**disk usage**:
- reth blockchain data: ~2 tb (mainnet, full archive)
- clickhouse data: ~500 gb (mainnet, 1 year history)
- growth rate: ~10 gb/month (clickhouse, compressed)

**network bandwidth**:
- reth p2p: ~5-10 mbps sustained
- clickhouse inserts: ~2-5 mbps sustained (compressed)
- metrics/monitoring: negligible (<1 mbps)

**monitoring**:
- prometheus retention: 30 days
- grafana alerts: high latency (p95 >100ms), circuit breaker trips
- log rotation: 7 days, max 10gb

---

## performance characteristics

### benchmark results (measured)

**throughput (historical backfill)**:
- blocks processed: 10,000
- transactions processed: 100,000
- elapsed time: 1.40 seconds
- **throughput: 7,159 blocks/sec, 71,592 tx/sec**

**latency (real-time sync)**:
- test blocks: 100 (50 tx each)
- **p50 (median): 4.08ms**
- **p95: 5.69ms**
- **p99: 6.30ms**
- **max: 6.30ms**

### performance tuning parameters

**buffer_size**:
- default: 10,000 rows
- high throughput: 50,000 rows (increases memory, reduces write frequency)
- low latency: 1,000 rows (reduces memory, increases write frequency)

**flush_interval_ms**:
- default: 100ms
- real-time priority: 50ms (more frequent flushes, lower latency)
- batch priority: 500ms (fewer flushes, higher throughput)

**clickhouse tuning**:
```sql
-- increase max insert block size
SET max_insert_block_size = 1048576;

-- enable compression
SET compression = 'lz4';

-- optimize partition merges
SET optimize_on_insert = 1;
```

### scalability limits

**theoretical maximum throughput**:
- limited by clickhouse insert latency (~3-5ms average)
- max sustainable: ~10,000-15,000 blocks/sec
- ethereum mainnet produces ~7.5 blocks/sec (well within capacity)

**memory usage**:
- base: ~500 mb (shadow-index process)
- buffer overhead: ~1 mb per 10,000 rows
- reth overhead: ~8-16 gb (blockchain state)

**cpu usage**:
- idle (no new blocks): <1% single core
- real-time sync: ~10-15% (2-3 cores)
- historical backfill: ~80-100% (all cores, parallel processing)

---

## testing

### test organization

```
tests/
├── benchmark.rs           # integration benchmarks (throughput, latency)
└── integration_tests/
    ├── db_tests.rs       # clickhouse integration tests
    ├── cursor_tests.rs   # cursor manager tests
    └── transform_tests.rs # transform engine tests
```

### unit tests

located in `src/**/*.rs` files, run with:

```bash
cargo test --lib
```

**coverage**:
- config validation: 100%
- cursor operations: 100%
- transform functions: 95%
- error handling: 90%

### integration tests

use testcontainers for isolated clickhouse instances:

```bash
cargo test --test integration_tests
```

**test scenarios**:
- schema creation and migration
- insert and query operations
- collapsingmergetree behavior (sign=-1)
- cursor recovery after crash
- concurrent writes

### benchmark tests

measure end-to-end performance:

```bash
cargo test --test benchmark benchmark_throughput -- --ignored --nocapture
cargo test --test benchmark benchmark_latency -- --ignored --nocapture
```

**benchmark configuration**:
- throughput: 10,000 blocks, 10 tx/block
- latency: 100 blocks, 50 tx/block, immediate flush

### continuous integration

github actions workflow:

```yaml
jobs:
  test:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v3
      - uses: actions-rs/toolchain@v1
        with:
          toolchain: 1.85
      - run: cargo test --all-features
      - run: cargo clippy -- -D warnings
      - run: cargo fmt -- --check
```

---

## development workflow

### local development setup

```bash
# 1. install rust 1.85+
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# 2. clone repository
git clone https://github.com/bit2swaz/shadow-index.git
cd shadow-index

# 3. start clickhouse
docker run -d -p 8123:8123 -p 9000:9000 clickhouse/clickhouse-server:23.8

# 4. initialize cursor
echo "0" > shadow-index.cursor

# 5. build and run
cargo build --release
cargo run --release
```

### code style guidelines

**formatting**:
```bash
cargo fmt
```

**linting**:
```bash
cargo clippy -- -D warnings
```

**conventions**:
- use snake_case for variables and functions
- use pascalcase for types and traits
- max line length: 100 characters
- prefer explicit types over inference in public apis
- add documentation comments for public interfaces

### git workflow

```bash
# 1. create feature branch
git checkout -b feature/add-reorg-metrics

# 2. make changes, test locally
cargo test

# 3. commit with descriptive message
git commit -m "feat: add prometheus metric for reorg depth"

# 4. push and create pull request
git push origin feature/add-reorg-metrics
```

### debugging tips

**enable trace logging**:
```bash
RUST_LOG=trace cargo run
```

**use clickhouse query log**:
```sql
SELECT query, query_duration_ms
FROM system.query_log
WHERE type = 'QueryFinish'
ORDER BY event_time DESC
LIMIT 10;
```

**inspect cursor state**:
```bash
cat shadow-index.cursor
```

**test clickhouse connection**:
```bash
curl http://localhost:8123
# should return "Ok."
```

---

## dependencies

### core dependencies

**async runtime**:
- `tokio = "1.36"`: async runtime with work-stealing scheduler
- `futures = "0.3"`: async utilities and combinators
- `async-trait = "0.1"`: async trait support

**database**:
- `clickhouse = "0.11"`: native protocol client for clickhouse
- `serde = "1.0"`: serialization framework
- `serde_json = "1.0"`: json serialization

**configuration**:
- `config = "0.14"`: layered configuration system
- `clap = "4.5"`: command-line argument parser

**ethereum**:
- `reth = { git = "paradigmxyz/reth", branch = "main" }`: ethereum execution client
- `reth-exex = { git = "..." }`: execution extension framework
- `alloy-primitives = "1.5"`: ethereum primitive types
- `revm = "34"`: ethereum virtual machine

**observability**:
- `metrics = "0.22"`: metrics facade
- `metrics-exporter-prometheus = "0.13"`: prometheus exporter
- `tracing = "0.1"`: structured logging
- `tracing-subscriber = "0.3"`: log collection and filtering

**utilities**:
- `eyre = "0.6"`: error handling and reporting
- `hex = "0.4"`: hex encoding/decoding
- `url = "2.5"`: url parsing

### development dependencies

- `testcontainers = "0.15"`: docker containers for integration tests
- `serial_test = "3.0"`: serialize test execution

### version constraints

**critical pinned versions**:
- rust: 1.85+ (required for async traits and reth compatibility)
- clickhouse: 23.8 lts (stable collapsingmergetree implementation)
- reth: main branch (tracking latest exex api changes)

**update policy**:
- patch versions: update freely (backward compatible)
- minor versions: update after testing (may introduce new features)
- major versions: coordinate with reth upgrades (breaking changes)

---

## file structure

```
shadow-index/
├── Cargo.toml                    # rust package manifest
├── Cargo.lock                    # dependency lock file
├── LICENSE                       # mit license
├── README.md                     # project overview
├── PRD.md                        # product requirements document
├── ROADMAP.md                    # development roadmap
├── docker-compose.yml            # development docker setup
├── docker-compose.prod.yml       # production docker setup
├── Dockerfile                    # multi-stage build definition
├── shadow-index.cursor           # cursor persistence file (runtime)
├── config.toml                   # optional configuration file
│
├── src/
│   ├── main.rs                   # entrypoint, metrics initialization
│   ├── lib.rs                    # library root
│   ├── config.rs                 # configuration loading and validation
│   │
│   ├── db/
│   │   ├── mod.rs                # database module exports
│   │   ├── models.rs             # row struct definitions
│   │   ├── writer.rs             # clickhouse writer with retry logic
│   │   └── migrations.rs         # schema migration system
│   │
│   ├── exex/
│   │   ├── mod.rs                # shadowexex main struct
│   │   └── buffer.rs             # composite batcher
│   │
│   ├── transform/
│   │   └── mod.rs                # reth primitive → row transforms
│   │
│   └── utils/
│       └── cursor.rs             # cursor manager
│
├── tests/
│   ├── benchmark.rs              # performance benchmarks
│   └── integration_tests/        # integration test suites
│
├── scripts/
│   ├── mock_metrics_server.py   # test metrics generator
│   └── check_monitoring.sh      # monitoring diagnostic script
│
├── docker/
│   ├── prometheus/
│   │   └── prometheus.yml        # prometheus scrape config
│   │
│   └── grafana/
│       └── provisioning/
│           ├── datasources/
│           │   └── datasource.yml       # prometheus + clickhouse datasources
│           └── dashboards/
│               ├── dashboard.yml        # dashboard provisioning config
│               └── shadow-index.json    # main performance dashboard
│
└── docs/
    ├── ARCHITECTURE.md           # system design deep-dive
    ├── DEPLOYMENT.md             # production deployment guide
    ├── DEVELOPMENT.md            # developer onboarding
    ├── BENCHMARKS.md             # performance metrics and analysis
    ├── API.md                    # sql query interface reference
    ├── MONITORING.md             # observability guide
    └── SSOT.md                   # this document (single source of truth)
```

---

## revision history

| version | date | changes |
|---------|------|---------|
| 0.1.0 | 2026-02-17 | initial ssot document created |

---

**end of single source of truth document**

for questions, issues, or contributions, see the main [README.md](../README.md) or open an issue on github.
