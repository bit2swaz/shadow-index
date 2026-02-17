# deployment guide

production deployment instructions for shadow-index on cloud infrastructure and bare metal.

## table of contents

- [prerequisites](#prerequisites)
- [hardware requirements](#hardware-requirements)
- [docker deployment](#docker-deployment)
- [configuration](#configuration)
- [network setup](#network-setup)
- [monitoring](#monitoring)
- [maintenance](#maintenance)
- [troubleshooting](#troubleshooting)

## prerequisites

### software dependencies

- **docker**: version 24.0+ with compose plugin
- **docker compose**: version 2.20+
- **linux kernel**: 5.10+ (for io_uring support in reth)
- **systemd**: for service management (optional)

### network requirements

- **inbound ports**:
  - `30303/tcp+udp`: reth p2p networking
  - `9001/tcp`: prometheus metrics (internal only)
- **outbound ports**:
  - `30303/tcp+udp`: ethereum p2p network
  - `443/tcp`: https for chain config downloads

### recommended os

- ubuntu 22.04 lts
- debian 12 (bookworm)
- rhel 9 / rocky linux 9

## hardware requirements

### minimum configuration (testnet)

| component | specification |
|-----------|--------------|
| cpu | 4 cores (x86_64 with sse4.2) |
| ram | 16 gb |
| disk | 500 gb nvme ssd |
| network | 100 mbps symmetric |

**suitable for**: sepolia testnet, holesky testnet

### recommended configuration (mainnet)

| component | specification |
|-----------|--------------|
| cpu | 16 cores (x86_64 with avx2) |
| ram | 64 gb |
| disk | 4 tb nvme ssd (raid 0 stripe preferred) |
| network | 1 gbps symmetric |

**suitable for**: ethereum mainnet archive node

### enterprise configuration

| component | specification |
|-----------|--------------|
| cpu | 32 cores (amd epyc / intel xeon) |
| ram | 128 gb ecc |
| disk | 8 tb nvme ssd raid 0 + 16 tb hdd for clickhouse cold storage |
| network | 10 gbps fiber |

**suitable for**: multi-chain indexing, high-query-load analytics

### disk space breakdown

**reth node storage**:
- sepolia testnet: ~100 gb (full), ~200 gb (archive)
- ethereum mainnet: ~1.5 tb (full), ~12 tb (archive)

**clickhouse storage**:
- sepolia (1 year): ~50 gb compressed
- ethereum mainnet (1 year): ~2 tb compressed with lz4

**growth rate**:
- ethereum mainnet: ~1 gb/day (full node)
- ethereum mainnet: ~10 gb/day (archive node with all state)

## docker deployment

### project structure

```
shadow-index/
├── docker-compose.yml          # development stack
├── docker-compose.prod.yml     # production stack
├── Dockerfile                  # multi-stage build
├── .dockerignore               # build exclusions
├── config.toml                 # configuration file
├── shadow-index.cursor         # persistent state (auto-created)
└── data/                       # reth + clickhouse volumes
    ├── clickhouse/             # database storage
    └── reth/                   # blockchain data
```

### step 1: clone repository

```bash
# create dedicated user (recommended)
sudo useradd -m -s /bin/bash shadow-index
sudo usermod -aG docker shadow-index
su - shadow-index

# clone repository
cd ~
git clone https://github.com/bit2swaz/shadow-index.git
cd shadow-index
```

### step 2: configure environment

create `.env` file for production credentials:

```bash
cat > .env << 'EOF'
# clickhouse credentials
SHADOW_INDEX__CLICKHOUSE__URL=http://clickhouse:8123
SHADOW_INDEX__CLICKHOUSE__DATABASE=shadow_index
SHADOW_INDEX__CLICKHOUSE__USERNAME=default
SHADOW_INDEX__CLICKHOUSE__PASSWORD=your_secure_password_here

# reth node configuration
SHADOW_INDEX__EXEX__BUFFER_SIZE=50000
SHADOW_INDEX__EXEX__FLUSH_INTERVAL_MS=500

# backfill configuration
SHADOW_INDEX__BACKFILL__ENABLED=true
SHADOW_INDEX__BACKFILL__BATCH_SIZE=100

# optional: cursor override
# SHADOW_INDEX__CURSOR__FILE_PATH=/app/data/shadow-index.cursor
EOF

chmod 600 .env  # restrict permissions
```

### step 3: initialize volumes

```bash
# create persistent directories
mkdir -p data/clickhouse
mkdir -p data/reth

# initialize cursor file
echo "0" > shadow-index.cursor

# set permissions
chmod 644 shadow-index.cursor
```

### step 4: start services

**development deployment** (sepolia testnet):

```bash
docker-compose up -d

# view logs
docker-compose logs -f shadow-index

# check status
docker-compose ps
```

**production deployment** (mainnet):

```bash
docker-compose -f docker-compose.prod.yml up -d

# view logs with filtering
docker-compose -f docker-compose.prod.yml logs -f shadow-index | grep -v "DEBUG"

# monitor resource usage
docker stats
```

### step 5: verify deployment

```bash
# check reth sync status
docker-compose exec shadow-index reth node --status

# check clickhouse connectivity
docker-compose exec clickhouse clickhouse-client --query "SHOW DATABASES"

# check shadow-index metrics
curl http://localhost:9001/metrics | grep shadow_index_blocks_processed_total

# query indexed data
docker-compose exec clickhouse clickhouse-client --query \
  "SELECT count(*) FROM shadow_index.blocks WHERE sign = 1"
```

## configuration

### layered configuration system

shadow-index uses a three-tier configuration hierarchy:

1. **default values** (embedded in code)
2. **config.toml** (file-based overrides)
3. **environment variables** (highest priority)

### config.toml structure

```toml
[clickhouse]
# database connection
url = "http://localhost:8123"
database = "shadow_index"
username = "default"
password = ""

# connection pool settings
max_connections = 10
connection_timeout_ms = 5000

[exex]
# buffering thresholds
buffer_size = 10000           # rows before flush
flush_interval_ms = 100       # milliseconds before time-based flush

# backpressure limits
max_pending_blocks = 1000     # block notification queue depth

[backfill]
# historical sync settings
enabled = true
batch_size = 100              # blocks per backfill batch
start_block = 0               # override start (0 = from cursor)

[cursor]
# state persistence
file_path = "./shadow-index.cursor"
sync_interval = 1             # blocks between cursor updates
```

### environment variable overrides

format: `SHADOW_INDEX__<SECTION>__<KEY>`

examples:

```bash
# override clickhouse url
export SHADOW_INDEX__CLICKHOUSE__URL="http://prod-db.internal:8123"

# override buffer size for high-throughput
export SHADOW_INDEX__EXEX__BUFFER_SIZE=50000

# disable backfill (realtime only)
export SHADOW_INDEX__BACKFILL__ENABLED=false
```

### validation rules

the configuration system enforces these constraints:

```rust
pub fn validate(&self) -> Result<(), ConfigError> {
    // clickhouse
    if self.clickhouse.url.is_empty() {
        return Err(ConfigError::Message("clickhouse url cannot be empty".into()));
    }
    
    // exex
    if self.exex.buffer_size == 0 {
        return Err(ConfigError::Message("buffer_size must be > 0".into()));
    }
    
    if self.exex.flush_interval_ms == 0 {
        return Err(ConfigError::Message("flush_interval_ms must be > 0".into()));
    }
    
    // backfill
    if self.backfill.batch_size == 0 {
        return Err(ConfigError::Message("batch_size must be > 0".into()));
    }
    
    Ok(())
}
```

configuration errors cause immediate startup failure with clear error messages.

## network setup

### firewall configuration

**ufw (ubuntu/debian)**:

```bash
# allow reth p2p
sudo ufw allow 30303/tcp
sudo ufw allow 30303/udp

# allow ssh (if not already enabled)
sudo ufw allow 22/tcp

# block external access to metrics and database
sudo ufw deny 9001/tcp
sudo ufw deny 8123/tcp

# enable firewall
sudo ufw enable
```

**firewalld (rhel/centos)**:

```bash
# allow reth p2p
sudo firewall-cmd --permanent --add-port=30303/tcp
sudo firewall-cmd --permanent --add-port=30303/udp

# block metrics and database
sudo firewall-cmd --permanent --remove-service=prometheus
sudo firewall-cmd --permanent --remove-service=clickhouse

# reload
sudo firewall-cmd --reload
```

### reverse proxy setup (nginx)

expose metrics endpoint through authenticated nginx proxy:

```nginx
upstream shadow_metrics {
    server localhost:9001;
}

server {
    listen 443 ssl http2;
    server_name metrics.yourdomain.com;
    
    ssl_certificate /etc/letsencrypt/live/yourdomain.com/fullchain.pem;
    ssl_certificate_key /etc/letsencrypt/live/yourdomain.com/privkey.pem;
    
    auth_basic "shadow-index metrics";
    auth_basic_user_file /etc/nginx/.htpasswd;
    
    location /metrics {
        proxy_pass http://shadow_metrics/metrics;
        proxy_set_header Host $host;
        proxy_set_header X-Real-IP $remote_addr;
    }
}
```

### dns configuration

recommended dns setup for production:

```
reth.yourdomain.com       -> reth p2p endpoint (30303)
db.internal.yourdomain.com -> clickhouse (internal only)
metrics.yourdomain.com    -> prometheus metrics (authenticated)
```

## monitoring

### prometheus integration

**prometheus.yml configuration**:

```yaml
global:
  scrape_interval: 15s
  evaluation_interval: 15s

scrape_configs:
  - job_name: 'shadow-index'
    static_configs:
      - targets: ['shadow-index:9001']
    metric_relabel_configs:
      # drop high-cardinality labels if needed
      - source_labels: [__name__]
        regex: 'shadow_index_.*'
        action: keep
```

**key metrics to monitor**:

```promql
# indexing rate (blocks/sec)
rate(shadow_index_blocks_processed_total[1m])

# event capture rate (events/sec)
rate(shadow_index_events_captured_total[1m])

# buffer saturation (percentage)
shadow_index_buffer_saturation / 10000 * 100

# database latency p95 (seconds)
histogram_quantile(0.95, rate(shadow_index_db_latency_seconds_bucket[5m]))

# retry rate (retries/min)
rate(shadow_index_db_retries_total[1m]) * 60

# circuit breaker trips (should be 0)
increase(shadow_index_circuit_breaker_trips_total[1h])
```

### grafana dashboard

import the provided dashboard json:

```bash
curl -X POST http://admin:admin@localhost:3000/api/dashboards/db \
  -H "Content-Type: application/json" \
  -d @docs/grafana-dashboard.json
```

**dashboard panels**:
1. indexing throughput (time series)
2. buffer saturation (gauge)
3. database latency (heatmap)
4. retry rate (time series)
5. circuit breaker status (single stat)
6. reorg frequency (counter)

### alerting rules

**prometheus alerting rules** (`/etc/prometheus/rules/shadow-index.yml`):

```yaml
groups:
  - name: shadow-index
    interval: 30s
    rules:
      - alert: ShadowIndexDown
        expr: up{job="shadow-index"} == 0
        for: 2m
        labels:
          severity: critical
        annotations:
          summary: "shadow-index is down"
          description: "shadow-index has been down for 2 minutes"
      
      - alert: HighDatabaseLatency
        expr: histogram_quantile(0.95, rate(shadow_index_db_latency_seconds_bucket[5m])) > 1.0
        for: 5m
        labels:
          severity: warning
        annotations:
          summary: "database writes are slow"
          description: "p95 latency is {{ $value }}s (threshold: 1s)"
      
      - alert: CircuitBreakerTripped
        expr: increase(shadow_index_circuit_breaker_trips_total[5m]) > 0
        labels:
          severity: critical
        annotations:
          summary: "circuit breaker has tripped"
          description: "clickhouse is unreachable, indexing has stopped"
      
      - alert: BufferSaturationHigh
        expr: shadow_index_buffer_saturation > 9000
        for: 10m
        labels:
          severity: warning
        annotations:
          summary: "buffer is nearly full"
          description: "buffer has {{ $value }} rows (threshold: 9000)"
```

### log aggregation

**structured logging with json output**:

```bash
# configure json logging
export RUST_LOG=shadow_index=info,reth=warn
export RUST_LOG_FORMAT=json

# pipe to log aggregator
docker-compose logs -f shadow-index | \
  fluent-bit -c /etc/fluent-bit/fluent-bit.conf
```

**example log entry**:

```json
{
  "timestamp": "2026-02-15T12:34:56.789Z",
  "level": "INFO",
  "target": "shadow_index::exex",
  "fields": {
    "message": "block processed",
    "block_number": 1234567,
    "tx_count": 42,
    "log_count": 128,
    "elapsed_ms": 45
  }
}
```

## maintenance

### database optimization

**run periodic optimize on clickhouse tables**:

```bash
# create optimization script
cat > /opt/shadow-index/optimize.sh << 'EOF'
#!/bin/bash
docker-compose exec clickhouse clickhouse-client --query \
  "OPTIMIZE TABLE shadow_index.blocks FINAL"
docker-compose exec clickhouse clickhouse-client --query \
  "OPTIMIZE TABLE shadow_index.transactions FINAL"
docker-compose exec clickhouse clickhouse-client --query \
  "OPTIMIZE TABLE shadow_index.logs FINAL"
docker-compose exec clickhouse clickhouse-client --query \
  "OPTIMIZE TABLE shadow_index.storage_diffs FINAL"
EOF

chmod +x /opt/shadow-index/optimize.sh

# schedule with cron (daily at 3am)
echo "0 3 * * * /opt/shadow-index/optimize.sh" | crontab -
```

### backup strategy

**1. cursor file backup** (every 5 minutes):

```bash
# systemd timer unit
cat > /etc/systemd/system/shadow-cursor-backup.timer << 'EOF'
[Unit]
Description=shadow-index cursor backup timer

[Timer]
OnCalendar=*:0/5
Persistent=true

[Install]
WantedBy=timers.target
EOF

# systemd service unit
cat > /etc/systemd/system/shadow-cursor-backup.service << 'EOF'
[Unit]
Description=backup shadow-index cursor file

[Service]
Type=oneshot
ExecStart=/bin/bash -c 'cp /home/shadow-index/shadow-index/shadow-index.cursor /backup/cursor-$(date +\%Y\%m\%d-\%H\%M\%S).txt'
User=shadow-index
EOF

# enable timer
sudo systemctl daemon-reload
sudo systemctl enable --now shadow-cursor-backup.timer
```

**2. clickhouse data backup** (daily):

```bash
# using clickhouse-backup tool
docker run --rm --volumes-from shadow-index-clickhouse-1 \
  -v /backup/clickhouse:/backup \
  yandex/clickhouse-backup:latest \
  clickhouse-backup create --config /etc/clickhouse-backup/config.yml
```

### version upgrades

**rolling upgrade procedure**:

```bash
# 1. stop shadow-index (keeps reth running)
docker-compose stop shadow-index

# 2. backup cursor file
cp shadow-index.cursor shadow-index.cursor.backup

# 3. pull new image
docker-compose pull shadow-index

# 4. restart with new version
docker-compose up -d shadow-index

# 5. monitor logs for errors
docker-compose logs -f shadow-index

# 6. verify metrics endpoint
curl http://localhost:9001/metrics
```

**rollback procedure**:

```bash
# 1. stop new version
docker-compose stop shadow-index

# 2. restore cursor file
cp shadow-index.cursor.backup shadow-index.cursor

# 3. use previous image tag
docker-compose up -d shadow-index:v1.0.0  # specify old version

# 4. verify recovery
docker-compose logs -f shadow-index
```

## troubleshooting

### common issues

#### issue: shadow-index won't start

**symptom**: container exits immediately after startup

**diagnosis**:

```bash
# check logs
docker-compose logs shadow-index

# check config validation
docker-compose exec shadow-index cat config.toml
```

**solutions**:

1. verify clickhouse url is accessible:
   ```bash
   docker-compose exec shadow-index curl http://clickhouse:8123/ping
   ```

2. check cursor file permissions:
   ```bash
   ls -la shadow-index.cursor
   chmod 644 shadow-index.cursor
   ```

3. validate configuration:
   ```bash
   # test config loading
   docker-compose run --rm shadow-index /app/shadow-index --validate-config
   ```

#### issue: high memory usage

**symptom**: container memory exceeds 2gb

**diagnosis**:

```bash
# check buffer saturation metric
curl http://localhost:9001/metrics | grep buffer_saturation

# check docker stats
docker stats shadow-index-1
```

**solutions**:

1. reduce buffer size:
   ```bash
   export SHADOW_INDEX__EXEX__BUFFER_SIZE=5000
   docker-compose restart shadow-index
   ```

2. increase flush frequency:
   ```bash
   export SHADOW_INDEX__EXEX__FLUSH_INTERVAL_MS=50
   docker-compose restart shadow-index
   ```

#### issue: clickhouse connection timeouts

**symptom**: repeated "connection refused" or "timeout" errors

**diagnosis**:

```bash
# check clickhouse status
docker-compose exec clickhouse clickhouse-client --query "SELECT 1"

# check network connectivity
docker-compose exec shadow-index ping -c 3 clickhouse

# check clickhouse logs
docker-compose logs clickhouse | grep -i error
```

**solutions**:

1. increase connection timeout:
   ```toml
   [clickhouse]
   connection_timeout_ms = 10000
   ```

2. scale clickhouse resources:
   ```yaml
   # docker-compose.yml
   services:
     clickhouse:
       deploy:
         resources:
           limits:
             cpus: '4'
             memory: 16G
   ```

#### issue: cursor file corruption

**symptom**: "invalid cursor value" error on startup

**diagnosis**:

```bash
# check cursor file content
cat shadow-index.cursor

# check file integrity
file shadow-index.cursor
```

**solutions**:

1. reset cursor to safe value:
   ```bash
   # get last indexed block from database
   docker-compose exec clickhouse clickhouse-client --query \
     "SELECT max(block_number) FROM shadow_index.blocks WHERE sign = 1"
   
   # update cursor file
   echo "1234567" > shadow-index.cursor
   ```

2. force full resync (caution: slow):
   ```bash
   echo "0" > shadow-index.cursor
   docker-compose restart shadow-index
   ```

#### issue: reorg handling failures

**symptom**: duplicate rows in database after chain reorg

**diagnosis**:

```bash
# check for blocks with net sign != 1
docker-compose exec clickhouse clickhouse-client --query \
  "SELECT block_number, sum(sign) as net FROM shadow_index.blocks GROUP BY block_number HAVING net != 1"
```

**solutions**:

1. trigger manual optimize to collapse rows:
   ```bash
   docker-compose exec clickhouse clickhouse-client --query \
     "OPTIMIZE TABLE shadow_index.blocks FINAL"
   ```

2. verify collapsingmergetree settings:
   ```sql
   SHOW CREATE TABLE shadow_index.blocks;
   -- should contain: ENGINE = CollapsingMergeTree(sign)
   ```

### performance tuning

#### scenario: slow historical backfill

**tuning parameters**:

```toml
[backfill]
batch_size = 500          # increase from 100
enabled = true

[exex]
buffer_size = 50000       # increase from 10000
flush_interval_ms = 500   # increase from 100
```

**expected improvement**: 2-3x faster backfill rate

#### scenario: high query latency

**clickhouse optimizations**:

```sql
-- add secondary indexes for common queries
ALTER TABLE shadow_index.logs ADD INDEX idx_address (address) TYPE bloom_filter;
ALTER TABLE shadow_index.transactions ADD INDEX idx_from (from) TYPE bloom_filter;

-- create materialized views for aggregations
CREATE MATERIALIZED VIEW shadow_index.daily_stats
ENGINE = SummingMergeTree()
ORDER BY (date, address)
AS SELECT
    toDate(timestamp) as date,
    address,
    count(*) as tx_count,
    sum(gas_used) as total_gas
FROM shadow_index.transactions
WHERE sign = 1
GROUP BY date, address;
```

### health checks

**endpoint-based healthcheck**:

```bash
#!/bin/bash
# /opt/shadow-index/healthcheck.sh

# check metrics endpoint
if ! curl -sf http://localhost:9001/metrics > /dev/null; then
    echo "ERROR: metrics endpoint unreachable"
    exit 1
fi

# check recent progress
LAST_BLOCK=$(curl -s http://localhost:9001/metrics | \
    grep shadow_index_blocks_processed_total | \
    awk '{print $2}')

if [ "$LAST_BLOCK" -eq 0 ]; then
    echo "ERROR: no blocks processed"
    exit 1
fi

echo "OK: shadow-index healthy (last block: $LAST_BLOCK)"
exit 0
```

**systemd watchdog integration**:

```ini
[Service]
Type=notify
WatchdogSec=60
Restart=on-failure
RestartSec=30
```

## security considerations

### credential management

**do not hardcode passwords**:

```bash
# bad: password in config file
[clickhouse]
password = "mypassword123"

# good: password from environment
export SHADOW_INDEX__CLICKHOUSE__PASSWORD=$(cat /run/secrets/clickhouse_password)
```

### container security

```yaml
# docker-compose.prod.yml security settings
services:
  shadow-index:
    security_opt:
      - no-new-privileges:true
    cap_drop:
      - ALL
    cap_add:
      - NET_BIND_SERVICE
    read_only: true
    tmpfs:
      - /tmp
```

### network isolation

```yaml
networks:
  internal:
    driver: bridge
    internal: true  # no external access
  external:
    driver: bridge

services:
  shadow-index:
    networks:
      - internal  # access to clickhouse only
      - external  # access to reth p2p
```

---

**last updated**: february 2026  
**version**: 1.0.0-rc1
