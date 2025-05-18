use crate::arbitrage::{
    calculator::calculate_multihop_profit_and_slippage,
    dynamic_threshold::{recommended_min_profit_threshold, VolatilityTracker},
    engine::ArbitrageEngine,
    executor::ArbitrageExecutor,
};
use crate::metrics::{Metrics, TradingPair};
use crate::utils::PoolInfo;
use futures::future::join_all;
use std::{sync::Arc, time::Duration};
use tokio::{
    sync::{Mutex, RwLock},
    time::interval,
};

pub async fn run_bot(
    executor: Arc<ArbitrageExecutor>,
    engine: Arc<ArbitrageEngine>,
    metrics: Arc<Mutex<Metrics>>,
    min_profit_threshold: f64,
    tx_fee_lamports: u64,
    simulation_mode: bool,
    pools: Arc<RwLock<std::collections::HashMap<solana_sdk::pubkey::Pubkey, Arc<PoolInfo>>>>,
) {
    engine.start_services().await;

    let mut volatility_tracker = VolatilityTracker::new(20);
    let engine_handle = engine.clone();

    // Dynamic threshold updater
    let threshold_task = tokio::spawn(async move {
        let mut intv = interval(Duration::from_secs(10));
        loop {
            intv.tick().await;
            let simulated_price = 1.0; // TODO: pull from real price feed
            volatility_tracker.add_price(simulated_price);
            let vol = volatility_tracker.volatility();
            let new_threshold = recommended_min_profit_threshold(vol);
            engine_handle.set_min_profit_threshold(new_threshold).await;
            println!("📈 Dynamic threshold updated: {:.4}", new_threshold);
        }
    });

    // Arbitrage detection
    let multihop_opps = engine.discover_multihop_opportunities().await;
    println!(
        "[ENGINE API] Found {} multi-hop arbitrage opportunities",
        multihop_opps.len()
    );

    let profitable_opps: Vec<_> = multihop_opps
        .into_iter()
        .filter(|opp| opp.is_profitable(min_profit_threshold))
        .collect();

    for opp in &profitable_opps {
        opp.log_summary();
        opp.log_hop();
        let mut m = metrics.lock().await;
        m.log_opportunity(
            &TradingPair(opp.input_token.clone(), opp.output_token.clone()),
            opp.profit_pct,
        );
        m.record_arbitrage_opportunity(
            opp.profit_pct,
            &opp.input_token,
            &opp.output_token,
            opp.input_amount,
            opp.expected_output,
        );
    }

    let exec_futs = profitable_opps.iter().map(|opp| {
        let executor = executor.clone();
        let engine = engine.clone();
        let metrics = metrics.clone();
        async move {
            let pools_opt = engine.resolve_pools_for_opportunity(opp).await;
            let pools_vec = match pools_opt {
                Some(p) => p,
                None => {
                    println!("[ENGINE] Skipping: missing pool info");
                    return;
                }
            };

            let pool_refs: Vec<&PoolInfo> = pools_vec.iter().map(|arc| arc.as_ref()).collect();

            let (total_profit, total_slippage, total_fee) = calculate_multihop_profit_and_slippage(
                &pool_refs,
                opp.input_amount,
                &vec![true; opp.hops.len()],
                &vec![(None, None, None); opp.hops.len()],
            );

            let fee_lamports = total_fee as u64;
            if !engine.should_execute_trade(total_slippage, fee_lamports) {
                println!(
                    "[ENGINE] Skipping trade: slippage {:.5} or fee {} too high",
                    total_slippage, fee_lamports
                );
                return;
            }

            match executor.execute_multihop(opp).await {
                Ok(sig) => {
                    println!("✅ Executed multi-hop: txid={}", sig);
                    let mut m = metrics.lock().await;
                    m.record_execution_result(true, tx_fee_lamports as f64, 0.0);
                    m.log_trade_result(
                        (opp.input_token.as_str(), opp.output_token.as_str()),
                        opp.total_profit,
                        simulation_mode,
                    );
                }
                Err(e) => {
                    eprintln!("❌ Execution failed: {}", e);
                    let mut m = metrics.lock().await;
                    m.record_execution_result(false, tx_fee_lamports as f64, 0.0);
                    m.log_trade_result(
                        (opp.input_token.as_str(), opp.output_token.as_str()),
                        0.0,
                        simulation_mode,
                    );
                }
            }
        }
    });

    join_all(exec_futs).await;

    // Graceful shutdown
    tokio::signal::ctrl_c()
        .await
        .expect("Failed to listen for shutdown");

    threshold_task.abort();

    println!("🛑 Arbitrage bot stopped.");
    metrics.lock().await.summary();
}
