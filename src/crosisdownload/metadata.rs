use async_trait::async_trait;
use crosis::{
    ConnectionMetadataFetcher, FetchConnectionMetadataError, FetchConnectionMetadataResult,
};
use reqwest::Client;

pub struct CookieJarConnectionMetadataFetcher {
    pub client: Client,
    pub replid: String,
}

#[async_trait]
impl ConnectionMetadataFetcher for CookieJarConnectionMetadataFetcher {
    async fn fetch(&self) -> FetchConnectionMetadataResult {
        let response = match self
            .client
            .post(format!(
                "https://replit.com/data/repls/{}/get_connection_metadata",
                self.replid
            ))
            .body("{}")
            .send()
            .await
        {
            Ok(resp) => resp,
            // TODO: log error once tracing
            Err(err) => {
                eprintln!("{}", err);
                return Err(FetchConnectionMetadataError::Abort);
            }
        };

        if response.status() != 200 {
            if response.status().as_u16() > 500 {
                return Err(FetchConnectionMetadataError::Retriable);
            }

            // TODO: log error once tracing

            match response.text().await.as_ref().map(|txt| txt.as_str()) {
                Ok("Repl temporarily unavailable") => {
                    eprintln!("Repl temporarily unavailable");
                    return Err(FetchConnectionMetadataError::Retriable);
                }
                Err(err) => {
                    eprintln!("{err:#?}");
                    return Err(FetchConnectionMetadataError::Abort);
                }
                _ => {
                    return Err(FetchConnectionMetadataError::Abort);
                }
            }
        }

        match response.json().await {
            Ok(resp) => Ok(resp),
            // TODO: log error once tracing
            Err(err) => {
                eprintln!("{}", err);
                return Err(FetchConnectionMetadataError::Abort);
            }
        }
    }
}
