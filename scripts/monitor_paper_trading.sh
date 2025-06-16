#!/bin/bash

# Paper Trading Monitoring and Management Script
# Usage: ./scripts/monitor_paper_trading.sh [start|stop|status|logs|reports|reset]

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" &> /dev/null && pwd)"
PROJECT_DIR="$(dirname "$SCRIPT_DIR")"
LOG_DIR="$PROJECT_DIR/paper_trading_logs"
REPORTS_DIR="$PROJECT_DIR/paper_trading_reports"
DEMO_LOGS_DIR="$PROJECT_DIR/demo_paper_logs"
PID_FILE="/tmp/solana_paper_trading.pid"

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# Helper functions
log() {
    echo -e "${GREEN}[$(date +'%Y-%m-%d %H:%M:%S')] $1${NC}"
}

warn() {
    echo -e "${YELLOW}[$(date +'%Y-%m-%d %H:%M:%S')] WARNING: $1${NC}"
}

error() {
    echo -e "${RED}[$(date +'%Y-%m-%d %H:%M:%S')] ERROR: $1${NC}"
}

info() {
    echo -e "${BLUE}[$(date +'%Y-%m-%d %H:%M:%S')] $1${NC}"
}

# Check if paper trading process is running
is_running() {
    if [ -f "$PID_FILE" ]; then
        PID=$(cat "$PID_FILE")
        if ps -p "$PID" > /dev/null 2>&1; then
            return 0
        else
            rm -f "$PID_FILE"
            return 1
        fi
    fi
    return 1
}

# Start paper trading
start_paper_trading() {
    info "🚀 Starting Paper Trading System..."
    
    if is_running; then
        warn "Paper trading is already running (PID: $(cat "$PID_FILE"))"
        return 1
    fi
    
    # Ensure directories exist
    mkdir -p "$LOG_DIR" "$REPORTS_DIR" "$DEMO_LOGS_DIR"
    
    # Setup environment
    cd "$PROJECT_DIR"
    if [ -f ".env.paper-trading" ]; then
        cp .env.paper-trading .env
        log "Environment configured for paper trading"
    else
        error "No .env.paper-trading file found!"
        return 1
    fi
    
    # Start the paper trading process
    info "Starting real environment paper trading demo..."
    nohup cargo run --example real_environment_paper_trading > "$LOG_DIR/paper_trading_$(date +%Y%m%d_%H%M%S).log" 2>&1 &
    echo $! > "$PID_FILE"
    
    sleep 2
    if is_running; then
        log "✅ Paper trading started successfully (PID: $(cat "$PID_FILE"))"
        log "📊 Logs: $LOG_DIR/paper_trading_$(date +%Y%m%d_%H%M%S).log"
    else
        error "Failed to start paper trading"
        return 1
    fi
}

# Stop paper trading
stop_paper_trading() {
    info "🛑 Stopping Paper Trading System..."
    
    if ! is_running; then
        warn "Paper trading is not currently running"
        return 1
    fi
    
    PID=$(cat "$PID_FILE")
    kill "$PID" 2>/dev/null || true
    sleep 2
    
    if kill -0 "$PID" 2>/dev/null; then
        warn "Process didn't stop gracefully, force killing..."
        kill -9 "$PID" 2>/dev/null || true
        sleep 1
    fi
    
    rm -f "$PID_FILE"
    log "✅ Paper trading stopped successfully"
}

# Show status
show_status() {
    info "📊 Paper Trading System Status"
    echo "================================"
    
    if is_running; then
        PID=$(cat "$PID_FILE")
        echo -e "Status: ${GREEN}RUNNING${NC} (PID: $PID)"
        echo "Uptime: $(ps -o etime= -p "$PID" | tr -d ' ')"
        echo "Memory: $(ps -o rss= -p "$PID" | awk '{print int($1/1024) "MB"}')"
        echo "CPU: $(ps -o %cpu= -p "$PID" | tr -d ' ')%"
    else
        echo -e "Status: ${RED}STOPPED${NC}"
    fi
    
    echo ""
    echo "📁 Directory Status:"
    echo "   Logs: $LOG_DIR ($(ls -1 "$LOG_DIR" 2>/dev/null | wc -l | tr -d ' ') files)"
    echo "   Reports: $REPORTS_DIR ($(ls -1 "$REPORTS_DIR" 2>/dev/null | wc -l | tr -d ' ') files)"
    echo "   Demo Logs: $DEMO_LOGS_DIR ($(ls -1 "$DEMO_LOGS_DIR" 2>/dev/null | wc -l | tr -d ' ') files)"
    
    # Check recent activity
    if [ -d "$DEMO_LOGS_DIR" ]; then
        LATEST_ANALYTICS=$(ls -t "$DEMO_LOGS_DIR"/paper_analytics_*.json 2>/dev/null | head -1)
        if [ -n "$LATEST_ANALYTICS" ]; then
            echo ""
            info "📈 Latest Analytics Summary:"
            if command -v jq >/dev/null 2>&1; then
                echo "   Session: $(jq -r '.session_start' "$LATEST_ANALYTICS") to $(jq -r '.session_end' "$LATEST_ANALYTICS")"
                echo "   Total Trades: $(jq -r '.total_trades' "$LATEST_ANALYTICS")"
                echo "   Success Rate: $(jq -r '(.success_rate * 100 | floor)' "$LATEST_ANALYTICS")%"
                echo "   Return: $(jq -r '.return_percentage' "$LATEST_ANALYTICS")%"
            else
                echo "   Latest file: $(basename "$LATEST_ANALYTICS")"
                echo "   (Install 'jq' for detailed summary)"
            fi
        fi
    fi
}

