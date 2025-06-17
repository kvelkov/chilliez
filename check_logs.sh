#!/bin/bash
# 📊 Dashboard Bot Log Viewer
# Quick tool to check logs from your arbitrage bot runs

echo "🎛️  DASHBOARD BOT LOG VIEWER"
echo "════════════════════════════"
echo ""

# Function to show recent log files
show_recent_logs() {
    echo "📁 Recent Log Files:"
    echo "━━━━━━━━━━━━━━━━━━━━"
    ls -lat /Users/kiril/Desktop/chilliez/paper_trading_logs/ | head -10
    echo ""
}

# Function to show log summary
show_log_summary() {
    echo "📊 Latest Bot Session Summary:"
    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
    
    # Find most recent profitability analysis
    LATEST_ANALYSIS=$(ls -t /Users/kiril/Desktop/chilliez/paper_trading_logs/profitability_analysis_*.json 2>/dev/null | head -1)
    
    if [ -f "$LATEST_ANALYSIS" ]; then
        echo "📋 File: $(basename $LATEST_ANALYSIS)"
        echo ""
        
        # Check if jq is available
        if command -v jq &> /dev/null; then
            echo "🎯 Performance Summary:"
            jq -r '.summary | "   ├─ Total Opportunities: \(.total_opportunities)\n   ├─ Profitable Found: \(.profitable_opportunities)\n   ├─ Success Rate: \(.success_rate_percent)%\n   ├─ Total Profit: €\(.total_potential_profit_eur)\n   ├─ Average Profit: €\(.average_profit_eur)\n   └─ Hourly Rate: €\(.hourly_profit_rate_eur)/hour"' "$LATEST_ANALYSIS"
            echo ""
            
            echo "⚙️  Configuration Used:"
            jq -r '.analyzer_stats.thresholds | "   ├─ Min Profit: €\(.minProfitEur)\n   ├─ Min Profit %: \(.minProfitPercent)%\n   └─ Max Trade Size: \(.maxTradeSize * 100)%"' "$LATEST_ANALYSIS"
            echo ""
            
            echo "💰 Wallet Info:"
            jq -r '.analyzer_stats.walletBalance | "   ├─ SOL Balance: \(.sol) SOL\n   ├─ USDC Balance: \(.usdc) USDC\n   └─ USDT Balance: \(.usdt) USDT"' "$LATEST_ANALYSIS"
        else
            echo "⚠️  Install jq for detailed analysis: brew install jq"
            echo "📄 Raw summary available in: $LATEST_ANALYSIS"
        fi
    else
        echo "❌ No profitability analysis logs found"
    fi
    echo ""
}

# Function to show recent arbitrage sessions
show_arbitrage_sessions() {
    echo "🔄 Recent Arbitrage Sessions:"
    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
    
    for session in $(ls -t /Users/kiril/Desktop/chilliez/paper_trading_logs/arbitrage_session_*.json 2>/dev/null | head -3); do
        if [ -f "$session" ]; then
            echo "📋 Session: $(basename $session)"
            if command -v jq &> /dev/null; then
                jq -r '.session | "   ├─ Runtime: \(.runtime_seconds)s\n   ├─ Opportunities: \(.total_opportunities)\n   └─ Data: \(.data_received_mb)MB"' "$session"
            fi
            echo ""
        fi
    done
}

# Function to tail live logs
tail_live_logs() {
    echo "📡 Live Log Monitoring (Press Ctrl+C to stop):"
    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
    echo ""
    
    # Monitor the log directory for new files
    LOG_DIR="/Users/kiril/Desktop/chilliez/paper_trading_logs"
    
    if [ -d "$LOG_DIR" ]; then
        # Get the most recent log file
        LATEST_LOG=$(ls -t "$LOG_DIR"/*.json 2>/dev/null | head -1)
        
        if [ -f "$LATEST_LOG" ]; then
            echo "📊 Monitoring: $(basename $LATEST_LOG)"
            echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
            tail -f "$LATEST_LOG"
        else
            echo "⚠️  No log files found to monitor"
        fi
    else
        echo "❌ Log directory not found: $LOG_DIR"
    fi
}

# Main menu
case "${1:-menu}" in
    "summary"|"s")
        show_log_summary
        ;;
    "recent"|"r")
        show_recent_logs
        ;;
    "sessions"|"sess")
        show_arbitrage_sessions
        ;;
    "live"|"l")
        tail_live_logs
        ;;
    "all"|"a")
        show_log_summary
        show_arbitrage_sessions
        show_recent_logs
        ;;
    "menu"|*)
        echo "🎛️  Available Commands:"
        echo "━━━━━━━━━━━━━━━━━━━━━"
        echo "   ./check_logs.sh summary    (or 's') - Show latest performance summary"
        echo "   ./check_logs.sh recent     (or 'r') - List recent log files"
        echo "   ./check_logs.sh sessions   (or 'sess') - Show arbitrage session details"
        echo "   ./check_logs.sh live       (or 'l') - Monitor live logs"
        echo "   ./check_logs.sh all        (or 'a') - Show everything"
        echo ""
        echo "🚀 Quick Examples:"
        echo "   ./check_logs.sh s          # Quick summary"
        echo "   ./check_logs.sh a          # Full overview"
        echo "   ./check_logs.sh l          # Watch live activity"
        echo ""
        show_log_summary
        ;;
esac
