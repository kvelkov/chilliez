#!/bin/bash

# Real-time Paper Trading Dashboard
# Usage: ./scripts/paper_trading_dashboard.sh [refresh_interval]

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" &> /dev/null && pwd)"
PROJECT_DIR="$(dirname "$SCRIPT_DIR")"
REFRESH_INTERVAL=${1:-5}  # Default 5 seconds

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
PURPLE='\033[0;35m'
CYAN='\033[0;36m'
WHITE='\033[1;37m'
NC='\033[0m' # No Color

# Clear screen and move cursor to top
clear_screen() {
    clear
    printf '\033[H\033[2J'
}

# Print header
print_header() {
    echo -e "${WHITE}╔══════════════════════════════════════════════════════════════════════════════╗${NC}"
    echo -e "${WHITE}║                    ${CYAN}🚀 SOLANA ARBITRAGE BOT - PAPER TRADING DASHBOARD${WHITE}                   ║${NC}"
    echo -e "${WHITE}║                              ${YELLOW}Real-time Monitoring Console${WHITE}                             ║${NC}"
    echo -e "${WHITE}╚══════════════════════════════════════════════════════════════════════════════╝${NC}"
    echo ""
    echo -e "${BLUE}Last Updated: $(date '+%Y-%m-%d %H:%M:%S')${NC} | ${YELLOW}Refresh: ${REFRESH_INTERVAL}s${NC} | ${GREEN}Press Ctrl+C to exit${NC}"
    echo ""
}

# Get system status
get_system_status() {
    local PID_FILE="/tmp/solana_paper_trading.pid"
    
    if [ -f "$PID_FILE" ]; then
        local PID=$(cat "$PID_FILE")
        if ps -p "$PID" > /dev/null 2>&1; then
            echo -e "${GREEN}RUNNING${NC} (PID: $PID)"
            local uptime=$(ps -o etime= -p "$PID" | tr -d ' ')
            local memory=$(ps -o rss= -p "$PID" | awk '{print int($1/1024) "MB"}')
            local cpu=$(ps -o %cpu= -p "$PID" | tr -d ' ')
            echo "   Uptime: $uptime | Memory: $memory | CPU: $cpu%"
        else
            echo -e "${RED}STOPPED${NC} (stale PID file)"
        fi
    else
        echo -e "${RED}STOPPED${NC}"
    fi
}

# Get portfolio summary
get_portfolio_summary() {
    local DEMO_LOGS_DIR="$PROJECT_DIR/demo_paper_logs"
    local LATEST_ANALYTICS=$(ls -t "$DEMO_LOGS_DIR"/paper_analytics_*.json 2>/dev/null | head -1)
    
    if [ -n "$LATEST_ANALYTICS" ] && command -v jq >/dev/null 2>&1; then
        local total_trades=$(jq -r '.total_trades' "$LATEST_ANALYTICS")
        local success_rate=$(jq -r '(.success_rate * 100 | floor)' "$LATEST_ANALYTICS")
        local return_pct=$(jq -r '.return_percentage' "$LATEST_ANALYTICS")
        local total_fees=$(jq -r '.total_fees_paid' "$LATEST_ANALYTICS")
        local max_drawdown=$(jq -r '.max_drawdown' "$LATEST_ANALYTICS")
        
        # Color code success rate
        local success_color=$RED
        if (( $(echo "$success_rate > 50" | bc -l 2>/dev/null || echo "0") )); then
            success_color=$GREEN
        elif (( $(echo "$success_rate > 25" | bc -l 2>/dev/null || echo "0") )); then
            success_color=$YELLOW
        fi
        
        # Color code returns
        local return_color=$RED
        if (( $(echo "$return_pct > 0" | bc -l 2>/dev/null || echo "0") )); then
            return_color=$GREEN
        fi
        
        echo "   Total Trades: ${WHITE}$total_trades${NC}"
        echo "   Success Rate: ${success_color}$success_rate%${NC}"
        echo "   Return: ${return_color}$return_pct%${NC}"
        echo "   Max Drawdown: ${RED}$max_drawdown%${NC}"
        echo "   Fees Paid: ${YELLOW}$total_fees${NC} lamports"
    else
        echo "   ${YELLOW}No analytics data available${NC}"
    fi
}

