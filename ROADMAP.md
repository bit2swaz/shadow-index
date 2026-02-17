### **Shadow-Index: Master Roadmap**

#### **Phase 1: The "Hello World" ExEx**

**Goal:** Get a compiled binary that runs a Reth node and prints "Commit" or "Revert" to stdout when blocks are processed.

* **Mini-Phase 1.1: The Boilerplate**
* **Context:** We need a binary entry point that initializes a Reth node using the `node` builder and attaches our custom `ShadowExEx`.
* **Task:** Create `main.rs` and a struct `ShadowExEx` that implements `Future`.
* **AI Prompt:**
> "I need to set up the entry point for a Reth ExEx.
> 1. Create a `ShadowExEx` struct in `src/exex/mod.rs` that implements the `Future` trait. For now, the `poll` method should just return `Poll::Pending`.
> 2. In `src/main.rs`, use `reth::cli::Cli` to parse arguments.
> 3. Use `reth::node::NodeBuilder` to launch an Ethereum node.
> 4. Inject `ShadowExEx` using the `.install_exex("shadow-index", ...)` method.
> 5. Ensure the code compiles and the binary runs `reth --help` successfully."
> 
> 




* **Mini-Phase 1.2: The Notification Loop**
* **Context:** The ExEx receives a stream of notifications. We need to handle them.
* **Task:** Implement the stream consumption logic in `ShadowExEx`.
* **AI Prompt:**
> "Modify `ShadowExEx` to accept a `reth_exex::ExExContext`.
> 1. In the `Future` implementation, create a loop that polls `ctx.notifications`.
> 2. Match on the notification type: `ChainCommitted` and `ChainReverted`.
> 3. For `ChainCommitted`, log 'Committed block {number}'.
> 4. For `ChainReverted`, log 'Reverted block {number}'.
> 5. **TDD Requirement:** Create a unit test in `src/exex/mod.rs` that constructs a `ShadowExEx`, feeds it a mock `ChainCommitted` notification using a channel, and asserts that the future processes it."
> 
> 





---

#### **Phase 2: The ClickHouse Infrastructure**

**Goal:** Establish a robust connection to ClickHouse and define the schema using `CollapsingMergeTree`.

* **Mini-Phase 2.1: Docker & Testcontainers**
* **Context:** We need a real DB for integration tests.
* **Task:** Set up `testcontainers` to spin up ClickHouse.
* **AI Prompt:**
> "I need a testing harness for ClickHouse.
> 1. Create `tests/helpers/mod.rs`.
> 2. Implement a function `spawn_clickhouse()` that uses the `testcontainers` crate to run the official ClickHouse image.
> 3. It should return the mapped port and the HTTP URL.
> 4. Write a test in `src/db/mod.rs` that spawns the container, connects using the `clickhouse` crate, and executes 'SELECT 1'."
> 
> 




* **Mini-Phase 2.2: Schema Migration**
* **Context:** We need tables for `blocks`, `transactions`, `logs`, and `storage_diffs` using the `sign` logic.
* **Task:** Define the DDL (Data Definition Language).
* **AI Prompt:**
> "Define the SQL schema for Shadow-Index.
> 1. Create a file `src/db/schema.sql` containing `CREATE TABLE` statements.
> 2. Define 4 tables: `blocks`, `transactions`, `logs`, `storage_diffs`.
> 3. All tables must use `ENGINE = CollapsingMergeTree(sign)`.
> 4. `sign` is `Int8`.
> 5. Use appropriate sorting keys (e.g., `(block_number, tx_index, sign)`).
> 6. Create a Rust struct `SchemaManager` that reads this file and runs the queries against the DB on startup."
> 
> 




