#!/usr/bin/env python3

from http.server import HTTPServer, BaseHTTPRequestHandler
import json
import time

MOCK_BLOCKS = [
    {"number": 19000010, "hash": "0xzyx890abc123def456789012345678901234567890123456789012345678901234", "timestamp": 1708300108},
    {"number": 19000009, "hash": "0xyz567def456abc789012345678901234567890123456789012345678901234567", "timestamp": 1708300096},
    {"number": 19000008, "hash": "0xvwx234abc789def012345678901234567890123456789012345678901234567890", "timestamp": 1708300084},
    {"number": 19000007, "hash": "0xstu901def012abc345678901234567890123456789012345678901234567890123", "timestamp": 1708300072},
    {"number": 19000006, "hash": "0xpqr678abc345def678901234567890123456789012345678901234567890123456", "timestamp": 1708300060},
    {"number": 19000005, "hash": "0xmno345def678abc901234567890123456789012345678901234567890123456789", "timestamp": 1708300048},
    {"number": 19000004, "hash": "0xjkl012abc901def234567890123456789012345678901234567890123456789012", "timestamp": 1708300036},
    {"number": 19000003, "hash": "0xghi789def234abc567890123456789012345678901234567890123456789012345", "timestamp": 1708300024},
    {"number": 19000002, "hash": "0xdef456abc567def890123456789012345678901234567890123456789012345678", "timestamp": 1708300012},
    {"number": 19000001, "hash": "0xabc123def890abc123456789012345678901234567890123456789012345678901", "timestamp": 1708300000},
]

class MockAPIHandler(BaseHTTPRequestHandler):
    def do_GET(self):
        if self.path == '/api/health':
            self.send_response(200)
            self.send_header('Content-Type', 'application/json')
            self.send_header('Access-Control-Allow-Origin', '*')
            self.end_headers()
            self.wfile.write(json.dumps({"status": "healthy"}).encode())
            print("[✓] Health check OK")
            
        elif self.path == '/api/blocks/latest':
            current_time = int(time.time())
            blocks = []
            for i, block in enumerate(MOCK_BLOCKS):
                blocks.append({
                    "number": block["number"] + i * 100,
                    "hash": block["hash"],
                    "timestamp": current_time - (i * 12)
                })
            
            self.send_response(200)
            self.send_header('Content-Type', 'application/json')
            self.send_header('Access-Control-Allow-Origin', '*')
            self.end_headers()
            self.wfile.write(json.dumps(blocks).encode())
            print(f"[✓] Served {len(blocks)} blocks")
            
        elif self.path.startswith('/api/tx/'):
            tx_hash = self.path.split('/')[-1]
            self.send_response(200)
            self.send_header('Content-Type', 'application/json')
            self.send_header('Access-Control-Allow-Origin', '*')
            self.end_headers()
            mock_tx = {
                "tx_hash": tx_hash,
                "block_number": 19000005,
                "from": "0x1234567890123456789012345678901234567890",
                "to": "0x0987654321098765432109876543210987654321",
                "value": "1000000000000000000",
                "status": 1
            }
            self.wfile.write(json.dumps(mock_tx).encode())
            print(f"[✓] Served transaction {tx_hash[:10]}...")
        else:
            self.send_response(404)
            self.send_header('Content-Type', 'application/json')
            self.send_header('Access-Control-Allow-Origin', '*')
            self.end_headers()
            self.wfile.write(json.dumps({"error": "not found"}).encode())
    
    def do_OPTIONS(self):
        self.send_response(200)
        self.send_header('Access-Control-Allow-Origin', '*')
        self.send_header('Access-Control-Allow-Methods', 'GET, OPTIONS')
        self.send_header('Access-Control-Allow-Headers', 'Content-Type')
        self.end_headers()
    
    def log_message(self, format, *args):
        # Custom logging
        
if __name__ == '__main__':
    print("╔════════════════════════════════════════════════════════════╗")
    print("shadow-index mock api server")
    print("starting on http://localhost:3000")
    print("")
    print("endpoints:")
    print("  GET /api/health")
    print("  GET /api/blocks/latest")
    print("  GET /api/tx/:hash")
    print("")
    
    server = HTTPServer(('0.0.0.0', 3000), MockAPIHandler)
    try:
        server.serve_forever()
    except KeyboardInterrupt:
        print("\ns