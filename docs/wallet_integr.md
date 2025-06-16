✅ GOAL:

Integrate wallet_pool.rs into jito_client.rs and submit atomic bundles from ephemeral wallets, sweeping profits to your collector wallet.

⸻

🧩 PART 1: Setup Checklist (Now)

✅ 1. Ensure Wallet Pool Module Is Fully Operational
	•	wallet_pool.rs handles:
	•	Ephemeral key generation
	•	TTL expiration
	•	Sweep logic

✅ 2. Finalize jito_client.rs with Submission Method

Make sure it has:
	•	A submit_bundle(transactions: Vec<Transaction>) method
	•	A fallback for retries (optional, but smart)

⸻

🛠️ PART 2: Execution Flow for Jito + Wallet Pool

Here’s how the integration workflow will look:

🔁 Loop (Arbitrage Engine or Executor)


// 1. Generate ephemeral wallet for current route
let wallet = wallet_pool.generate_wallet();
let trader = &wallet.keypair;

// 2. Prepare arbitrage instructions
let swap_ixs = vec![instr1, instr2, ...]; // from arb route

// 3. Create transaction signed by ephemeral wallet
let tx = Transaction::new_signed_with_payer(
    &swap_ixs,
    Some(&trader.pubkey()),
    &[trader],
    recent_blockhash,
);

// 4. Optional sweep tx (chained into bundle)
let balance = rpc.get_balance(&trader.pubkey()).await?;
let sweep_tx = if balance > 10000 {
    Some(wallet_pool.create_sweep_transaction(wallet, balance - 5000, recent_blockhash))
} else {
    None
};

// 5. Bundle txs and send via Jito
let mut bundle = vec![tx];
if let Some(sweep) = sweep_tx {
    bundle.push(sweep);
}

jito_client.submit_bundle(bundle).await?;

⸻

📋 PART 3: Final To-Do List

🔹 Code & Architecture
	•	✅ Refactor wallet_pool.rs for get_signing_wallet() with TTL logic
	•	✅ Add balance threshold logic to decide when to sweep
	•	✅ Ensure all sweep txs are ready to be bundled into Jito

🔹 Jito Integration
	•	✅ Prepare submit_bundle(transactions: Vec<Transaction>) API in jito_client.rs
	•	✅ Add default_tip_lamports field to JitoConfig if needed
	•	✅ Inject ephemeral wallet’s Keypair in the bundle signer logic
	•	✅ Handle bundle submission error/timeout with backoff or retries

🔹 Security & Performance
	•	✅ Don’t persist ephemeral keypairs to disk
	•	✅ Set up per-wallet metrics or logs for auditing
	•	✅ Use fast blockhash refresh (get_latest_blockhash) every N seconds

🔹 Future-Proofing
	•	⏳ Add AI path scoring or filter unstable paths pre-execution
	•	⏳ Rotate collector wallets over time for enhanced security
	•	⏳ Use burner Jito bundles (if you suspect sniffing)


src/
├── wallet/
│   ├── mod.rs
│   ├── wallet_pool.rs     // Generates wallets + sweeps
│   └── helper.rs          // ATA mgmt, token utilities
├── arbitrage/
│   ├── engine.rs          // Execution flow
│   └── jito_client.rs     // Bundle submission