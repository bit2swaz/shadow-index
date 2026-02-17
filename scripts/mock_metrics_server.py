#!/usr/bin/env python3
"""
Mock Prometheus metrics server for testing Shadow-Index Grafana dashboard.
Simulates the metrics that shadow-index would expose.
"""

from http.server import HTTPServer, BaseHTTPRequestHandler
import random
import time
from datetime import datetime

class MockMetricsHandler(BaseHTTPRequestHandler):
    start_time = time.time()
    blocks_processed = 0
    events_captured = 0
    reorgs_handled = 0
    circuit_breaker_trips = 0
    db_retries = 0
    
    def do_GET(self):
        if self.path == '/metrics':
            # Simulate growing counters
            elapsed = time.time() - self.start_time
            self.blocks_processed = int(elapsed * random.uniform(5, 10))  # ~5-10 blocks/sec
            self.events_captured = int(self.blocks_processed * random.uniform(50, 150))
            
            # Occasional errors
            if random.random() < 0.01:
                self.reorgs_handled += 1
            if random.random() < 0.005:
                self.circuit_breaker_trips += 1
            if random.random() < 0.02:
                self.db_retries += 1
            
            # Generate buffer saturation (0-10000 range)
            buffer_saturation = random.randint(1000, 8000)
            
            # Generate latency histogram buckets (in seconds)
            latency_samples = 100
            latency_sum = 0.0
            latency_buckets = {
                0.001: 0,  # 1ms
                0.002: 0,  # 2ms
                0.005: 0,  # 5ms
                0.010: 0,  # 10ms
                0.025: 0,  # 25ms
                0.050: 0,  # 50ms
                0.100: 0,  # 100ms
                0.250: 0,  # 250ms
                0.500: 0,  # 500ms
                1.000: 0,  # 1s
                2.500: 0,  # 2.5s
            }
            
            # Simulate realistic latency distribution (most samples 3-8ms)
            for _ in range(latency_samples):
                # Generate latency mostly in 3-8ms range with occasional spikes
                if random.random() < 0.9:
                    latency = random.uniform(0.003, 0.008)  # 3-8ms normal case
                else:
                    latency = random.uniform(0.010, 0.050)  # 10-50ms occasional spike
                
                latency_sum += latency
                for bucket_value in sorted(latency_buckets.keys()):
                    if latency <= bucket_value:
                        latency_buckets[bucket_value] += 1
            
            # Build Prometheus text format response
            metrics = []
            
            # HELP and TYPE declarations
            metrics.append("# HELP shadow_index_blocks_processed_total Total blocks processed by shadow-index")
            metrics.append("# TYPE shadow_index_blocks_processed_total counter")
            metrics.append(f"shadow_index_blocks_processed_total {self.blocks_processed}")
            metrics.append("")
            
            metrics.append("# HELP shadow_index_events_captured_total Total events captured (transactions + logs + storage diffs)")
            metrics.append("# TYPE shadow_index_events_captured_total counter")
            metrics.append(f"shadow_index_events_captured_total {self.events_captured}")
            metrics.append("")
            
            metrics.append("# HELP shadow_index_buffer_saturation Current buffer size (number of rows buffered)")
            metrics.append("# TYPE shadow_index_buffer_saturation gauge")
            metrics.append(f"shadow_index_buffer_saturation {buffer_saturation}")
            metrics.append("")
            
            metrics.append("# HELP shadow_index_reorgs_handled_total Total chain reorganizations handled")
            metrics.append("# TYPE shadow_index_reorgs_handled_total counter")
            metrics.append(f"shadow_index_reorgs_handled_total {self.reorgs_handled}")
            metrics.append("")
            
            metrics.append("# HELP shadow_index_circuit_breaker_trips_total Total circuit breaker activations")
            metrics.append("# TYPE shadow_index_circuit_breaker_trips_total counter")
            metrics.append(f"shadow_index_circuit_breaker_trips_total {self.circuit_breaker_trips}")
            metrics.append("")
            
            metrics.append("# HELP shadow_index_db_retries_total Total database retry attempts")
            metrics.append("# TYPE shadow_index_db_retries_total counter")
            metrics.append(f"shadow_index_db_retries_total {self.db_retries}")
            metrics.append("")
            
            # Histogram for latency
            metrics.append("# HELP shadow_index_db_latency_seconds Database write latency histogram")
            metrics.append("# TYPE shadow_index_db_latency_seconds histogram")
            cumulative_count = 0
            for bucket_value in sorted(latency_buckets.keys()):
                cumulative_count += latency_buckets[bucket_value]
                metrics.append(f'shadow_index_db_latency_seconds_bucket{{le="{bucket_value}"}} {cumulative_count}')
            metrics.append(f'shadow_index_db_latency_seconds_bucket{{le="+Inf"}} {latency_samples}')
            metrics.append(f"shadow_index_db_latency_seconds_sum {latency_sum}")
            metrics.append(f"shadow_index_db_latency_seconds_count {latency_samples}")
            metrics.append("")
            
            response = "\n".join(metrics)
            
            self.send_response(200)
            self.send_header('Content-Type', 'text/plain; version=0.0.4; charset=utf-8')
            self.send_header('Content-Length', str(len(response)))
            self.end_headers()
            self.wfile.write(response.encode('utf-8'))
        else:
            self.send_response(404)
            self.end_headers()
    
    def log_message(self, format, *args):
        # Suppress default HTTP request logging
        pass

def run_server(port=9001):
    server_address = ('', port)
    httpd = HTTPServer(server_address, MockMetricsHandler)
    print(f"🚀 Mock metrics server running on http://localhost:{port}/metrics")
    print(f"📊 Dashboard: http://localhost:3000 (admin/shadow123)")
    print(f"📈 Prometheus: http://localhost:9090")
    print(f"⏹️  Press Ctrl+C to stop\n")
    
    try:
        httpd.serve_forever()
    except KeyboardInterrupt:
        print("\n\n✋ Shutting down mock server...")
        httpd.shutdown()

if __name__ == '__main__':
    run_server()
