# development guide

local development setup and contributor guidelines for shadow-index.

## table of contents

- [getting started](#getting-started)
- [project structure](#project-structure)
- [development workflow](#development-workflow)
- [testing](#testing)
- [debugging](#debugging)
- [contributing](#contributing)
- [code style](#code-style)

## getting started

### prerequisites

**required tools**:

- rust 1.85+ (install via [rustup](https://rustup.rs))
- docker 24.0+ (for integration tests)
- git 2.30+

**recommended tools**:

- [rust-analyzer](https://rust-analyzer.github.io/) (vscode/neovim language server)
- [cargo-watch](https://github.com/watchexec/cargo-watch) (auto-rebuild on file changes)
- [cargo-nextest](https://nexte.st/) (faster test runner)
- [bacon](https://github.com/Canop/bacon) (background code checker)

### initial setup

```bash
# 1. clone repository
git clone https://github.com/bit2swaz/shadow-index.git
cd shadow-index

# 2. install rust toolchain
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
rustup default stable
rustup component add clippy rustfmt

# 3. install cargo tools
cargo install cargo-watch cargo-nextest bacon

# 4. verify installation
rustc --version
cargo --version
docker --version

# 5. build project
cargo build

# 6. run unit tests
cargo test --lib

# 7. start development environment
docker-compose up -d clickhouse
```

### editor configuration

**vscode settings** (`.vscode/settings.json`):

```json
{
  "rust-analyzer.cargo.features": "all",
  "rust-analyzer.checkOnSave.command": "clippy",
  "rust-analyzer.inlayHints.typeHints.enable": true,
  "rust-analyzer.inlayHints.parameterHints.enable": true,
  "editor.formatOnSave": true,
  "[rust]": {
    "editor.defaultFormatter": "rust-lang.rust-analyzer"
  }
}
```

**neovim configuration** (lua):

```lua
require('lspconfig').rust_analyzer.setup({
  settings = {
    ['rust-analyzer'] = {
      cargo = { features = "all" },
      checkOnSave = { command = "clippy" },
    }
  }
})
```

## project structure

```
shadow-index/
├── Cargo.toml                  # workspace manifest
├── Cargo.lock                  # dependency lockfile
├── src/
│   ├── main.rs                 # binary entry point
│   ├── lib.rs                  # library root
│   ├── config.rs               # configuration system
│   ├── db/
│   │   ├── mod.rs              # database module root
│   │   ├── models.rs           # row structs (BlockRow, TransactionRow, etc.)
│   │   ├── writer.rs           # clickhouse batch writer with retry logic
│   │   └── migrations.rs       # schema migration runner
│   ├── exex/
│   │   ├── mod.rs              # main exex loop and orchestration
│   │   └── buffer.rs           # composite batcher with flush logic
│   ├── transform/
│   │   ├── mod.rs              # block/transaction transforms
│   │   └── state.rs            # storage diff extraction
│   └── utils/
│       └── cursor.rs           # checkpoint persistence
├── crates/
│   └── testing/
│       └── src/
│           └── lib.rs          # testcontainer helpers
├── docs/
│   ├── ARCHITECTURE.md         # system design deep-dive
│   ├── DEPLOYMENT.md           # production deployment guide
│   ├── DEVELOPMENT.md          # this file
│   ├── BENCHMARKS.md           # performance metrics
│   └── API.md                  # sql query reference
├── docker/
│   ├── Dockerfile              # multi-stage build
│   ├── docker-compose.yml      # dev environment
│   └── docker-compose.prod.yml # production stack
├── scripts/
│   ├── deploy.sh               # deployment automation
│   └── test.sh                 # test runner
└── tests/
    └── integration/            # end-to-end tests (ignored by default)
```

### module responsibilities

#### src/main.rs

entry point for the binary. handles:
- command line argument parsing
- configuration loading
- reth node initialization
- exex registration

**key function**:

```rust
#[tokio::main]
async fn main() -> Result<()> {
    // load configuration
    let config = config::AppConfig::load()
        .expect("failed to load configuration");
    
    // initialize tracing
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .init();
    
    // create database client
    let db_client = db::create_client_from_config(&config.clickhouse);
    
    // initialize cursor
    let cursor = cursor::CursorManager::new(&config.cursor.file_path)?;
    
    // start reth node with shadow exex
    reth::cli::Cli::parse()
        .run(|builder, _| async move {
            let handle = builder
                .node(EthereumNode::default())
                .install_exex("shadow-index", |ctx| async move {
                    Ok(exex::ShadowExEx::new(ctx, db_client, cursor))
                })
                .launch()
                .await?;
            
            handle.wait_for_node_exit().await
        })
        .await
}
```

#### src/config.rs

layered configuration system using the `config` crate:

```rust
#[derive(Debug, Deserialize, Clone)]
pub struct AppConfig {
    pub clickhouse: ClickHouseConfig,
    pub exex: ExExConfig,
    pub backfill: BackfillConfig,
    pub cursor: CursorConfig,
}

impl AppConfig {
    pub fn load() -> Result<Self, ConfigError> {
        Config::builder()
            .set_default("clickhouse.url", "http://localhost:8123")?
            .set_default("exex.buffer_size", 10000)?
            .add_source(File::with_name("config.toml").required(false))
            .add_source(Environment::with_prefix("SHADOW_INDEX").separator("__"))
            .build()?
            .try_deserialize()
    }
}
```

#### src/db/writer.rs

handles all clickhouse interactions:

```rust
pub struct ClickHouseWriter {
    client: Client,
}

impl ClickHouseWriter {
    pub async fn insert_batch<T>(&self, table: &str, rows: &[T]) -> Result<()>
    where
        T: Serialize,
    {
        // retry logic with exponential backoff
        // error discrimination (permanent vs transient)
        // prometheus metrics emission
    }
}
```

#### src/exex/mod.rs

main orchestration logic:

```rust
pub struct ShadowExEx {
    ctx: ExExContext<Node>,
    writer: ClickHouseWriter,
    cursor: CursorManager,
    batcher: Batcher,
}

impl Future for ShadowExEx {
    // processes CanonStateNotification stream
    // coordinates transforms, buffering, and writes
}
```

#### src/transform/mod.rs

pure functions for data extraction:

```rust
pub fn transform_block(
    block: &SealedBlock,
    receipts: &[Receipt],
    sign: i8
) -> TransformResult {
    // convert reth types to row structs
    // normalize addresses/hashes
    // apply sign for collapsingmergetree
}
```

## development workflow

### typical development cycle

```bash
# 1. create feature branch
git checkout -b feature/add-websocket-api

# 2. start auto-rebuild watcher
cargo watch -x check -x test -x run

# 3. make changes to src/
vim src/api/websocket.rs

# 4. watcher automatically runs:
#    - cargo check (compile errors)
#    - cargo test (unit tests)
#    - cargo run (starts service)

# 5. run integration tests manually
cargo test --lib --ignored -- --nocapture

# 6. lint and format
cargo clippy --all-targets --all-features
cargo fmt

# 7. commit changes
git add .
git commit -m "feat: add websocket subscription api"

# 8. push and create pr
git push origin feature/add-websocket-api
```

### building

```bash
# debug build (fast compile, slow runtime)
cargo build

# release build (slow compile, optimized runtime)
cargo build --release

# build specific binary
cargo build --bin shadow-index

# check without building binary
cargo check

# build with all features
cargo build --all-features

# cross-compilation for production linux
cargo build --release --target x86_64-unknown-linux-gnu
```

### running locally

**option 1: standalone (requires manual clickhouse setup)**

```bash
# start clickhouse in docker
docker-compose up -d clickhouse

# wait for healthcheck
sleep 5

# run shadow-index with sepolia testnet
export CLICKHOUSE_URL="http://localhost:8123"
cargo run -- node --chain sepolia --datadir ./data/reth
```

**option 2: full stack via docker-compose**

```bash
# rebuild shadow-index image after code changes
docker-compose build shadow-index

# start all services
docker-compose up -d

# view logs
docker-compose logs -f shadow-index
```

**option 3: hybrid (compiled binary + docker clickhouse)**

```bash
# start just clickhouse
docker-compose up -d clickhouse

# run locally compiled binary
cargo run --release -- node --chain sepolia
```

## testing

### test organization

shadow-index has two test categories:

1. **unit tests** (42 tests): fast, no external dependencies
2. **integration tests** (11 tests): use testcontainers to spin up clickhouse

### running tests

```bash
# run unit tests only (default)
cargo test --lib

# run integration tests (requires docker)
cargo test --lib --ignored

# run all tests
cargo test --lib -- --include-ignored

# run specific test
cargo test --lib test_migration_runner -- --ignored --nocapture

# run tests with output (even on success)
cargo test --lib -- --nocapture

# run tests in parallel (faster)
cargo nextest run

# run tests with coverage
cargo tarpaulin --out html --output-dir coverage/
```

### test structure

**unit test example** (no external dependencies):

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_buffer_should_flush_on_size() {
        let mut batcher = Batcher::with_thresholds(10, Duration::from_secs(100));
        
        // push 11 rows (exceeds threshold of 10)
        for _ in 0..11 {
            batcher.push_block(BlockRow::default());
        }
        
        assert!(batcher.should_flush());
    }
    
    #[test]
    fn test_buffer_should_flush_on_time() {
        let mut batcher = Batcher::with_thresholds(10000, Duration::from_millis(50));
        
        batcher.push_block(BlockRow::default());
        
        // should not flush immediately
        assert!(!batcher.should_flush());
        
        // wait for time threshold
        std::thread::sleep(Duration::from_millis(60));
        
        // should flush after timeout
        assert!(batcher.should_flush());
    }
}
```

**integration test example** (uses testcontainers):

```rust
#[cfg(test)]
mod integration {
    use shadow_index_testing::ClickHouseContainer;

    #[tokio::test]
    #[ignore]  // requires docker
    async fn test_full_pipeline() {
        // start isolated clickhouse container
        let container = ClickHouseContainer::start().await;
        let client = container.client();
        
        // run schema migrations
        let migrations = vec![
            include_str!("../migrations/001_create_blocks_table.sql"),
            include_str!("../migrations/002_create_transactions_table.sql"),
        ];
        
        for (idx, sql) in migrations.iter().enumerate() {
            client.query(sql).execute().await.unwrap();
        }
        
        // create writer and insert test data
        let writer = ClickHouseWriter::new(client);
        let rows = vec![
            BlockRow {
                block_number: 1,
                hash: [0u8; 32],
                sign: 1,
                ..Default::default()
            }
        ];
        
        writer.insert_batch("blocks", &rows).await.unwrap();
        
        // verify insertion
        let count: u64 = client
            .query("SELECT count(*) FROM blocks WHERE sign = 1")
            .fetch_one()
            .await
            .unwrap();
        
        assert_eq!(count, 1);
    }
}
```

### testcontainer setup

**crates/testing/src/lib.rs**:

```rust
use testcontainers::{clients::Cli, Container, Image, RunnableImage};

pub struct ClickHouse;

impl Image for ClickHouse {
    type Args = ();
    
    fn name(&self) -> String {
        "clickhouse/clickhouse-server".to_string()
    }
    
    fn tag(&self) -> String {
        "23.8".to_string()
    }
    
    fn ready_conditions(&self) -> Vec<testcontainers::core::WaitFor> {
        vec![testcontainers::core::WaitFor::message_on_stdout("Ready for connections")]
    }
}

pub struct ClickHouseContainer<'a> {
    container: Container<'a, ClickHouse>,
}

impl<'a> ClickHouseContainer<'a> {
    pub async fn start() -> Self {
        let docker = Cli::default();
        let image = RunnableImage::from(ClickHouse)
            .with_mapped_port((8123, 8123));
        let container = docker.run(image);
        
        // wait for ready
        tokio::time::sleep(Duration::from_secs(2)).await;
        
        Self { container }
    }
    
    pub fn client(&self) -> clickhouse::Client {
        clickhouse::Client::default()
            .with_url("http://localhost:8123")
    }
}
```

### continuous integration

**github actions workflow** (`.github/workflows/ci.yml`):

```yaml
name: ci

on: [push, pull_request]

jobs:
  test:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v3
      
      - name: install rust
        uses: actions-rs/toolchain@v1
        with:
          toolchain: stable
          components: clippy, rustfmt
      
      - name: cache dependencies
        uses: actions/cache@v3
        with:
          path: |
            ~/.cargo/registry
            ~/.cargo/git
            target/
          key: ${{ runner.os }}-cargo-${{ hashFiles('**/Cargo.lock') }}
      
      - name: check formatting
        run: cargo fmt -- --check
      
      - name: run clippy
        run: cargo clippy --all-targets --all-features -- -D warnings
      
      - name: run unit tests
        run: cargo test --lib
      
      - name: run integration tests
        run: cargo test --lib --ignored
      
      - name: build release
        run: cargo build --release
```

## debugging

### logging

shadow-index uses the `tracing` crate for structured logging:

```bash
# set log level
export RUST_LOG=shadow_index=debug,reth=info

# enable specific modules
export RUST_LOG=shadow_index::db=trace

# json output for log aggregation
export RUST_LOG_FORMAT=json

# run with verbose logging
cargo run
```

**adding instrumentation**:

```rust
use tracing::{info, warn, error, debug, trace};

#[tracing::instrument(skip(self))]
async fn flush_batch(&mut self) -> Result<()> {
    let start = Instant::now();
    
    debug!(
        blocks = self.batcher.blocks.len(),
        txs = self.batcher.transactions.len(),
        "flushing batch"
    );
    
    self.writer.insert_batch("blocks", &self.batcher.blocks).await?;
    
    info!(
        elapsed_ms = start.elapsed().as_millis(),
        "batch flushed successfully"
    );
    
    Ok(())
}
```

### interactive debugging

**using lldb (macos/linux)**:

```bash
# build with debug symbols
cargo build

# launch debugger
rust-lldb target/debug/shadow-index

# set breakpoints
(lldb) breakpoint set --name shadow_index::exex::flush_batch
(lldb) breakpoint set --file src/exex/mod.rs --line 123

# run
(lldb) run node --chain sepolia

# inspect variables when breakpoint hits
(lldb) frame variable
(lldb) print self.batcher.blocks.len()
```

**using vscode debugger**:

```json
// .vscode/launch.json
{
  "version": "0.2.0",
  "configurations": [
    {
      "type": "lldb",
      "request": "launch",
      "name": "debug shadow-index",
      "cargo": {
        "args": ["build", "--bin=shadow-index"]
      },
      "args": ["node", "--chain", "sepolia"],
      "cwd": "${workspaceFolder}",
      "env": {
        "RUST_LOG": "shadow_index=debug",
        "CLICKHOUSE_URL": "http://localhost:8123"
      }
    }
  ]
}
```

### profiling

**cpu profiling with flamegraph**:

```bash
# install perf (linux) or instruments (macos)
cargo install flamegraph

# record profile
cargo flamegraph -- node --chain sepolia

# open flamegraph.svg in browser
firefox flamegraph.svg
```

**memory profiling with valgrind**:

```bash
# build with debug info
cargo build

# run with massif (heap profiler)
valgrind --tool=massif target/debug/shadow-index node --chain sepolia

# analyze results
ms_print massif.out.<pid>
```

## contributing

### branching strategy

- `main`: stable release branch
- `develop`: integration branch for features
- `feature/*`: new features
- `fix/*`: bug fixes
- `docs/*`: documentation updates

### commit message format

follow [conventional commits](https://www.conventionalcommits.org/):

```
<type>(<scope>): <subject>

<body>

<footer>
```

**types**: feat, fix, docs, style, refactor, test, chore

**examples**:

```
feat(exex): add websocket subscription api

implement real-time event streaming using tokio-tungstenite.
clients can subscribe to specific contract addresses and receive
logs as they are indexed.

closes #42
```

```
fix(db): handle clickhouse connection pool exhaustion

retry with exponential backoff when connection pool is full.
increases circuit breaker threshold to 10 attempts.

fixes #38
```

### pull request checklist

before submitting a pr:

- [ ] code compiles without warnings (`cargo clippy`)
- [ ] code is formatted (`cargo fmt`)
- [ ] unit tests pass (`cargo test --lib`)
- [ ] integration tests pass (`cargo test --lib --ignored`)
- [ ] documentation updated (if applicable)
- [ ] changelog entry added (if user-facing change)
- [ ] commit messages follow conventional commits format

### code review process

1. create pull request with detailed description
2. automated ci checks run (tests, lints, formatting)
3. maintainer review (1-2 business days)
4. address feedback and push updates
5. final approval and merge to develop
6. periodic releases from develop to main

## code style

### rust idioms

**prefer explicit error handling**:

```rust
// bad: unwrap can panic
let block = provider.block_by_number(123).unwrap();

// good: propagate errors
let block = provider.block_by_number(123)?;
```

**use type aliases for clarity**:

```rust
// bad: naked types
fn process(data: Vec<Vec<u8>>) -> Result<Vec<Vec<u8>>>

// good: named types
type Rows = Vec<Vec<u8>>;
fn process(data: Rows) -> Result<Rows>
```

**prefer iterators over loops**:

```rust
// bad: manual loop
let mut result = Vec::new();
for tx in transactions {
    if tx.value > 0 {
        result.push(tx);
    }
}

// good: iterator chain
let result: Vec<_> = transactions
    .into_iter()
    .filter(|tx| tx.value > 0)
    .collect();
```

### documentation

**document public apis with examples**:

```rust
/// inserts a batch of rows into the specified clickhouse table.
///
/// # arguments
///
/// * `table` - the target table name (e.g., "blocks")
/// * `rows` - slice of serializable rows to insert
///
/// # errors
///
/// returns an error if:
/// - clickhouse connection fails
/// - table does not exist
/// - rows fail serialization
///
/// # example
///
/// ```
/// let writer = ClickHouseWriter::new(client);
/// let rows = vec![BlockRow { block_number: 1, sign: 1, ..Default::default() }];
/// writer.insert_batch("blocks", &rows).await?;
/// ```
pub async fn insert_batch<T>(&self, table: &str, rows: &[T]) -> Result<()>
where
    T: Serialize,
{
    // implementation
}
```

### performance guidelines

1. **avoid allocations in hot paths**: use `&str` instead of `String`, `&[T]` instead of `Vec<T>`
2. **batch operations**: group database writes to minimize network round-trips
3. **use async for io-bound work**: network calls, disk reads
4. **use rayon for cpu-bound work**: parallel transformation of large datasets
5. **profile before optimizing**: measure with flamegraph/criterion before micro-optimizations

### error handling

**use `eyre` for application errors**:

```rust
use eyre::{Result, eyre, WrapErr};

fn process_block(block: &Block) -> Result<()> {
    let receipts = fetch_receipts(block.hash)
        .wrap_err("failed to fetch receipts")?;
    
    if receipts.is_empty() {
        return Err(eyre!("block has no receipts"));
    }
    
    Ok(())
}
```

**define custom error types for libraries**:

```rust
#[derive(Debug, thiserror::Error)]
pub enum TransformError {
    #[error("invalid block: {0}")]
    InvalidBlock(String),
    
    #[error("missing receipt for tx {0}")]
    MissingReceipt(TxHash),
}
```

---

**last updated**: february 2026  
**version**: 1.0.0-rc1