* **Mini-Phase 2.3: The Async Writer**
* **Context:** We need a struct that accepts rows and inserts them.
* **Task:** Implement `ClickHouseWriter`.
* **AI Prompt:**
> "Implement the DB Writer.
> 1. Create struct `ClickHouseWriter` in `src/db/writer.rs`.
> 2. It should hold a `clickhouse::Client`.
> 3. Implement a generic method `insert_batch<T: Row>(&self, table: &str, rows: &[T])`.
> 4. **TDD:** Write an integration test that spawns the DB (Phase 2.1), creates tables (Phase 2.2), inserts 10 dummy rows, and queries them back to verify count."
> 
> 





---

#### **Phase 3: The Data Pipeline (ETL)**

**Goal:** Transform Reth internal types into ClickHouse rows and flush them efficiently.

* **Mini-Phase 3.1: Data Structures (DTOs)**
* **Context:** Define Rust structs that map 1:1 to the ClickHouse tables.
* **Task:** Create structs with `#[derive(Row, Serialize)]`.
* **AI Prompt:**
> "Create Data Transfer Objects (DTOs) in `src/db/models.rs`.
> 1. Create structs: `BlockRow`, `TransactionRow`, `LogRow`.
> 2. Ensure types match ClickHouse (e.g., `u64` for block number, `Vec<u8>` for raw bytes/hashes).
> 3. Add the `sign: i8` field to every struct.
> 4. Implement `derive(serde::Serialize, clickhouse::Row)` for all."
> 
> 




* **Mini-Phase 3.2: The Transformer**
* **Context:** Convert `reth_primitives::Block` -> `BlockRow`, etc.
* **Task:** Implement transformation logic.
* **AI Prompt:**
> "Implement the transformation logic in `src/transform/mod.rs`.
> 1. Create a function `transform_block(block: &SealedBlock, sign: i8) -> (BlockRow, Vec<TransactionRow>)`.
> 2. Create a function `transform_logs(receipts: &[Receipt], sign: i8) -> Vec<LogRow>`.
> 3. Use `alloy_primitives` to handle type conversions (Address to [u8; 20]).
> 4. **TDD:** Write unit tests with a mock `SealedBlock` containing 2 txs. Verify the output vectors contain the correct data and sign."
> 
> 




* **Mini-Phase 3.3: The Buffered Pipeline**
* **Context:** We need the Composite Batcher (Time OR Size).
* **Task:** Implement the buffering logic.
* **AI Prompt:**
> "Implement a robust buffering system in `src/exex/buffer.rs`.
> 1. Create a `Batcher` struct that holds a `Vec<BlockRow>`, `Vec<TransactionRow>`, etc.
> 2. Implement `push(&mut self, data...)`.
> 3. Implement `should_flush(&self) -> bool` which returns true if `size >= 10_000` OR `last_flush_time > 100ms`.
> 4. **TDD:** Write a test that simulates pushing 10k rows and verifies `should_flush` is true. Write another that waits 101ms and verifies `should_flush` is true."
> 
> 





---

#### **Phase 4: The "Shadow" (State Diffs)**

**Goal:** Extract storage slot changes from the Bundle State.

* **Mini-Phase 4.1: Bundle Extraction**
* **Context:** `ChainCommitted` contains `BundleState`. We need to iterate it.
* **Task:** Extract storage diffs.
* **AI Prompt:**
> "Implement State Diff extraction in `src/transform/state.rs`.
> 1. Create `StorageDiffRow { address, slot, value, sign }`.
> 2. Write a function `transform_state(bundle: &BundleState, sign: i8) -> Vec<StorageDiffRow>`.
> 3. Iterate over `bundle.storage`. Note that `BundleState` is complex; you need to capture both 'original' and 'new' values to determine the diff.
> 4. **TDD:** Construct a `BundleState` with one storage update. Verify the output `StorageDiffRow` matches."
> 
> 





---

#### **Phase 5: Glue & Production**

**Goal:** Connect the ExEx loop to the Transformer and Writer, and add metrics.