# Get recent activity
get_recent_activity() {
    local DEMO_LOGS_DIR="$PROJECT_DIR/demo_paper_logs"
    local LATEST_TRADES=$(ls -t "$DEMO_LOGS_DIR"/paper_trades_*.jsonl 2>/dev/null | head -1)
    
    if [ -n "$LATEST_TRADES" ] && command -v jq >/dev/null 2>&1; then
        echo "   ${WHITE}Recent Trades (Last 5):${NC}"
        tail -5 "$LATEST_TRADES" | while read line; do
            local timestamp=$(echo "$line" | jq -r '.timestamp' | cut -c12-19)
            local token_in=$(echo "$line" | jq -r '.token_in' | sed 's/Token_//')
            local token_out=$(echo "$line" | jq -r '.token_out' | sed 's/Token_//')
            local success=$(echo "$line" | jq -r '.execution_success')
            local amount_in=$(echo "$line" | jq -r '.amount_in')
            local slippage=$(echo "$line" | jq -r '.slippage_applied')
            
            if [ "$success" = "true" ]; then
                local status="${GREEN}✓${NC}"
            else
                local status="${RED}✗${NC}"
            fi
            
            printf "   %s [%s] %s → %s (%s units, %0.2f%% slip)\n" \
                "$status" "$timestamp" "$token_in" "$token_out" "$amount_in" \
                "$(echo "$slippage * 100" | bc -l 2>/dev/null || echo "0")"
        done
    else
        echo "   ${YELLOW}No trade data available${NC}"
    fi
}

# Get network status
get_network_status() {
    local rpc_url=$(grep "SOLANA_RPC_URL" "$PROJECT_DIR/.env" 2>/dev/null | cut -d'=' -f2)
    local jupiter_url=$(grep "JUPITER_API_URL" "$PROJECT_DIR/.env" 2>/dev/null | cut -d'=' -f2)
    
    # Test RPC connection
    if curl -s --max-time 3 "$rpc_url" -H "Content-Type: application/json" -d '{"jsonrpc":"2.0","id":1,"method":"getHealth"}' | grep -q "ok"; then
        echo "   Solana RPC: ${GREEN}✓ Connected${NC} ($rpc_url)"
    else
        echo "   Solana RPC: ${RED}✗ Disconnected${NC} ($rpc_url)"
    fi
    
    # Test Jupiter connection
    if curl -s --max-time 3 "$jupiter_url/health" > /dev/null 2>&1; then
        echo "   Jupiter API: ${GREEN}✓ Connected${NC} ($jupiter_url)"
    else
        echo "   Jupiter API: ${RED}✗ Disconnected${NC} ($jupiter_url)"
    fi
}

# Get performance metrics
get_performance_metrics() {
    local DEMO_LOGS_DIR="$PROJECT_DIR/demo_paper_logs"
    local LATEST_ANALYTICS=$(ls -t "$DEMO_LOGS_DIR"/paper_analytics_*.json 2>/dev/null | head -1)
    
    if [ -n "$LATEST_ANALYTICS" ] && command -v jq >/dev/null 2>&1; then
        local session_start=$(jq -r '.session_start' "$LATEST_ANALYTICS")
        local session_end=$(jq -r '.session_end' "$LATEST_ANALYTICS")
        local avg_profit=$(jq -r '.average_profit_per_trade' "$LATEST_ANALYTICS")
        local largest_win=$(jq -r '.largest_win' "$LATEST_ANALYTICS")
        local largest_loss=$(jq -r '.largest_loss' "$LATEST_ANALYTICS")
        local sharpe_ratio=$(jq -r '.sharpe_ratio' "$LATEST_ANALYTICS")
        
        echo "   Session: ${CYAN}$(echo $session_start | cut -c12-19)${NC} to ${CYAN}$(echo $session_end | cut -c12-19)${NC}"
        echo "   Avg P&L/Trade: ${WHITE}$avg_profit${NC} lamports"
        echo "   Best Trade: ${GREEN}$largest_win${NC} lamports"
        echo "   Worst Trade: ${RED}$largest_loss${NC} lamports"
        echo "   Sharpe Ratio: ${WHITE}$sharpe_ratio${NC}"
    else
        echo "   ${YELLOW}No performance data available${NC}"
    fi
}

# Get system resources
get_system_resources() {
    local disk_usage=$(df "$PROJECT_DIR" | awk 'NR==2 {print $5}' | sed 's/%//')
    local load_avg=$(uptime | awk -F'load average:' '{print $2}' | awk '{print $1}' | sed 's/,//')
    
    # Color code disk usage
    local disk_color=$GREEN
    if [ "$disk_usage" -gt 80 ]; then
        disk_color=$RED
    elif [ "$disk_usage" -gt 60 ]; then
        disk_color=$YELLOW
    fi
    
    echo "   Disk Usage: ${disk_color}$disk_usage%${NC}"
    echo "   Load Average: ${WHITE}$load_avg${NC}"
    
    # Check log file sizes
    local log_count=$(find "$PROJECT_DIR" -name "*paper*.log" -o -name "*paper*.json*" 2>/dev/null | wc -l | tr -d ' ')
    echo "   Log Files: ${WHITE}$log_count${NC}"
}

