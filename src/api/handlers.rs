use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Json, Response},
};
use serde::{Deserialize, Serialize};
use serde_json::json;

use super::state::AppState;

/// Health check response
#[derive(Serialize)]
pub struct HealthResponse {
    status: String,
}

/// Block data from database
#[derive(Debug, Serialize, Deserialize, clickhouse::Row)]
pub struct BlockRow {
    pub block_number: u64,
    pub hash: String,
    pub parent_hash: String,
    pub timestamp: i64,
    pub gas_used: u64,
    pub base_fee: u128,
    pub miner: String,
    pub state_root: String,
    pub extra_data: String,
}

/// Transaction data from database
#[derive(Debug, Serialize, Deserialize, clickhouse::Row)]
pub struct TransactionRow {
    pub tx_hash: String,
    pub block_number: u64,
    pub tx_index: u32,
    pub from: String,
    pub to: Option<String>,
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

/// Custom error type for API responses
pub enum ApiError {
    DatabaseError(String),
    NotFound(String),
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, error_message) = match self {
            ApiError::DatabaseError(msg) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                json!({
                    "error": "database_error",
                    "message": msg
                }),
            ),
            ApiError::NotFound(msg) => (
                StatusCode::NOT_FOUND,
                json!({
                    "error": "not_found",
                    "message": msg
                }),
            ),
        };

        (status, Json(error_message)).into_response()
    }
}

/// Health check endpoint
/// 
/// Returns a simple 200 OK response to indicate the API is running
pub async fn health_check() -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "healthy".to_string(),
    })
}

/// Get the latest 10 blocks
/// 
/// Queries the blocks table, filtering for sign=1 (non-reorged blocks)
/// and ordering by timestamp descending
pub async fn get_latest_blocks(
    State(state): State<AppState>,
) -> Result<Json<Vec<BlockRow>>, ApiError> {
    let query = r#"
        SELECT 
            block_number,
            hash,
            parent_hash,
            timestamp,
            gas_used,
            base_fee,
            miner,
            state_root,
            extra_data
        FROM blocks
        WHERE sign = 1
        ORDER BY timestamp DESC
        LIMIT 10
    "#;

    let blocks = state
        .client
        .query(query)
        .fetch_all::<BlockRow>()
        .await
        .map_err(|e| {
            tracing::error!("Failed to fetch latest blocks: {}", e);
            ApiError::DatabaseError(format!("Failed to query blocks: {}", e))
        })?;

    Ok(Json(blocks))
}

/// Get a specific transaction by hash
/// 
/// Queries the transactions table for a transaction with the given hash,
/// filtering for sign=1 (non-reorged transactions)
pub async fn get_transaction(
    State(state): State<AppState>,
    Path(tx_hash): Path<String>,
) -> Result<Json<TransactionRow>, ApiError> {
    // Normalize hash format (add 0x prefix if missing)
    let normalized_hash = if tx_hash.starts_with("0x") {
        tx_hash.clone()
    } else {
        format!("0x{}", tx_hash)
    };

    let query = r#"
        SELECT 
            tx_hash,
            block_number,
            tx_index,
            from,
            to,
            value,
            input,
            status,
            nonce,
            gas_limit,
            gas_price,
            max_fee_per_gas,
            max_priority_fee_per_gas,
            chain_id
        FROM transactions
        WHERE sign = 1
        AND tx_hash = ?
        LIMIT 1
    "#;

    let mut cursor = state
        .client
        .query(query)
        .bind(&normalized_hash)
        .fetch::<TransactionRow>()
        .map_err(|e| {
            tracing::error!("Failed to fetch transaction {}: {}", normalized_hash, e);
            ApiError::DatabaseError(format!("Failed to query transaction: {}", e))
        })?;

    match cursor.next().await {
        Ok(Some(tx)) => Ok(Json(tx)),
        Ok(None) => Err(ApiError::NotFound(format!(
            "Transaction {} not found",
            normalized_hash
        ))),
        Err(e) => {
            tracing::error!("Error fetching transaction {}: {}", normalized_hash, e);
            Err(ApiError::DatabaseError(format!(
                "Failed to fetch transaction: {}",
                e
            )))
        }
    }
}
