// src/wallet/helper.rs

use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::{
    commitment_config::CommitmentConfig,
    pubkey::Pubkey,               // Added Signer trait import
    signature::{Keypair, Signer}, // Added Signer trait import
    transaction::Transaction,
    // Removed Instruction as it's implicitly used by create_associated_token_account
};
use spl_associated_token_account::{
    get_associated_token_address, instruction::create_associated_token_account,
};

/// Get or create associated token account for a given wallet and token mint
pub async fn get_or_create_ata(
    rpc: &RpcClient,
    payer: &Keypair, // Who pays for the account creation
    owner: &Pubkey,  // Owner of the ATA (wallet that owns the token)
    mint: &Pubkey,   // Token mint address (e.g., USDC)
) -> Result<Pubkey, Box<dyn std::error::Error>> {
    let ata = get_associated_token_address(owner, mint);

    let account_info_result = rpc
        .get_account_with_commitment(&ata, CommitmentConfig::confirmed())
        .await;

    // Check if account exists by seeing if get_account_with_commitment returns a value (Some) or not (None)
    if account_info_result.is_ok() && account_info_result.as_ref().unwrap().value.is_none() {
        // Account does not exist, create it
        let ix = create_associated_token_account(
            &payer.pubkey(), // fee payer
            &payer.pubkey(), // Associated Token Account program needs the fee payer's pubkey here too
            owner,           // wallet that will own the ATA
            mint,            // token mint
        );

        let blockhash = rpc.get_latest_blockhash().await?;

        let tx =
            Transaction::new_signed_with_payer(&[ix], Some(&payer.pubkey()), &[payer], blockhash);

        rpc.send_and_confirm_transaction(&tx).await?;
    } else if account_info_result.is_err() {
        // Propagate the error if get_account_with_commitment failed for other reasons
        account_info_result?;
    }

    Ok(ata)
}
