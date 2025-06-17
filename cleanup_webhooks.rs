// Quick webhook cleanup script
use std::env;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Load environment
    dotenv::from_filename(".env.paper-trading").ok();
    
    let api_key = env::var("HELIUS_API_KEY")?;
    
    // List all webhooks
    let client = reqwest::Client::new();
    let url = format!("https://api.helius.xyz/v0/webhooks?api-key={}", api_key);
    
    let response = client.get(&url).send().await?;
    let webhooks: serde_json::Value = response.json().await?;
    
    println!("Current webhooks:");
    if let Some(webhooks_array) = webhooks.as_array() {
        for (i, webhook) in webhooks_array.iter().enumerate() {
            if let Some(id) = webhook.get("webhookID") {
                println!("{}. ID: {}", i + 1, id);
                if let Some(url) = webhook.get("webhookURL") {
                    println!("   URL: {}", url);
                }
                if let Some(programs) = webhook.get("accountAddresses") {
                    println!("   Programs: {}", programs);
                }
            }
        }
        
        if webhooks_array.len() > 0 {
            println!("\nTo delete all webhooks, set DELETE_ALL=true and run again");
            
            if env::var("DELETE_ALL").unwrap_or_default() == "true" {
                for webhook in webhooks_array {
                    if let Some(id) = webhook.get("webhookID").and_then(|v| v.as_str()) {
                        let delete_url = format!("https://api.helius.xyz/v0/webhooks/{}?api-key={}", id, api_key);
                        match client.delete(&delete_url).send().await {
                            Ok(_) => println!("✅ Deleted webhook: {}", id),
                            Err(e) => println!("❌ Failed to delete webhook {}: {}", id, e),
                        }
                    }
                }
            }
        } else {
            println!("No webhooks found.");
        }
    }
    
    Ok(())
}
