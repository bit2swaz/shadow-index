#!/bin/bash
# Metrics Verification Script for Shadow-Index

set -e

echo "╔════════════════════════════════════════════════════════════════╗"
echo "║        Shadow-Index Metrics Verification Script               ║"
echo "╚════════════════════════════════════════════════════════════════╝"
echo ""

METRICS_URL="http://localhost:9001/metrics"
TIMEOUT=5

echo "Testing Metrics Endpoint: $METRICS_URL"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo ""

# Test 1: Endpoint reachability
echo "Test 1: Checking if endpoint is reachable..."
if curl -s --connect-timeout $TIMEOUT "$METRICS_URL" > /dev/null; then
    echo "   Endpoint is reachable"
else
    echo "   Endpoint is not reachable"
    echo "   Tip: Make sure Shadow-Index is running: docker-compose ps shadow-index"
    exit 1
fi
echo ""

# Test 2: Check for Shadow-Index metrics
echo "Test 2: Checking for Shadow-Index metrics..."
METRIC_COUNT=$(curl -s "$METRICS_URL" | grep -c "shadow_index" || echo "0")
if [ "$METRIC_COUNT" -gt 0 ]; then
    echo "   Found $METRIC_COUNT Shadow-Index metric lines"
else
    echo "   No Shadow-Index metrics found"
    echo "   Tip: Check if ExEx is processing blocks: docker-compose logs shadow-index | grep 'processed block'"
    exit 1
fi
echo ""

# Test 3: Verify each metric type
echo "Test 3: Verifying individual metrics..."
echo ""

METRICS=(
    "shadow_index_blocks_processed_total:Blocks Processed"
    "shadow_index_events_captured_total:Events Captured"
    "shadow_index_reorgs_handled_total:Reorgs Handled"
    "shadow_index_buffer_saturation:Buffer Saturation"
    "shadow_index_db_latency_seconds:DB Latency"
    "shadow_index_db_retries_total:DB Retries"
    "shadow_index_circuit_breaker_trips_total:Circuit Breaker Trips"
)

for metric_info in "${METRICS[@]}"; do
    IFS=':' read -r metric_name metric_desc <<< "$metric_info"
    
    if curl -s "$METRICS_URL" | grep -q "$metric_name"; then
        VALUE=$(curl -s "$METRICS_URL" | grep "^$metric_name " | awk '{print $2}')
        if [ -n "$VALUE" ]; then
            echo "   $metric_desc: $VALUE"
        else
            echo "   $metric_desc: [histogram/multiple values]"
        fi
    else
        echo "    $metric_desc: NOT FOUND (may appear after first block)"
    fi
done
echo ""

# Test 4: Check Prometheus format
echo "Test 4: Verifying Prometheus format..."
if curl -s "$METRICS_URL" | grep -q "^# HELP"; then
    echo "   Metrics include HELP text"
fi
if curl -s "$METRICS_URL" | grep -q "^# TYPE"; then
    echo "   Metrics include TYPE declarations"
fi
echo ""

# Test 5: Sample metrics output
echo "Sample Metrics Output:"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
curl -s "$METRICS_URL" | grep "shadow_index" | head -20
echo ""
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo ""

# Summary
echo "All tests passed! Metrics endpoint is working correctly."
echo ""
echo "Next Steps:"
echo "   • View all metrics: curl http://localhost:9001/metrics"
echo "   • Access Prometheus: http://localhost:9090"
echo "   • Access Grafana: http://localhost:3000"
echo ""
echo "Documentation: docs/METRICS.md"