# Show logs
show_logs() {
    info "📋 Recent Paper Trading Logs"
    echo "============================="
    
    if [ -d "$LOG_DIR" ]; then
        LATEST_LOG=$(ls -t "$LOG_DIR"/paper_trading_*.log 2>/dev/null | head -1)
        if [ -n "$LATEST_LOG" ]; then
            echo "Latest log file: $(basename "$LATEST_LOG")"
            echo ""
            tail -20 "$LATEST_LOG"
        else
            warn "No log files found in $LOG_DIR"
        fi
    else
        warn "Log directory does not exist: $LOG_DIR"
    fi
    
    # Show demo logs if available
    if [ -d "$DEMO_LOGS_DIR" ]; then
        LATEST_TRADES=$(ls -t "$DEMO_LOGS_DIR"/paper_trades_*.jsonl 2>/dev/null | head -1)
        if [ -n "$LATEST_TRADES" ]; then
            echo ""
            info "📊 Recent Trade Activity:"
            tail -5 "$LATEST_TRADES" | while read line; do
                if command -v jq >/dev/null 2>&1; then
                    timestamp=$(echo "$line" | jq -r '.timestamp')
                    token_in=$(echo "$line" | jq -r '.token_in')
                    token_out=$(echo "$line" | jq -r '.token_out')
                    success=$(echo "$line" | jq -r '.execution_success')
                    if [ "$success" = "true" ]; then
                        status="SUCCESS"
                    else
                        status="FAILED"
                    fi
                    echo "   [$timestamp] $token_in -> $token_out ($status)"
                else
                    echo "   $line"
                fi
            done
        fi
    fi
}

# Generate reports
generate_reports() {
    info "📊 Generating Paper Trading Reports..."
    
    cd "$PROJECT_DIR"
    
    # Check if we have analytics data
    if [ ! -d "$DEMO_LOGS_DIR" ] || [ -z "$(ls "$DEMO_LOGS_DIR"/paper_analytics_*.json 2>/dev/null)" ]; then
        warn "No analytics data found to generate reports"
        return 1
    fi
    
    # Create reports directory
    mkdir -p "$REPORTS_DIR"
    
    # Generate summary report
    REPORT_FILE="$REPORTS_DIR/paper_trading_summary_$(date +%Y%m%d_%H%M%S).md"
    
    cat > "$REPORT_FILE" << EOF
# Paper Trading Performance Report
Generated: $(date)

## Executive Summary

EOF
    
    # Process latest analytics if jq is available
    if command -v jq >/dev/null 2>&1; then
        LATEST_ANALYTICS=$(ls -t "$DEMO_LOGS_DIR"/paper_analytics_*.json 2>/dev/null | head -1)
        if [ -n "$LATEST_ANALYTICS" ]; then
            cat >> "$REPORT_FILE" << EOF
### Latest Session Performance
- **Start Time**: $(jq -r '.session_start' "$LATEST_ANALYTICS")
- **End Time**: $(jq -r '.session_end' "$LATEST_ANALYTICS")
- **Total Trades**: $(jq -r '.total_trades' "$LATEST_ANALYTICS")
- **Success Rate**: $(jq -r '(.success_rate * 100 | floor)' "$LATEST_ANALYTICS")%
- **Total Return**: $(jq -r '.return_percentage' "$LATEST_ANALYTICS")%
- **Fees Paid**: $(jq -r '.total_fees_paid' "$LATEST_ANALYTICS") lamports

### Risk Metrics
- **Max Drawdown**: $(jq -r '.max_drawdown' "$LATEST_ANALYTICS")%
- **Sharpe Ratio**: $(jq -r '.sharpe_ratio' "$LATEST_ANALYTICS")
- **Largest Win**: $(jq -r '.largest_win' "$LATEST_ANALYTICS") lamports
- **Largest Loss**: $(jq -r '.largest_loss' "$LATEST_ANALYTICS") lamports

EOF
        fi
    else
        echo "- Install 'jq' for detailed analytics processing" >> "$REPORT_FILE"
    fi
    
    cat >> "$REPORT_FILE" << EOF

## System Information
- **Environment**: Paper Trading (Devnet)
- **RPC Endpoint**: $(grep SOLANA_RPC_URL .env.paper-trading 2>/dev/null || echo "Not configured")
- **Report Generated**: $(date)

## Recommendations
1. Monitor success rate and adjust strategies if below target
2. Review slippage patterns for optimization opportunities
3. Analyze fee impact on profitability
4. Consider risk management improvements based on drawdown metrics

---
*This report was automatically generated by the Paper Trading Monitoring System*
EOF
    
    log "✅ Report generated: $REPORT_FILE"
    
    # Also create a CSV summary for easy analysis
    CSV_FILE="$REPORTS_DIR/paper_trading_data_$(date +%Y%m%d_%H%M%S).csv"
    echo "timestamp,session_start,session_end,total_trades,success_rate,return_percentage,max_drawdown,total_fees" > "$CSV_FILE"
    
    for analytics_file in "$DEMO_LOGS_DIR"/paper_analytics_*.json; do
        if [ -f "$analytics_file" ] && command -v jq >/dev/null 2>&1; then
            timestamp=$(date -Iseconds)
            session_start=$(jq -r '.session_start' "$analytics_file")
            session_end=$(jq -r '.session_end' "$analytics_file")
            total_trades=$(jq -r '.total_trades' "$analytics_file")
            success_rate=$(jq -r '.success_rate' "$analytics_file")
            return_percentage=$(jq -r '.return_percentage' "$analytics_file")
            max_drawdown=$(jq -r '.max_drawdown' "$analytics_file")
            total_fees=$(jq -r '.total_fees_paid' "$analytics_file")
            echo "\"$timestamp\",\"$session_start\",\"$session_end\",$total_trades,$success_rate,$return_percentage,$max_drawdown,$total_fees" >> "$CSV_FILE"
        fi
    done
    
    log "✅ CSV data exported: $CSV_FILE"
}

