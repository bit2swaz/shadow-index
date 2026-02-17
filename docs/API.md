# query api reference

sql query interface and examples for shadow-index clickhouse tables.

## table of contents

- [overview](#overview)
- [database schema](#database-schema)
- [query examples](#query-examples)
- [performance optimization](#performance-optimization)
- [common patterns](#common-patterns)
- [advanced queries](#advanced-queries)

## overview

shadow-index stores blockchain data in clickhouse using the `CollapsingMergeTree` engine. queries must filter or aggregate using the `sign` column to account for chain reorganizations.

**key principles**:

1. **always filter by sign**: use `WHERE sign = 1` for active rows
2. **use sum(sign) for aggregates**: sum rows by primary key to get net state
3. **optimize with proper indexes**: leverage primary key ordering for range scans
4. **partition pruning**: use block_number ranges to reduce scan size

**note**: shadow-index does not currently expose a rest api or graphql endpoint. all queries are executed directly against the clickhouse database using the native sql interface.

## database schema

### blocks table

```sql
CREATE TABLE shadow_index.blocks (
    block_number UInt64,
    hash FixedString(32),
    parent_hash FixedString(32),
    timestamp DateTime64(3),
    gas_used UInt64,
    gas_limit UInt64,
    base_fee UInt128,
    miner FixedString(20),
    sign Int8
)
ENGINE = CollapsingMergeTree(sign)
ORDER BY (block_number, hash, sign);
```

**primary key**: (block_number, hash, sign)

**columns**:
- `block_number`: sequential block height
- `hash`: keccak256 block hash (32 bytes)
- `parent_hash`: hash of previous block
- `timestamp`: block timestamp with millisecond precision
- `gas_used`: total gas consumed by all transactions
- `gas_limit`: maximum gas allowed in block
- `base_fee`: eip-1559 base fee per gas (wei)
- `miner`: coinbase address that mined the block
- `sign`: +1 for commit, -1 for revert

### transactions table

```sql
CREATE TABLE shadow_index.transactions (
    tx_hash FixedString(32),
    block_number UInt64,
    tx_index UInt32,
    from FixedString(20),
    to Nullable(FixedString(20)),
    value UInt256,
    gas_limit UInt64,
    gas_used UInt64,
    gas_price UInt128,
    nonce UInt64,
    status UInt8,
    sign Int8
)
ENGINE = CollapsingMergeTree(sign)
ORDER BY (block_number, tx_hash, sign);
```

**primary key**: (block_number, tx_hash, sign)

**columns**:
- `tx_hash`: keccak256 transaction hash (32 bytes)
- `block_number`: block containing this transaction
- `tx_index`: position within block (0-indexed)
- `from`: sender address
- `to`: recipient address (null for contract creation)
- `value`: ether transferred (wei)
- `gas_limit`: maximum gas allocated
- `gas_used`: actual gas consumed
- `gas_price`: effective gas price (base_fee + priority_fee)
- `nonce`: sender nonce
- `status`: 1 for success, 0 for revert
- `sign`: +1 for commit, -1 for revert

### logs table

```sql
CREATE TABLE shadow_index.logs (
    block_number UInt64,
    tx_hash FixedString(32),
    log_index UInt32,
    address FixedString(20),
    topic0 FixedString(32),
    topic1 Nullable(FixedString(32)),
    topic2 Nullable(FixedString(32)),
    topic3 Nullable(FixedString(32)),
    data String,
    sign Int8
)
ENGINE = CollapsingMergeTree(sign)
ORDER BY (block_number, tx_hash, log_index, sign);
```

**primary key**: (block_number, tx_hash, log_index, sign)

**columns**:
- `block_number`: block containing this log
- `tx_hash`: transaction that emitted this log
- `log_index`: position within transaction
- `address`: contract address that emitted the log
- `topic0`: event signature (keccak256 of event abi)
- `topic1-3`: indexed event parameters
- `data`: non-indexed event data (hex-encoded)
- `sign`: +1 for commit, -1 for revert

### storage_diffs table

```sql
CREATE TABLE shadow_index.storage_diffs (
    block_number UInt64,
    address FixedString(20),
    slot FixedString(32),
    value FixedString(32),
    sign Int8
)
ENGINE = CollapsingMergeTree(sign)
ORDER BY (block_number, address, slot, sign);
```

**primary key**: (block_number, address, slot, sign)

**columns**:
- `block_number`: block where state changed
- `address`: contract address
- `slot`: storage slot (32 bytes)
- `value`: new storage value (32 bytes)
- `sign`: +1 for commit, -1 for revert

## query examples

### basic queries

#### 1. count total blocks indexed

```sql
SELECT count(*) as total_blocks
FROM shadow_index.blocks
WHERE sign = 1;
```

**expected output**:
```
┌─total_blocks─┐
│      6420183 │
└──────────────┘
```

#### 2. get latest block

```sql
SELECT 
    block_number,
    hex(hash) as block_hash,
    formatDateTime(timestamp, '%Y-%m-%d %H:%i:%s') as block_time,
    gas_used,
    hex(miner) as miner
FROM shadow_index.blocks
WHERE sign = 1
ORDER BY block_number DESC
LIMIT 1;
```

**expected output**:
```
┌─block_number─┬─block_hash──────────────────────────────────────────────────┬─block_time──────────┬─gas_used─┬─miner──────────────────────────────────┐
│      6420183 │ a3f5e8c2d91b4a6789012cdef3456789abcdef0123456789abcdef012 │ 2026-02-15 08:23:45 │ 29847521 │ 95222290dd7278aa3ddd389cc1e1d165cc4bafe5 │
└──────────────┴─────────────────────────────────────────────────────────────┴─────────────────────┴──────────┴────────────────────────────────────────┘
```

#### 3. query specific transaction

```sql
SELECT 
    hex(tx_hash) as transaction,
    block_number,
    hex(from) as sender,
    hex(to) as recipient,
    value / 1e18 as eth_value,
    gas_used,
    status
FROM shadow_index.transactions
WHERE tx_hash = unhex('b6f9de95b3f3c9304bcfb642f348c4c0e8f7a3c2d91b4a6789012cdef3456789')
  AND sign = 1;
```

**expected output**:
```
┌─transaction─────────────────────────────────────────────────────┬─block_number─┬─sender──────────────────────────────────┬─recipient───────────────────────────────┬─eth_value─┬─gas_used─┬─status─┐
│ b6f9de95b3f3c9304bcfb642f348c4c0e8f7a3c2d91b4a6789012cdef3456789 │      6400000 │ 742d35cc6634c0532925a3b844bc9e7fe3ceb1e │ a0b86991c6218b36c1d19d4a2e9eb0ce3606eb48 │    100.50 │    65000 │      1 │
└─────────────────────────────────────────────────────────────────┴──────────────┴─────────────────────────────────────────┴─────────────────────────────────────────┴───────────┴──────────┴────────┘
```

### analytical queries

#### 4. top 10 gas consumers (last 10k blocks)

```sql
SELECT 
    hex(from) as address,
    count(*) as tx_count,
    sum(gas_used) as total_gas,
    sum(gas_used * gas_price) / 1e18 as eth_spent,
    avg(gas_price / 1e9) as avg_gwei_price
FROM shadow_index.transactions
WHERE sign = 1
  AND block_number > (SELECT max(block_number) FROM shadow_index.blocks WHERE sign = 1) - 10000
GROUP BY from
ORDER BY total_gas DESC
LIMIT 10;
```

**expected output**:
```
┌─address──────────────────────────────────────┬─tx_count─┬─total_gas─┬─eth_spent──┬─avg_gwei_price─┐
│ a9d1e08c7793af67e9d92fe308d5697fb81d3e43 │      842 │  17856234 │    2.45678 │          34.21 │
│ 28c6c06298d514db089934071355e5743bf21d60 │      523 │  15234876 │    1.98234 │          32.45 │
│ dac17f958d2ee523a2206206994597c13d831ec7 │      412 │  12983456 │    1.67890 │          31.89 │
└──────────────────────────────────────────────┴──────────┴───────────┴────────────┴────────────────┘
```

#### 5. real-time indexing rate

```sql
SELECT 
    toStartOfMinute(timestamp) as minute,
    count(*) as blocks_indexed,
    sum(gas_used) / 1e6 as million_gas,
    countDistinct(miner) as unique_miners
FROM shadow_index.blocks
WHERE sign = 1
  AND timestamp > now() - INTERVAL 1 HOUR
GROUP BY minute
ORDER BY minute DESC
LIMIT 10;
```

**expected output**:
```
┌─minute──────────────────┬─blocks_indexed─┬─million_gas─┬─unique_miners─┐
│ 2026-02-15 08:23:00     │              5 │       148.5 │             4 │
│ 2026-02-15 08:22:00     │              5 │       142.3 │             5 │
│ 2026-02-15 08:21:00     │              5 │       156.7 │             5 │
└─────────────────────────┴────────────────┴─────────────┴───────────────┘
```

#### 6. contract event logs (erc20 transfers)

```sql
SELECT 
    block_number,
    hex(tx_hash) as transaction,
    hex(address) as token_contract,
    hex(topic1) as from_address,
    hex(topic2) as to_address,
    reinterpretAsUInt256(reverse(unhex(substring(data, 3)))) as amount
FROM shadow_index.logs
WHERE sign = 1
  AND topic0 = unhex('ddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef')  -- Transfer(address,address,uint256)
  AND address = unhex('a0b86991c6218b36c1d19d4a2e9eb0ce3606eb48')  -- usdc
  AND block_number > 6400000
ORDER BY block_number DESC
LIMIT 10;
```

**expected output**:
```
┌─block_number─┬─transaction─────────────────────────────────────────────────┬─token_contract──────────────────────────────┬─from_address────────────────────────────────┬─to_address──────────────────────────────────┬─amount────────┐
│      6420180 │ c3f9de95b3f3c9304bcfb642f348c4c0e8f7a3c2d91b4a678901... │ a0b86991c6218b36c1d19d4a2e9eb0ce3606eb48 │ 000000000000000000000000742d35cc6634c053... │ 00000000000000000000000028c6c06298d514db... │    50000000000 │
│      6420175 │ d4e8bf96a4c4d9405cfd753f459d5d1f9g8b4d3e92c5b789ab2... │ a0b86991c6218b36c1d19d4a2e9eb0ce3606eb48 │ 00000000000000000000000095222290dd7278aa... │ 000000000000000000000000dac17f958d2ee523... │   100000000000 │
└──────────────┴─────────────────────────────────────────────────────────────┴─────────────────────────────────────────────┴─────────────────────────────────────────────┴─────────────────────────────────────────────┴───────────────┘
```

### reorg analysis

#### 7. detect reverted blocks

```sql
SELECT 
    block_number,
    hex(hash) as block_hash,
    sum(sign) as net_sign
FROM shadow_index.blocks
GROUP BY block_number, hash
HAVING net_sign = 0  -- reverted blocks sum to zero
ORDER BY block_number DESC
LIMIT 10;
```

**expected output** (if reorgs occurred):
```
┌─block_number─┬─block_hash──────────────────────────────────────────────────┬─net_sign─┐
│      6419500 │ e5a9b7c3d2f1a8967543210fedcba9876543210fedcba987654321 │        0 │
│      6419499 │ f6b8c8d4e3g2b9a78654321gfedcb0987654321gfedcb0987654321 │        0 │
└──────────────┴─────────────────────────────────────────────────────────────┴──────────┘
```

#### 8. reorg frequency

```sql
SELECT 
    toStartOfDay(timestamp) as date,
    countDistinct(block_number) as reorg_count
FROM shadow_index.blocks
WHERE sign = -1
GROUP BY date
ORDER BY date DESC
LIMIT 30;
```

**expected output**:
```
┌─date────────────┬─reorg_count─┐
│ 2026-02-15      │           3 │
│ 2026-02-14      │           5 │
│ 2026-02-13      │           2 │
└─────────────────┴─────────────┘
```

## performance optimization

### index usage

clickhouse will use the primary key index for queries that filter on leading columns:

**efficient** (uses index):
```sql
-- filters on block_number (first key column)
SELECT * FROM blocks WHERE block_number = 6420000 AND sign = 1;

-- range scan on block_number
SELECT * FROM blocks WHERE block_number BETWEEN 6400000 AND 6420000 AND sign = 1;
```

**inefficient** (full table scan):
```sql
-- filters on non-indexed column
SELECT * FROM blocks WHERE gas_used > 20000000 AND sign = 1;

-- filters on hash without block_number
SELECT * FROM blocks WHERE hash = unhex('a3f5e8c2d91b4a6789012cdef3456789') AND sign = 1;
```

### secondary indexes

add secondary indexes for frequent non-primary-key queries:

```sql
-- create bloom filter index for address lookups
ALTER TABLE shadow_index.transactions 
ADD INDEX idx_from (from) TYPE bloom_filter GRANULARITY 1;

ALTER TABLE shadow_index.logs 
ADD INDEX idx_address (address) TYPE bloom_filter GRANULARITY 1;

-- rebuild table to apply indexes
OPTIMIZE TABLE shadow_index.transactions FINAL;
OPTIMIZE TABLE shadow_index.logs FINAL;
```

### materialized views

pre-compute expensive aggregations:

```sql
-- daily transaction statistics
CREATE MATERIALIZED VIEW shadow_index.daily_tx_stats
ENGINE = SummingMergeTree()
ORDER BY (date, address)
AS SELECT
    toDate(timestamp) as date,
    from as address,
    count(*) as tx_count,
    sum(gas_used) as total_gas,
    sum(value) as total_value
FROM shadow_index.transactions
INNER JOIN shadow_index.blocks USING (block_number)
WHERE transactions.sign = 1 AND blocks.sign = 1
GROUP BY date, address;
```

query the materialized view:

```sql
SELECT 
    date,
    hex(address) as sender,
    sum(tx_count) as transactions,
    sum(total_gas) as gas_used
FROM shadow_index.daily_tx_stats
WHERE date >= today() - 30
GROUP BY date, address
ORDER BY gas_used DESC
LIMIT 100;
```

## common patterns

### pattern 1: aggregate with sign handling

when aggregating, use `sum(sign)` to get net state:

```sql
-- count active blocks (after reorgs)
SELECT count(*) FROM (
    SELECT block_number, sum(sign) as net
    FROM shadow_index.blocks
    GROUP BY block_number
    HAVING net > 0
);
```

### pattern 2: join with sign filtering

join tables with proper sign filtering:

```sql
SELECT 
    b.block_number,
    b.timestamp,
    count(t.tx_hash) as tx_count
FROM shadow_index.blocks b
LEFT JOIN shadow_index.transactions t 
    ON b.block_number = t.block_number 
    AND t.sign = 1
WHERE b.sign = 1
  AND b.block_number BETWEEN 6400000 AND 6410000
GROUP BY b.block_number, b.timestamp
ORDER BY b.block_number;
```

### pattern 3: time-series aggregation

group by time intervals:

```sql
SELECT 
    toStartOfInterval(timestamp, INTERVAL 5 MINUTE) as interval,
    count(*) as blocks,
    avg(gas_used) as avg_gas,
    max(gas_used) as max_gas
FROM shadow_index.blocks
WHERE sign = 1
  AND timestamp > now() - INTERVAL 1 DAY
GROUP BY interval
ORDER BY interval;
```

### pattern 4: contract state tracking

track storage slot changes over time:

```sql
SELECT 
    block_number,
    hex(slot) as storage_slot,
    hex(value) as storage_value
FROM shadow_index.storage_diffs
WHERE sign = 1
  AND address = unhex('a0b86991c6218b36c1d19d4a2e9eb0ce3606eb48')  -- usdc
  AND slot = unhex('0000000000000000000000000000000000000000000000000000000000000000')  -- slot 0
ORDER BY block_number DESC
LIMIT 10;
```

## advanced queries

### query 1: calculate rolling average gas price

```sql
SELECT 
    block_number,
    formatDateTime(timestamp, '%Y-%m-%d %H:%i') as time,
    base_fee / 1e9 as base_fee_gwei,
    avg(base_fee / 1e9) OVER (
        ORDER BY block_number 
        ROWS BETWEEN 99 PRECEDING AND CURRENT ROW
    ) as rolling_avg_100_blocks
FROM shadow_index.blocks
WHERE sign = 1
  AND block_number > 6400000
ORDER BY block_number DESC
LIMIT 100;
```

### query 2: identify smart contract deployments

```sql
SELECT 
    block_number,
    hex(tx_hash) as transaction,
    hex(from) as deployer,
    gas_used,
    formatDateTime(b.timestamp, '%Y-%m-%d %H:%i:%s') as deployed_at
FROM shadow_index.transactions t
INNER JOIN shadow_index.blocks b 
    ON t.block_number = b.block_number 
    AND b.sign = 1
WHERE t.sign = 1
  AND t.to IS NULL  -- contract creation
  AND t.status = 1  -- successful
  AND t.block_number > 6400000
ORDER BY t.block_number DESC
LIMIT 20;
```

### query 3: analyze nft transfers (erc721)

```sql
-- erc721 transfer events: Transfer(address indexed from, address indexed to, uint256 indexed tokenId)
SELECT 
    block_number,
    hex(address) as nft_contract,
    hex(substring(topic1, 13)) as from_address,  -- strip padding
    hex(substring(topic2, 13)) as to_address,
    reinterpretAsUInt256(reverse(topic3)) as token_id,
    count(*) OVER (PARTITION BY address) as total_transfers
FROM shadow_index.logs
WHERE sign = 1
  AND topic0 = unhex('ddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef')
  AND length(topic3) = 32  -- erc721 has indexed tokenId
  AND block_number > 6400000
ORDER BY block_number DESC
LIMIT 50;
```

### query 4: gas efficiency by contract

```sql
SELECT 
    hex(t.to) as contract,
    count(*) as call_count,
    avg(t.gas_used) as avg_gas_per_call,
    sum(t.gas_used * t.gas_price) / 1e18 as total_eth_cost,
    quantile(0.5)(t.gas_used) as median_gas,
    quantile(0.95)(t.gas_used) as p95_gas
FROM shadow_index.transactions t
WHERE t.sign = 1
  AND t.to IS NOT NULL
  AND t.status = 1
  AND t.block_number > (SELECT max(block_number) FROM shadow_index.blocks WHERE sign = 1) - 50000
GROUP BY t.to
HAVING call_count > 100
ORDER BY total_eth_cost DESC
LIMIT 20;
```

### query 5: detect suspicious activity (flash loan pattern)

```sql
-- find transactions that receive and send large amounts in same block
WITH large_receives AS (
    SELECT DISTINCT
        block_number,
        hex(substring(topic2, 13)) as address
    FROM shadow_index.logs
    WHERE sign = 1
      AND topic0 = unhex('ddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef')
      AND reinterpretAsUInt256(reverse(unhex(substring(data, 3)))) > 1000000000000000000000  -- > 1000 tokens
),
large_sends AS (
    SELECT DISTINCT
        block_number,
        hex(substring(topic1, 13)) as address
    FROM shadow_index.logs
    WHERE sign = 1
      AND topic0 = unhex('ddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef')
      AND reinterpretAsUInt256(reverse(unhex(substring(data, 3)))) > 1000000000000000000000
)
SELECT 
    r.block_number,
    r.address,
    count(*) as suspicious_transactions
FROM large_receives r
INNER JOIN large_sends s 
    ON r.block_number = s.block_number 
    AND r.address = s.address
GROUP BY r.block_number, r.address
ORDER BY r.block_number DESC
LIMIT 10;
```

## client libraries

### clickhouse-client (cli)

```bash
# connect to database
clickhouse-client --host localhost --port 9000

# run query from file
clickhouse-client < query.sql

# export to csv
clickhouse-client --query "SELECT * FROM shadow_index.blocks WHERE sign=1 LIMIT 1000" --format CSV > blocks.csv
```

### python (clickhouse-driver)

```python
from clickhouse_driver import Client

client = Client(host='localhost', port=9000)

# execute query
blocks = client.execute(
    'SELECT block_number, hex(hash) as hash FROM shadow_index.blocks WHERE sign = 1 ORDER BY block_number DESC LIMIT 10'
)

for block in blocks:
    print(f"Block {block[0]}: {block[1]}")
```

### rust (clickhouse-rs)

```rust
use clickhouse::Client;

#[tokio::main]
async fn main() -> Result<()> {
    let client = Client::default()
        .with_url("http://localhost:8123")
        .with_database("shadow_index");
    
    let blocks: Vec<(u64, String)> = client
        .query("SELECT block_number, hex(hash) FROM blocks WHERE sign = 1 ORDER BY block_number DESC LIMIT 10")
        .fetch_all()
        .await?;
    
    for (number, hash) in blocks {
        println!("Block {}: {}", number, hash);
    }
    
    Ok(())
}
```

### javascript (clickhouse-client)

```javascript
const { ClickHouse } = require('clickhouse');

const client = new ClickHouse({
  url: 'http://localhost',
  port: 8123,
  database: 'shadow_index'
});

client.query('SELECT block_number, hex(hash) as hash FROM blocks WHERE sign = 1 ORDER BY block_number DESC LIMIT 10')
  .toPromise()
  .then(rows => {
    rows.forEach(row => {
      console.log(`Block ${row.block_number}: ${row.hash}`);
    });
  });
```

---

**last updated**: february 2026  
**version**: 1.0.0-rc1
