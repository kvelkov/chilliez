#!/bin/bash

# Comprehensive Paper Trading Setup and Management
# This script sets up and manages the complete paper trading environment
# Usage: ./scripts/complete_paper_trading_setup.sh [setup|start|monitor|help]

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" &> /dev/null && pwd)"
PROJECT_DIR="$(dirname "$SCRIPT_DIR")"

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
PURPLE='\033[0;35m'
CYAN='\033[0;36m'
WHITE='\033[1;37m'
NC='\033[0m'

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

success() {
    echo -e "${GREEN}[$(date +'%Y-%m-%d %H:%M:%S')] ✅ $1${NC}"
}

print_banner() {
    echo -e "${CYAN}"
    echo "╔══════════════════════════════════════════════════════════════════════════════╗"
    echo "║                                                                              ║"
    echo "║           🚀 SOLANA ARBITRAGE BOT - PAPER TRADING SETUP 🚀                  ║"
    echo "║                                                                              ║"
    echo "║                    Complete Environment Configuration                        ║"
    echo "║                                                                              ║"
    echo "╚══════════════════════════════════════════════════════════════════════════════╝"
    echo -e "${NC}"
}

# Check system dependencies
check_dependencies() {
    info "🔍 Checking system dependencies..."
    
    local missing_deps=()
    
    # Check required tools
    if ! command -v cargo >/dev/null 2>&1; then
        missing_deps+=("cargo (Rust)")
    fi
    
    if ! command -v solana >/dev/null 2>&1; then
        missing_deps+=("solana-cli")
    fi
    
    # Check optional tools
    if ! command -v jq >/dev/null 2>&1; then
        warn "jq is not installed - analytics will be limited"
        echo "  Install with: brew install jq (macOS) or apt-get install jq (Ubuntu)"
    fi
    
    if ! command -v bc >/dev/null 2>&1; then
        warn "bc is not installed - calculations will be limited"
        echo "  Install with: brew install bc (macOS) or apt-get install bc (Ubuntu)"
    fi
    
    if [ ${#missing_deps[@]} -gt 0 ]; then
        error "Missing required dependencies:"
        for dep in "${missing_deps[@]}"; do
            echo "  - $dep"
        done
        echo ""
        echo "Please install missing dependencies and run again."
        exit 1
    fi
    
    success "All required dependencies are installed"
}

# Setup environment configuration
setup_environment() {
    info "⚙️ Setting up environment configuration..."
    
    cd "$PROJECT_DIR"
    
    # Copy paper trading environment
    if [ -f ".env.paper-trading" ]; then
        cp .env.paper-trading .env
        success "Environment configured for paper trading"
    else
        error ".env.paper-trading file not found!"
        exit 1
    fi
    
    # Check collector wallet
    if [ ! -f "paper-trading-collector.json" ]; then
        warn "Paper trading collector wallet not found, generating new one..."
        solana-keygen new --outfile paper-trading-collector.json --no-bip39-passphrase
        success "New collector wallet generated"
    else
        success "Collector wallet found"
    fi
    
    # Create necessary directories
    mkdir -p paper_trading_logs paper_trading_reports demo_paper_logs
    success "Directory structure created"
}

# Test system connectivity
test_connectivity() {
    info "🌐 Testing network connectivity..."
    
    cd "$PROJECT_DIR"
    
    # Load environment variables
    source .env 2>/dev/null || true
    
    local rpc_url="${SOLANA_RPC_URL:-https://api.devnet.solana.com}"
    local jupiter_url="${JUPITER_API_URL:-https://quote-api.jup.ag/v6}"
    
    # Test Solana RPC
    if curl -s --max-time 5 "$rpc_url" -H "Content-Type: application/json" -d '{"jsonrpc":"2.0","id":1,"method":"getHealth"}' | grep -q "ok"; then
        success "Solana RPC connected: $rpc_url"
    else
        warn "Solana RPC connection failed: $rpc_url"
    fi
    
    # Test Jupiter API
    if curl -s --max-time 5 "$jupiter_url/health" >/dev/null 2>&1; then
        success "Jupiter API connected: $jupiter_url"
    else
        warn "Jupiter API connection failed: $jupiter_url"
    fi
}

# Compile the project
compile_project() {
    info "🔨 Compiling Solana arbitrage bot..."
    
    cd "$PROJECT_DIR"
    
    if cargo check --examples >/dev/null 2>&1; then
        success "Project compilation successful"
    else
        error "Project compilation failed"
        echo "Please check the build errors and fix them before continuing."
        exit 1
    fi
}

# Run initial test
run_initial_test() {
    info "🧪 Running initial paper trading test..."
    
    cd "$PROJECT_DIR"
    
    # Run a quick test to ensure everything works
    timeout 30s cargo run --example real_environment_paper_trading >/dev/null 2>&1 || true
    
    # Check if logs were generated
    if [ -d "demo_paper_logs" ] && [ "$(ls -A demo_paper_logs 2>/dev/null)" ]; then
        success "Initial test completed - logs generated"
    else
        warn "Initial test completed but no logs found"
    fi
}

# Display setup summary
display_summary() {
    echo ""
    echo -e "${WHITE}╔══════════════════════════════════════════════════════════════════════════════╗${NC}"
    echo -e "${WHITE}║                            ${GREEN}🎉 SETUP COMPLETE! 🎉${WHITE}                               ║${NC}"
    echo -e "${WHITE}╚══════════════════════════════════════════════════════════════════════════════╝${NC}"
    echo ""
    
    info "📁 Project Structure:"
    echo "   📂 $PROJECT_DIR/"
    echo "   ├── 📄 .env (paper trading configuration)"
    echo "   ├── 🔑 paper-trading-collector.json (wallet)"
    echo "   ├── 📊 demo_paper_logs/ (trade logs)"
    echo "   ├── 📈 paper_trading_reports/ (performance reports)"
    echo "   └── 🛠️  scripts/ (management tools)"
    echo ""
    
    info "🚀 Available Commands:"
    echo "   Start Paper Trading:    ./scripts/monitor_paper_trading.sh start"
    echo "   Check Status:           ./scripts/monitor_paper_trading.sh status"
    echo "   View Logs:              ./scripts/monitor_paper_trading.sh logs"
    echo "   Generate Reports:       ./scripts/monitor_paper_trading.sh reports"
    echo "   Real-time Dashboard:    ./scripts/paper_trading_dashboard.sh"
    echo "   Stop Trading:           ./scripts/monitor_paper_trading.sh stop"
    echo ""
    
    info "🔧 Quick Start:"
    echo "   1. Start the system:    ./scripts/monitor_paper_trading.sh start"
    echo "   2. Open dashboard:      ./scripts/paper_trading_dashboard.sh"
    echo "   3. Monitor progress:    ./scripts/monitor_paper_trading.sh status"
    echo ""
    
    info "📖 Documentation:"
    echo "   Paper Trading Guide:    docs/PAPER_TRADING.txt"
    echo "   System Architecture:    docs/architecture/"
    echo "   API Documentation:      docs/dex_clients_overview.md"
    echo ""
    
    success "Paper trading environment is ready for use! 🚀"
}

# Start paper trading with dashboard
start_with_dashboard() {
    info "🚀 Starting paper trading system with dashboard..."
    
    # Start the monitoring system
    if "$SCRIPT_DIR/monitor_paper_trading.sh" start; then
        sleep 3
        
        # Launch dashboard
        info "📊 Launching real-time dashboard..."
        exec "$SCRIPT_DIR/paper_trading_dashboard.sh"
    else
        error "Failed to start paper trading system"
        exit 1
    fi
}

# Show monitoring interface
show_monitoring() {
    info "📊 Launching monitoring interface..."
    
    echo -e "${CYAN}Choose monitoring option:${NC}"
    echo "1. Real-time Dashboard"
    echo "2. Status Check"
    echo "3. View Logs"
    echo "4. Generate Reports"
    echo "5. Exit"
    echo ""
    
    read -p "Enter your choice (1-5): " choice
    
    case $choice in
        1)
            exec "$SCRIPT_DIR/paper_trading_dashboard.sh"
            ;;
        2)
            "$SCRIPT_DIR/monitor_paper_trading.sh" status
            ;;
        3)
            "$SCRIPT_DIR/monitor_paper_trading.sh" logs
            ;;
        4)
            "$SCRIPT_DIR/monitor_paper_trading.sh" reports
            ;;
        5)
            info "Goodbye! 👋"
            exit 0
            ;;
        *)
            warn "Invalid choice"
            show_monitoring
            ;;
    esac
}

