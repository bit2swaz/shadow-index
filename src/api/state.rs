use clickhouse::Client;

/// Application state shared across all API handlers
/// 
/// Holds a thread-safe ClickHouse client instance to prevent
/// opening a new connection pool on every request.
#[derive(Clone)]
pub struct AppState {
    /// ClickHouse client with connection pooling
    pub client: Client,
}

impl AppState {
    /// Create a new AppState with the given ClickHouse client
    pub fn new(client: Client) -> Self {
        Self { client }
    }
}
