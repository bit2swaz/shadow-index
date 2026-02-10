CREATE TABLE IF NOT EXISTS blocks (
    block_number UInt64,
    sign Int8,
    hash FixedString(32),
    parent_hash FixedString(32),
    timestamp DateTime64(3),
    gas_used UInt64,
    base_fee UInt128,
    miner FixedString(20),
    state_root FixedString(32),
    extra_data String
) ENGINE = CollapsingMergeTree(sign)
ORDER BY (block_number, sign)
PRIMARY KEY block_number;

CREATE TABLE IF NOT EXISTS transactions (
    tx_hash FixedString(32),
    block_number UInt64,
    tx_index UInt32,
    sign Int8,
    from FixedString(20),
    to Nullable(FixedString(20)),
    value UInt256,
    input String,
    status UInt8,
    nonce UInt64,
    gas_limit UInt64,
    gas_price UInt128,
    max_fee_per_gas Nullable(UInt128),
    max_priority_fee_per_gas Nullable(UInt128),
    chain_id UInt64
) ENGINE = CollapsingMergeTree(sign)
ORDER BY (block_number, tx_index, sign)
PRIMARY KEY (block_number, tx_hash);

CREATE TABLE IF NOT EXISTS logs (
    block_number UInt64,
    sign Int8,
    tx_hash FixedString(32),
    tx_index UInt32,
    log_index UInt32,
    address FixedString(20),
    topic0 FixedString(32),
    topics Array(FixedString(32)),
    data String
) ENGINE = CollapsingMergeTree(sign)
ORDER BY (block_number, tx_index, log_index, sign)
PRIMARY KEY (block_number, tx_hash, log_index);

CREATE TABLE IF NOT EXISTS storage_diffs (
    block_number UInt64,
    sign Int8,
    address FixedString(20),
    slot FixedString(32),
    value FixedString(32)
) ENGINE = CollapsingMergeTree(sign)
ORDER BY (block_number, address, slot, sign)
PRIMARY KEY (block_number, address, slot);