# Main setup process
run_complete_setup() {
    print_banner
    
    echo -e "${BLUE}This script will set up a complete paper trading environment for the Solana arbitrage bot.${NC}"
    echo ""
    
    read -p "Continue with setup? (y/N) " -n 1 -r
    echo
    if [[ ! $REPLY =~ ^[Yy]$ ]]; then
        info "Setup cancelled"
        exit 0
    fi
    
    echo ""
    info "🚀 Starting comprehensive paper trading setup..."
    echo ""
    
    # Run setup steps
    check_dependencies
    setup_environment
    test_connectivity
    compile_project
    run_initial_test
    
    display_summary
    
    echo ""
    read -p "Would you like to start paper trading now? (y/N) " -n 1 -r
    echo
    if [[ $REPLY =~ ^[Yy]$ ]]; then
        start_with_dashboard
    else
        info "You can start paper trading later with: ./scripts/monitor_paper_trading.sh start"
    fi
}

# Show help
show_help() {
    echo "Comprehensive Paper Trading Setup and Management"
    echo ""
    echo "Usage: $0 [command]"
    echo ""
    echo "Commands:"
    echo "  setup     - Run complete setup process (default)"
    echo "  start     - Start paper trading with dashboard"
    echo "  monitor   - Show monitoring options"
    echo "  help      - Show this help message"
    echo ""
    echo "Examples:"
    echo "  $0                    # Run complete setup"
    echo "  $0 setup              # Run complete setup"
    echo "  $0 start              # Start with dashboard"
    echo "  $0 monitor            # Show monitoring options"
    echo ""
    echo "After setup, you can use these management commands:"
    echo "  ./scripts/monitor_paper_trading.sh [start|stop|status|logs|reports]"
    echo "  ./scripts/paper_trading_dashboard.sh [refresh_interval]"
}

# Main command handling
case "${1:-setup}" in
    setup)
        run_complete_setup
        ;;
    start)
        start_with_dashboard
        ;;
    monitor)
        show_monitoring
        ;;
    help|--help|-h)
        show_help
        ;;
    *)
        error "Unknown command: $1"
        echo "Use '$0 help' for usage information"
        exit 1
        ;;
esac