* **Mini-Phase 5.1: The Grand Integration**
* **Context:** Connect the loop from Phase 1 to the logic in Phases 2, 3, and 4.
* **Task:** Update `ShadowExEx` loop.
* **AI Prompt:**
> "Integrate all components in `src/exex/mod.rs`.
> 1. In the `ChainCommitted` match arm:
> * Call `transform_block`, `transform_logs`, `transform_state` with `sign = 1`.
> * Push to `Batcher`.
> * If `Batcher.should_flush()`, call `writer.insert_batch` and reset batcher.
> 
> 
> 2. In the `ChainReverted` match arm:
> * Do the same but with `sign = -1`.
> 
> 
> 3. Handle errors: If `writer` fails, the ExEx should return `Poll::Ready(Err)`.
> 4. **TDD:** Integration test with `testcontainers`. Send a mock block. Verify it appears in ClickHouse."
> 
> 




* **Mini-Phase 5.2: Docker Compose**
* **Context:** Run it all together.
* **Task:** Create `docker-compose.yml`.
* **AI Prompt:**
> "Create a `docker-compose.yml` file.
> 1. Service `clickhouse`: Use official image, expose ports 8123/9000.
> 2. Service `shadow-index`: Build from local Dockerfile.
> 3. Pass `CLICKHOUSE_URL` env var.
> 4. Create a `Dockerfile` that builds the Rust binary (use a multi-stage build with `chef` or standard `cargo build --release`)."
> 
> 





---

#### **Phase 6: The "Ops" Layer (Resilience & Monitoring)**

**Goal:** Ensure the system is observable and fails safely.

* **Mini-Phase 6.1: The Circuit Breaker**
* **Context:** Stop the node if the DB fails to prevent data loss.
* **Task:** Implement retries and panic-on-failure.
* **AI Prompt:**
> "Implement a 'Circuit Breaker' in `src/exex/mod.rs`.
> 1. Wrap the `writer.insert_batch` call in a retry loop (3 retries with exponential backoff).
> 2. If all retries fail, return `Poll::Ready(Err(eyre!('CRITICAL: ClickHouse unreachable'))`.
> 3. This will cause the Reth node to shut down gracefully."
> 
> 




* **Mini-Phase 6.2: Metrics**
* **Context:** Expose Prometheus metrics.
* **Task:** Add metrics to the loop.
* **AI Prompt:**
> "Add Prometheus metrics using the `metrics` crate.
> 1. Register: `shadow_blocks_processed`, `shadow_buffer_size`, `shadow_db_latency`.
> 2. Update these inside the `ShadowExEx` loop."
> 
> 





---

#### **Phase 7: The Historical Backfill**

**Goal:** Populate the database with past data.

* **Mini-Phase 7.1: The Cursor**
* **Context:** Resume from where we left off.
* **Task:** Implement local cursor file management.
* **AI Prompt:**
> "Implement a `CursorManager` in `src/utils/cursor.rs`.
> 1. On successful flush, write `block_number` to `shadow-index.cursor` (atomic write).
> 2. On startup, read this file to determine the starting block."
> 
> 




* **Mini-Phase 7.2: Backfill Mode**
* **Context:** Fetch old blocks if `cursor < head`.
* **Task:** Implement backfill logic.
* **AI Prompt:**
> "Implement 'Backfill Mode'.
> 1. On startup, if `node_head > cursor`, enter Catchup Mode.
> 2. Use `reth_provider` to fetch historical blocks.
> 3. Feed them into the `transform -> buffer -> write` pipeline.
> 4. Log progress."
> 
>   





---

#### **Phase 8: Maintenance**

**Goal:** Handle schema changes.

* **Mini-Phase 8.1: Migrations**
* **Context:** Apply SQL updates on startup.
* **Task:** Implement a migration runner.
* **AI Prompt:**
> "Implement a Migration Runner in `src/db/migrations.rs`.
> 1. Define a list of SQL strings.
> 2. Check `schema_version` table in DB.
> 3. Apply missing migrations sequentially on startup."
> 
> 