# Main dashboard loop
main_dashboard() {
    while true; do
        clear_screen
        print_header
        
        # System Status Section
        echo -e "${WHITE}╔════════════════════════════════════════════════════════════════════════════╗${NC}"
        echo -e "${WHITE}║  ${PURPLE}📊 SYSTEM STATUS${WHITE}                                                           ║${NC}"
        echo -e "${WHITE}╚════════════════════════════════════════════════════════════════════════════╝${NC}"
        echo -e "${YELLOW}System Status:${NC}"
        get_system_status
        echo ""
        
        # Portfolio Section
        echo -e "${WHITE}╔════════════════════════════════════════════════════════════════════════════╗${NC}"
        echo -e "${WHITE}║  ${PURPLE}💰 PORTFOLIO PERFORMANCE${WHITE}                                                  ║${NC}"
        echo -e "${WHITE}╚════════════════════════════════════════════════════════════════════════════╝${NC}"
        echo -e "${YELLOW}Portfolio Summary:${NC}"
        get_portfolio_summary
        echo ""
        
        # Performance Section
        echo -e "${WHITE}╔════════════════════════════════════════════════════════════════════════════╗${NC}"
        echo -e "${WHITE}║  ${PURPLE}📈 PERFORMANCE METRICS${WHITE}                                                     ║${NC}"
        echo -e "${WHITE}╚════════════════════════════════════════════════════════════════════════════╝${NC}"
        echo -e "${YELLOW}Performance Metrics:${NC}"
        get_performance_metrics
        echo ""
        
        # Activity Section
        echo -e "${WHITE}╔════════════════════════════════════════════════════════════════════════════╗${NC}"
        echo -e "${WHITE}║  ${PURPLE}📊 RECENT ACTIVITY${WHITE}                                                         ║${NC}"
        echo -e "${WHITE}╚════════════════════════════════════════════════════════════════════════════╝${NC}"
        echo -e "${YELLOW}Trade Activity:${NC}"
        get_recent_activity
        echo ""
        
        # Network Section
        echo -e "${WHITE}╔════════════════════════════════════════════════════════════════════════════╗${NC}"
        echo -e "${WHITE}║  ${PURPLE}🌐 NETWORK STATUS${WHITE}                                                          ║${NC}"
        echo -e "${WHITE}╚════════════════════════════════════════════════════════════════════════════╝${NC}"
        echo -e "${YELLOW}Network Connectivity:${NC}"
        get_network_status
        echo ""
        
        # Resources Section
        echo -e "${WHITE}╔════════════════════════════════════════════════════════════════════════════╗${NC}"
        echo -e "${WHITE}║  ${PURPLE}⚡ SYSTEM RESOURCES${WHITE}                                                        ║${NC}"
        echo -e "${WHITE}╚════════════════════════════════════════════════════════════════════════════╝${NC}"
        echo -e "${YELLOW}System Resources:${NC}"
        get_system_resources
        echo ""
        
        # Footer
        echo -e "${WHITE}╔════════════════════════════════════════════════════════════════════════════╗${NC}"
        echo -e "${WHITE}║  ${GREEN}Commands: ${CYAN}monitor_paper_trading.sh [start|stop|status|logs|reports]${WHITE}         ║${NC}"
        echo -e "${WHITE}╚════════════════════════════════════════════════════════════════════════════╝${NC}"
        
        sleep "$REFRESH_INTERVAL"
    done
}

# Handle Ctrl+C gracefully
trap 'echo -e "\n${GREEN}Dashboard stopped. Have a great day! 👋${NC}"; exit 0' INT

# Check dependencies
if ! command -v jq >/dev/null 2>&1; then
    echo -e "${YELLOW}Warning: 'jq' not found. Install with: brew install jq (macOS) or apt-get install jq (Ubuntu)${NC}"
    echo "Some features will be limited without jq."
    echo ""
fi

if ! command -v bc >/dev/null 2>&1; then
    echo -e "${YELLOW}Warning: 'bc' not found. Install with: brew install bc (macOS) or apt-get install bc (Ubuntu)${NC}"
    echo "Some calculations will be limited without bc."
    echo ""
fi

# Start dashboard
echo -e "${CYAN}🚀 Starting Paper Trading Dashboard...${NC}"
echo -e "${YELLOW}Refresh interval: ${REFRESH_INTERVAL} seconds${NC}"
echo -e "${GREEN}Press Ctrl+C to exit${NC}"
echo ""
sleep 2

main_dashboard
