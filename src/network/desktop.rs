use reqwest::Client;
use serde_json::json;
use std::time::Duration;

use crate::models::UplinkMessage;

pub struct HttpClient {
    client: Client,
    base_url: String,
}

impl HttpClient {
    pub fn new(base_url: String) -> Self {
        let client = Client::builder()
            .timeout(Duration::from_secs(5))
            .build()
            .expect("Failed to create HTTP client");

        Self { client, base_url }
    }

    pub async fn send_uplink(&self, message: &UplinkMessage) -> Result<(), reqwest::Error> {
        let response = self
            .client
            .post(&self.base_url)
            .json(message)
            .header("User-Agent", "Uplink/0.1.0 (Platform; Desktop)")
            .header("Accept", "application/json")
            .send()
            .await?;

        if response.status().is_success() {
            let body = response.text().await?;
            println!("Response: {}", body);
        } else {
            println!("Request failed with status: {}", response.status());
        }

        Ok(())
    }
}
