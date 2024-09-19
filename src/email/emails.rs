use anyhow::Result;
use log::info;
use once_cell::sync::Lazy;
use reqwest::{
    header::{HeaderValue, AUTHORIZATION, CONTENT_TYPE},
    Client,
};
use serde_json::{json, Value};

static LOOPS_CLIENT: Lazy<Client> = Lazy::new(|| {
    let mut headers = reqwest::header::HeaderMap::new();
    headers.insert(
        AUTHORIZATION,
        HeaderValue::from_str(&format!(
            "Bearer {}",
            dotenv::var("LOOPS_API_KEY").expect("a loops api key in the env")
        ))
        .expect("a built str header value"),
    );

    Client::builder()
        .user_agent(crate::utils::random_user_agent())
        .default_headers(headers.clone())
        .build()
        .expect("a built loops client")
});
static LOOPS_TX_URL: &str = "https://app.loops.so/api/v1/transactional";

async fn send_loop(to: &str, payload: &Value) -> Result<reqwest::Response, reqwest::Error> {
    LOOPS_CLIENT.post(LOOPS_TX_URL).json(&payload).send().await
}

pub async fn send_greet_email(to: &str, username: &str) -> Result<()> {
    let payload = json!({
      "transactionalId": "cm0pegyzg01xquhjkf7r3fh85",
      "email": to,
      "dataVariables": {
        "replitUsername": username
      }
    });

    send_loop(to, &payload).await?;
    info!("Sent greet email to {to} ({username})");

    Ok(())
}

pub async fn send_partial_success_email(
    to: &str,
    username: &str,
    repl_count_total: usize,
    repl_ids_failed: &Vec<String>,
    link_export_download: &str,
) -> Result<()> {
    let failed_repl_links = repl_ids_failed
        .iter()
        .map(|id| format!("https://replit.com/replid/{id}"))
        .collect::<Vec<String>>()
        .join("\n");

    let payload = json!({
      "transactionalId": "cm0pgg7bw002gp8uk5ufqfvov",
      "email": to,
      "dataVariables": {
        "replitUsername": username,
        "replCountSuccess": repl_count_total - repl_ids_failed.len(),
        "replCountTotal": repl_count_total,
        "linkExportDownload": link_export_download,
        "failedReplLinks": failed_repl_links
      }
    });

    send_loop(to, &payload).await?;
    info!("Sent partial success email to {to} ({username})");

    Ok(())
}

pub async fn send_success_email(
    to: &str,
    username: &str,
    repl_count_total: usize,
    link_export_download: &str,
) -> Result<()> {
    let payload = json!({
      "transactionalId": "cm0pg42wh002u3ml1d4et51zp",
      "email": to,
      "dataVariables": {
        "replitUsername": username,
        "replCountTotal": repl_count_total,
        "link_export_download": link_export_download
      }
    });

    send_loop(to, &payload).await?;
    info!("Sent success email to {to} ({username})");

    Ok(())
}

pub async fn send_failed_no_repls_email(to: &str, username: &str) -> Result<()> {
    let payload = json!({
      "transactionalId": "cm0pgqger00bbo6ie7wyia98y",
      "email": to,
      "dataVariables": {
        "replitUsername": username
      }
    });

    send_loop(&to, &payload).await?;
    info!("Sent failed no repls email to {to} ({username})");

    Ok(())
}

pub async fn send_failure_email(to: &str, username: &str) -> Result<()> {
    let payload = json!({
      "transactionalId": "cm0pgxpy303hf53mv955aoqls",
      "email": to,
      "dataVariables": {
        "replitUsername": username
      }
    });

    send_loop(&to, &payload).await?;
    info!("Sent failure email to {to} ({username})");

    Ok(())
}
