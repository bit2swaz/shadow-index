# monitoring and observability

comprehensive guide to monitoring shadow-index performance with prometheus and grafana.

## table of contents

- [overview](#overview)
- [architecture](#architecture)
- [service endpoints](#service-endpoints)
- [available metrics](#available-metrics)
- [grafana dashboards](#grafana-dashboards)
- [prometheus configuration](#prometheus-configuration)
- [testing and development](#testing-and-development)
- [troubleshooting](#troubleshooting)

## overview

shadow-index exposes prometheus metrics for real-time monitoring of indexing performance, database health, and system behavior. this guide covers the complete observability stack including prometheus for metrics collection and grafana for visualization.

**monitoring objectives**:

1. track indexing throughput (blocks/sec and transactions/sec)
2. measure end-to-end latency (block received to database flush)
3. monitor buffer saturation and memory usage
4. detect errors (circuit breaker trips, reorg handling, database retries)
5. ensure system health and identify bottlenecks

## architecture

```
┌─────────────┐     metrics      ┌────────────┐     query      ┌─────────┐
│ shadow-index├─────────────────>│ prometheus ├───────────────>│ grafana │
│   (port     │   /metrics       │  (port     │   promql       │ (port   │
│    9001)    │   scrape 15s     │   9090)    │                │  3000)  │
└─────────────┘                  └────────────┘                └─────────┘
```

the observability stack consists of:

- **shadow-index**: exposes prometheus metrics on port 9001
- **prometheus**: scrapes metrics every 15 seconds, stores time-series data
- **grafana**: visualizes metrics with pre-built dashboards, connects to prometheus

## service endpoints

## service endpoints

### grafana dashboard

- **url**: http://localhost:3000
- **default credentials**: username `admin`, password `shadow123`
- **main dashboard**: http://localhost:3000/d/shadow-index-main/shadow-index-performance-dashboard
- **datasources**: http://localhost:3000/datasources

### prometheus

- **url**: http://localhost:9090
- **targets status**: http://localhost:9090/targets
- **query interface**: http://localhost:9090/graph
- **api endpoint**: http://localhost:9090/api/v1/query

### metrics endpoint

- **url**: http://localhost:9001/metrics
- **format**: prometheus text format (openmetrics compatible)
- **scrape interval**: 15 seconds (configurable in prometheus.yml)

## available metrics

## available metrics

shadow-index exposes the following prometheus metrics:

### counters

**shadow_index_blocks_processed_total**
- total number of blocks successfully processed and indexed
- use `rate()` function to calculate blocks/sec throughput

**shadow_index_events_captured_total**
- cumulative count of all events (transactions + logs + storage diffs)
- indicates total data volume processed

**shadow_index_reorgs_handled_total**
- number of chain reorganizations detected and handled
- spikes indicate network instability or deep reorgs

**shadow_index_circuit_breaker_trips_total**
- count of circuit breaker activations due to database failures
- non-zero values indicate connectivity issues

**shadow_index_db_retries_total**
- total database operation retries (exponential backoff)
- persistent growth indicates database performance degradation

### gauges

**shadow_index_buffer_saturation**
- current number of rows buffered in memory (range: 0 to buffer_size)
- values near buffer_size indicate batching working efficiently
- consistently low values may indicate insufficient block rate

### histograms

**shadow_index_db_latency_seconds**
- distribution of database write latency measurements
- buckets: 1ms, 2ms, 5ms, 10ms, 25ms, 50ms, 100ms, 250ms, 500ms, 1s, 2.5s
- use `histogram_quantile()` to calculate percentiles (p50, p95, p99)

## grafana dashboards

### shadow-index performance dashboard

the main performance dashboard (`shadow-index-main`) provides real-time visibility into system behavior with the following panels:

**block processing throughput** (timeseries)
- query: `rate(shadow_index_blocks_processed_total[1m])`
- shows blocks/sec and events/sec over time
- identifies throughput bottlenecks and performance degradation

**database write latency** (timeseries)
- queries:
  - p50: `histogram_quantile(0.50, rate(shadow_index_db_latency_seconds_bucket[1m])) * 1000`
  - p95: `histogram_quantile(0.95, rate(shadow_index_db_latency_seconds_bucket[1m])) * 1000`
  - p99: `histogram_quantile(0.99, rate(shadow_index_db_latency_seconds_bucket[1m])) * 1000`
- displays latency percentiles in milliseconds
- thresholds: green <50ms, yellow <100ms, red >100ms

**buffer saturation** (gauge)
- query: `shadow_index_buffer_saturation`
- visual indicator of memory buffer utilization
- thresholds: green <5000, yellow <8000, orange <10000, red ≥10000

**error and recovery metrics** (timeseries)
- tracks reorgs, circuit breaker trips, and database retries
- spikes indicate system stress or external failures

**total blocks processed** (stat panel)
- cumulative counter since process start
- useful for validating sync progress

**total events captured** (stat panel)
- cumulative event count (transactions + logs + storage diffs)
- indicates data ingestion volume

**circuit breaker status** (stat panel)
- query: `increase(shadow_index_circuit_breaker_trips_total[5m])`
- displays "healthy" (green) if 0, "circuit open" (red) if >0

**current p95 latency** (stat panel)
- 5-minute p95 latency snapshot
- quick health check for database performance

### dashboard configuration

- **refresh interval**: 5 seconds (configurable)
- **time range**: last 15 minutes (default)
- **timezone**: browser local time
- **provisioning**: auto-loaded from `docker/grafana/provisioning/dashboards/`

## prometheus configurationretries (exponential backoff)
- persistent growth indicates database performance degradation

### gauges

**shadow_index_buffer_saturation**
- current number of rows buffered in memory (range: 0 to buffer_size)
- values near buffer_size indicate batching working efficiently
- consistently low values may indicate insufficient block rate

### histograms

**shadow_index_db_latency_seconds**
- distribution of database write latency measurements
- buckets: 1ms, 2ms, 5ms, 10ms, 25ms, 50ms, 100ms, 250ms, 500ms, 1s, 2.5s
- use `histogram_quantile()` to calculate percentiles (p50, p95, p99)

## grafana dashboards

The Grafana dashboard includes:

1. **Block Processing Throughput** - Line chart showing blocks/sec and events/sec
2. **Database Write Latency** - Line chart with p50, p95, p99 percentiles
3. **Buffer Saturation** - Gauge showing current buffer size
4. **Error & Recovery Metrics** - Line chart tracking reorgs, circuit breaker trips, retries
5. **Total Blocks Processed** - Stat panel with running total
6. **Total Events Captured** - Stat panel with running total
7. **Circuit Breaker Status** - Stat panel showing health status
8. **Current p95 Latency** - Stat panel showing recent p95 latency

## 🧪 Testing the Dashboard

1. Open http://localhost:3000 in your browser
2. Login with `admin` / `shadow123`
3. The dashboard should auto-load or navigate to it from the sidebar
4. Watch the metrics update in real-time (5s refresh interval)
5. The mock server simulates realistic patterns:
   - ~5-10 blocks/sec throughput
   - 50-150 events per block
   - 3-8ms latency (p50-p95)
   - Occasional spikes and errors

## 🛠️ Managing the Stack

### Stop All Services
```bash
docker stop shadow-prometheus shadow-grafana
kill $(pgrep -f mock_metrics_server.py)
```

### Restart Prometheus (after config changes)
```bash
docker restart shadow-prometheus
```

### View Grafana Logs
```bash
docker logs -f shadow-grafana
```

### View Prometheus Logs
```bash
docker logs -f shadow-prometheus
```

### Check Scrape Status
```bash
curl -s http://localhost:9090/api/v1/targets | python3 -m json.tool
```

## 🎯 Next Steps

1. **Replace mock server** - Once the actual shadow-index binary is built, stop the mock server and the dashboard will pick up real metrics
2. **Add more dashboards** - Create additional panels for specific use cases
3. **Set up alerts** - Configure Grafana alerts for high latency, circuit breaker trips, etc.
4. **Export dashboard** - Save your customized dashboard JSON for version control

## 📝 Files Created

- `docker/prometheus/prometheus.yml` - Prometheus scrape configuration
- `docker/grafana/provisioning/datasources/datasource.yml` - Auto-provisioned datasources
- `docker/grafana/provisioning/dashboards/dashboard.yml` - Dashboard provisioning config
- `docker/grafana/provisioning/dashboards/shadow-index.json` - Main performance dashboard
- `scripts/mock_metrics_server.py` - Test metrics generator

## 🐛 Troubleshooting

## troubleshooting

### dashboard shows "no data"

**symptom**: grafana panels display "no data" or empty graphs

**causes and solutions**:

1. **insufficient time range**: rate() queries need at least 2 data points
   - increase time range in grafana (top-right corner) to "last 30 minutes" or more
   - wait 2-3 minutes for prometheus to collect multiple scrapes

2. **prometheus not scraping**:
   ```bash
   # check target status
   curl http://localhost:9090/api/v1/targets
   # should show shadow-index target as "up"
   ```

3. **metrics endpoint unreachable**:
   ```bash
   # verify metrics are being served
   curl http://localhost:9001/metrics | grep shadow_index_blocks
   ```

4. **docker networking issue**:
   - linux: verify prometheus uses `172.17.0.1:9001` as target
   - mac/windows: change to `host.docker.internal:9001` in prometheus.yml
   - restart prometheus after config changes: `docker restart shadow-prometheus`

### rate() queries return empty

**symptom**: queries like `rate(shadow_index_blocks_processed_total[1m])` return no data

**solution**: rate() requires at least 2 samples within the time window
- ensure prometheus has been scraping for >1 minute
- check scrape interval in prometheus config (default: 15s)
- use longer time range: `rate(shadow_index_blocks_processed_total[5m])`

### histogram percentiles show nan

**symptom**: `histogram_quantile()` returns nan or empty

**causes**:
- no samples in histogram buckets
- incorrect bucket label (should be `le`, not `bucket`)
- insufficient scrape duration

**solution**:
```bash
# verify histogram buckets exist
curl http://localhost:9001/metrics | grep shadow_index_db_latency_seconds_bucket

# check prometheus has the data
curl -s "http://localhost:9090/api/v1/query?query=shadow_index_db_latency_seconds_bucket"
```

### prometheus target down

**symptom**: prometheus shows shadow-index target as "down" with connection errors

**solutions**:

1. **verify service is running**:
   ```bash
   # check if metrics endpoint responds
   curl http://localhost:9001/metrics
   
   # check if process is running
   ps aux | grep shadow-index
   ```

2. **check docker network**:
   ```bash
   # verify containers are on same network
   docker network inspect shadow-network
   
   # test connectivity from prometheus container
   docker exec shadow-prometheus wget -O- http://172.17.0.1:9001/metrics
   ```

3. **firewall blocking requests**:
   ```bash
   # temporarily disable firewall (testing only)
   sudo ufw disable
   ```

### grafana datasource not working

**symptom**: grafana cannot query prometheus

**solutions**:

1. **verify datasource configuration**:
   - navigate to http://localhost:3000/datasources
   - click "prometheus" datasource
   - click "test" button at bottom
   - should show "data source is working"

2. **check prometheus url**:
   - from grafana container, prometheus should be at `http://prometheus:9090`
   - test: `docker exec shadow-grafana wget -O- http://prometheus:9090/-/healthy`

3. **restart services**:
   ```bash
   docker restart shadow-prometheus shadow-grafana
   ```

### dashboard panels empty after restart

**symptom**: dashboard loads but all panels are empty

**solution**: grafana may be querying before prometheus finishes starting
- wait 30-60 seconds after restart
- manually refresh dashboard (icon at top-right)
- check time range includes current time

## production deployment

### security considerations

**change default credentials**:
```bash
# in docker-compose.yml or environment
GF_SECURITY_ADMIN_PASSWORD=<strong-password>
```

**enable authentication**:
```yaml
# in prometheus.yml
basic_auth:
  username: prometheus
  password: <secure-password>
```

**restrict network access**:
- bind grafana/prometheus to localhost only: `127.0.0.1:3000`
- use reverse proxy (nginx/traefik) with tls
- implement ip whitelisting

### retention and storage

**prometheus data retention**:
```yaml
# in docker-compose.yml
command:
  - '--storage.tsdb.retention.time=30d'
  - '--storage.tsdb.retention.size=10GB'
```

**grafana persistent storage**:
```yaml
volumes:
  - grafana_data:/var/lib/grafana
  - prometheus_data:/prometheus
```

### alerting

configure grafana alerts for critical conditions:

1. **high latency alert**: p95 > 100ms for 5 minutes
2. **circuit breaker trip**: any trip in last 5 minutes
3. **throughput drop**: blocks/sec < 50% of baseline
4. **buffer saturation**: buffer >90% for 10 minutes

example alert rule in grafana:
```yaml
alert: HighLatency
expr: histogram_quantile(0.95, rate(shadow_index_db_latency_seconds_bucket[5m])) > 0.1
for: 5m
labels:
  severity: warning
annotations:
  summary: "database write latency is high (p95 > 100ms)"
```

## additional resources

- **prometheus documentation**: https://prometheus.io/docs/
- **grafana documentation**: https://grafana.com/docs/
- **promql tutorial**: https://prometheus.io/docs/prometheus/latest/querying/basics/
- **dashboard examples**: https://grafana.com/grafana/dashboards/

---

for questions or issues, refer to the main project README or open an issue on github.

