use reqwest;
use serde_json::json;
use std::env; // For loading API key from .env

// Fetch AI-generated fixes for errors
pub async fn fetch_ai_suggestions(prompt: &str) -> Result<String, reqwest::Error> {
    // Load API key from .env file
    let api_key = env::var("OPENAI_API_KEY").expect("❌ Missing OPENAI_API_KEY in .env");

    let api_url = "https://api.openai.com/v1/completions";
    let client = reqwest::Client::new();

    let payload = json!({
        "model": "gpt-4.1",
        "prompt": prompt,
        "max_tokens": 500
    });

    let response = client
        .post(api_url)
        .header("Authorization", format!("Bearer {}", api_key))
        .json(&payload)
        .send()
        .await?
        .text()
        .await?;

    Ok(response)
}
