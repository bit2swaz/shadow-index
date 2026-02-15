#!/bin/bash
# Quick start script for Shadow-Index Docker deployment

set -e

echo "Shadow-Index Docker Deployment"
echo "=================================="
echo ""

# Check prerequisites
if ! command -v docker &> /dev/null; then
    echo "Error: Docker is not installed"
    exit 1
fi

if ! command -v docker-compose &> /dev/null; then
    echo "Error: Docker Compose is not installed"
    exit 1
fi

echo "Prerequisites check passed"
echo ""

# Create data directory for cursor persistence
echo "Creating data directory..."
mkdir -p data

# Initialize cursor file if it doesn't exist
if [ ! -f shadow-index.cursor ]; then
    echo "Initializing cursor file (starting from block 0)..."
    echo "0" > shadow-index.cursor
else
    CURRENT_BLOCK=$(cat shadow-index.cursor)
    echo "Existing cursor found: Block $CURRENT_BLOCK"
fi

echo ""
echo "Select deployment mode:"
echo "  1) Development (with Prometheus + Grafana)"
echo "  2) Production (minimal)"
read -p "Enter choice [1-2]: " choice

case $choice in
    1)
        echo ""
        echo "  Building and starting development stack..."
        docker-compose up -d
        COMPOSE_FILE="docker-compose.yml"
        ;;
    2)
        echo ""
        echo "  Building and starting production stack..."
        docker-compose -f docker-compose.prod.yml up -d
        COMPOSE_FILE="docker-compose.prod.yml"
        ;;
    *)
        echo "  Invalid choice"
        exit 1
        ;;
esac

echo ""
echo "Waiting for services to be healthy..."
sleep 10

# Check service status
echo " "
echo "Service Status:"
if [ "$COMPOSE_FILE" = "docker-compose.yml" ]; then
    docker-compose ps
else
    docker-compose -f docker-compose.prod.yml ps
fi

echo ""
echo "Deployment complete!"
echo ""
echo "Access points:"
echo "  - ClickHouse HTTP:     http://localhost:8123"
echo "  - Reth JSON-RPC:       http://localhost:8545"
echo "  - Prometheus Metrics:  http://localhost:9001/metrics"

if [ "$choice" = "1" ]; then
    echo "  - Prometheus UI:       http://localhost:9090"
    echo "  - Grafana UI:          http://localhost:3000 (admin/shadow123)"
fi

echo ""
echo "Useful commands:"
echo "  View logs:       docker-compose logs -f shadow-index"
echo "  Check cursor:    cat shadow-index.cursor"
echo "  Query DB:        docker exec shadow-clickhouse clickhouse-client"
echo "  Stop services:   docker-compose down"
echo ""
