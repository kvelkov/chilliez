// Quick test to verify environment loading works
use std::env;

fn main() {
    println!("Testing .env.paper-trading loading...");
    
    // Load the paper trading environment file
    match dotenv::from_filename(".env.paper-trading") {
        Ok(_) => println!("✅ Successfully loaded .env.paper-trading"),
        Err(e) => {
            println!("❌ Failed to load .env.paper-trading: {}", e);
            return;
        }
    }
    
    // Check for webhook URL
    match env::var("WEBHOOK_URL") {
        Ok(url) => println!("✅ WEBHOOK_URL found: {}", url),
        Err(_) => println!("❌ WEBHOOK_URL not found"),
    }
    
    // Check for other required vars
    match env::var("RPC_URL") {
        Ok(url) => println!("✅ RPC_URL found: {}", url),
        Err(_) => println!("❌ RPC_URL not found"),
    }
    
    match env::var("WS_URL") {
        Ok(url) => println!("✅ WS_URL found: {}", url),
        Err(_) => println!("❌ WS_URL not found"),
    }
}
