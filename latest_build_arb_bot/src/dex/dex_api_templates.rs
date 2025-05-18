// src/dex/dex_api_templates.rs

#[allow(dead_code)]
pub fn get_http_client() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .user_agent("rhodes-arb-bot/1.0")
        .build()
        .expect("Failed to build HTTP client")
}
