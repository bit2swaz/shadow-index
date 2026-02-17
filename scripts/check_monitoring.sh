#!/bin/bash
# Quick diagnostic script for Grafana "No Data" issue

echo "==================================================================="
echo "Shadow-Index Monitoring Diagnostic"
echo "==================================================================="
echo ""

echo "1. Checking if mock metrics server is running..."
if pgrep -f "mock_metrics_server.py" > /dev/null; then
    echo "   ✓ Mock server is running (PID: $(pgrep -f mock_metrics_server.py))"
else
    echo "   ✗ Mock server is NOT running"
    echo "   Start it with: python3 scripts/mock_metrics_server.py &"
fi
echo ""

echo "2. Checking metrics endpoint..."
if curl -s -f http://localhost:9001/metrics > /dev/null; then
    BLOCK_COUNT=$(curl -s http://localhost:9001/metrics | grep "shadow_index_blocks_processed_total" | grep -v "#" | awk '{print $2}')
    echo "   ✓ Metrics endpoint responding"
    echo "   Current blocks: $BLOCK_COUNT"
else
    echo "   ✗ Metrics endpoint not responding"
fi
echo ""

echo "3. Checking Prometheus..."
if curl -s -f http://localhost:9090/-/healthy > /dev/null; then
    echo "   ✓ Prometheus is healthy"
    
    # Check target status
    TARGET_STATUS=$(curl -s "http://localhost:9090/api/v1/targets" | grep -o '"health":"[^"]*"' | head -1 | cut -d'"' -f4)
    echo "   Target status: $TARGET_STATUS"
    
    # Check if Prometheus has data
    DATA_POINTS=$(wget -qO- "http://localhost:9090/api/v1/query?query=shadow_index_blocks_processed_total" 2>/dev/null | grep -o '"value":\[[^]]*\]' | wc -l)
    if [ "$DATA_POINTS" -gt 0 ]; then
        echo "   ✓ Prometheus has metric data ($DATA_POINTS data points)"
    else
        echo "   ✗ Prometheus has NO metric data"
        echo "   Wait 30 seconds for scrapes to accumulate"
    fi
else
    echo "   ✗ Prometheus not responding"
fi
echo ""

echo "4. Checking Grafana..."
if curl -s -f http://localhost:3000/api/health > /dev/null; then
    echo "   ✓ Grafana is healthy"
    
    # Check datasources
    DS_COUNT=$(curl -s -u admin:shadow123 http://localhost:3000/api/datasources 2>/dev/null | grep -o '"name":"[^"]*"' | wc -l)
    echo "   Datasources configured: $DS_COUNT"
else
    echo "   ✗ Grafana not responding"
fi
echo ""

echo "5. Testing rate() query (used by dashboard)..."
RATE_RESULT=$(wget -qO- "http://localhost:9090/api/v1/query?query=rate(shadow_index_blocks_processed_total%5B2m%5D)" 2>/dev/null | grep -o '"value":\[[^]]*\]')
if [ -n "$RATE_RESULT" ]; then
    RATE_VALUE=$(echo "$RATE_RESULT" | grep -o '[0-9.]*' | tail -1)
    echo "   ✓ rate() query working: ${RATE_VALUE} blocks/sec"
else
    echo "   ✗ rate() query returned no data"
    echo "   This is normal if prometheus has been running <2 minutes"
fi
echo ""

echo "==================================================================="
echo "Grafana Dashboard Troubleshooting"
echo "==================================================================="
echo ""
echo "If you see 'No Data' in Grafana:"
echo ""
echo "1. Change time range to 'Last 30 minutes' (top-right corner)"
echo "2. Wait 2-3 minutes for metrics to accumulate"
echo "3. Click the refresh button or wait for auto-refresh (5s)"
echo ""
echo "Dashboard URL: http://localhost:3000/d/shadow-index-main/shadow-index-performance-dashboard"
echo "Login: admin / shadow123"
echo ""
echo "==================================================================="