# Reset paper trading environment
reset_environment() {
    info "🔄 Resetting Paper Trading Environment..."
    
    # Stop if running
    if is_running; then
        stop_paper_trading
    fi
    
    # Archive old logs
    if [ -d "$LOG_DIR" ] && [ "$(ls -A "$LOG_DIR" 2>/dev/null)" ]; then
        ARCHIVE_DIR="$PROJECT_DIR/paper_trading_archive_$(date +%Y%m%d_%H%M%S)"
        mkdir -p "$ARCHIVE_DIR"
        mv "$LOG_DIR"/* "$ARCHIVE_DIR/" 2>/dev/null || true
        log "📦 Old logs archived to: $ARCHIVE_DIR"
    fi
    
    if [ -d "$DEMO_LOGS_DIR" ] && [ "$(ls -A "$DEMO_LOGS_DIR" 2>/dev/null)" ]; then
        ARCHIVE_DIR="${ARCHIVE_DIR:-$PROJECT_DIR/paper_trading_archive_$(date +%Y%m%d_%H%M%S)}"
        mkdir -p "$ARCHIVE_DIR"
        mv "$DEMO_LOGS_DIR"/* "$ARCHIVE_DIR/" 2>/dev/null || true
        log "📦 Demo logs archived to: $ARCHIVE_DIR"
    fi
    
    # Recreate directories
    mkdir -p "$LOG_DIR" "$REPORTS_DIR" "$DEMO_LOGS_DIR"
    
    # Reset environment configuration
    cd "$PROJECT_DIR"
    if [ -f ".env.paper-trading" ]; then
        cp .env.paper-trading .env
        log "✅ Environment reset to paper trading configuration"
    fi
    
    log "✅ Paper trading environment reset complete"
}

# Main command handling
case "${1:-status}" in
    start)
        start_paper_trading
        ;;
    stop)
        stop_paper_trading
        ;;
    restart)
        stop_paper_trading
        sleep 2
        start_paper_trading
        ;;
    status)
        show_status
        ;;
    logs)
        show_logs
        ;;
    reports)
        generate_reports
        ;;
    reset)
        read -p "This will archive current logs and reset the environment. Continue? (y/N) " -n 1 -r
        echo
        if [[ $REPLY =~ ^[Yy]$ ]]; then
            reset_environment
        else
            info "Reset cancelled"
        fi
        ;;
    help|--help|-h)
        echo "Paper Trading Monitoring and Management Script"
        echo ""
        echo "Usage: $0 [command]"
        echo ""
        echo "Commands:"
        echo "  start     - Start paper trading system"
        echo "  stop      - Stop paper trading system"
        echo "  restart   - Restart paper trading system"
        echo "  status    - Show system status and metrics"
        echo "  logs      - Show recent logs and activity"
        echo "  reports   - Generate performance reports"
        echo "  reset     - Reset environment and archive old data"
        echo "  help      - Show this help message"
        echo ""
        echo "Examples:"
        echo "  $0 start          # Start paper trading"
        echo "  $0 status         # Check current status"
        echo "  $0 logs           # View recent activity"
        echo "  $0 reports        # Generate performance report"
        ;;
    *)
        error "Unknown command: $1"
        echo "Use '$0 help' for usage information"
        exit 1
        ;;
esac
