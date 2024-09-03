use tokio::fs;

use anyhow::Result;
use replit_takeout::crosisdownload::{util::download_repl_zip, ReplInfo};
use reqwest::{cookie::Jar, header, Client, Url};

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::init();

    let connect_sid = std::env::args().nth(1).expect("a token");
    let username = std::env::args().nth(2).expect("a username");
    let replslug = std::env::args().nth(3).expect("a repl slug");

    let cookie = &format!("connect.sid={connect_sid}; Domain=replit.com");
    let url = "https://replit.com/graphql".parse::<Url>().unwrap();

    let jar = std::sync::Arc::new(Jar::default());
    jar.add_cookie_str(cookie, &url);

    let mut headers = header::HeaderMap::new();
    headers.insert(
        "X-Requested-With",
        header::HeaderValue::from_static("XMLHttpRequest"),
    );
    headers.insert(
        reqwest::header::REFERER,
        header::HeaderValue::from_static("https://replit.com/~"),
    );

    let client = Client::builder()
        .user_agent(replit_takeout::utils::random_user_agent())
        .default_headers(headers.clone())
        .cookie_provider(jar.clone())
        .build()?;

    fs::create_dir_all(format!("repls/{}/", username)).await?;

    download_repl_zip(
        client,
        ReplInfo {
            id: "",
            username: &username,
            slug: &replslug,
        },
        &format!("repls/{}/{}.zip", username, replslug),
    )
    .await?;

    Ok(())
}
