#!/bin/bash
# Run script for the sandbox game
# Usage: ./run.sh [server|client|both|multi]

set -e

MODE=${1:-both}

# Colors for output
GREEN='\033[0;32m'
BLUE='\033[0;34m'
YELLOW='\033[0;33m'
NC='\033[0m' # No Color

# Kill any existing server processes to avoid "Address already in use"
cleanup_server() {
    pkill -f "target.*release/server" 2>/dev/null || true
    sleep 0.5
}

case $MODE in
    server)
        cleanup_server
        echo -e "${GREEN}Starting server...${NC}"
        cargo run -p server --release
        ;;
    client)
        echo -e "${BLUE}Starting client...${NC}"
        cargo run -p client --release
        ;;
    both)
        cleanup_server
        echo -e "${GREEN}Starting server in background...${NC}"
        cargo run -p server --release &
        SERVER_PID=$!
        
        # Wait for server to start
        sleep 2
        
        echo -e "${BLUE}Starting client...${NC}"
        cargo run -p client --release
        
        # When client exits, kill the server
        echo -e "${GREEN}Client closed. Stopping server...${NC}"
        kill $SERVER_PID 2>/dev/null || true
        ;;
    multi)
        cleanup_server
        echo -e "${GREEN}Starting server in background...${NC}"
        cargo run -p server --release &
        SERVER_PID=$!
        
        # Wait for server to start
        sleep 2
        
        echo -e "${BLUE}Starting client 1...${NC}"
        cargo run -p client --release &
        CLIENT1_PID=$!
        
        sleep 1
        
        echo -e "${YELLOW}Starting client 2...${NC}"
        cargo run -p client --release &
        CLIENT2_PID=$!
        
        echo -e "${GREEN}Server and 2 clients running. Press Enter to stop all...${NC}"
        read
        
        echo -e "${GREEN}Stopping all processes...${NC}"
        kill $CLIENT1_PID $CLIENT2_PID $SERVER_PID 2>/dev/null || true
        ;;
    *)
        echo "Usage: ./run.sh [server|client|both|multi]"
        echo "  server - Start only the server"
        echo "  client - Start only the client"  
        echo "  both   - Start server then client (default)"
        echo "  multi  - Start server + 2 clients for multiplayer testing"
        exit 1
        ;;
esac
