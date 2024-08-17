mod metadata;

use std::{sync::Arc, time::Duration};

use anyhow::Result;
use crosis::{
    goval::{self, command::Body, Command, Ping, Pong},
    Client,
};
use log::debug;
use metadata::CookieJarConnectionMetadataFetcher;
use reqwest::{cookie::Jar, header::HeaderMap};

const NO_GO: [&str; 6] = [
    "node_modules",
    ".venv",
    ".pythonlibs",
    "target",
    "vendor",
    ".upm",
];
pub async fn download(
    headers: HeaderMap,
    jar: Arc<Jar>,
    replid: String,
    replname: &str,
    filepath: &str,
) -> Result<()> {
    debug!("https://replit.com/replid/{}", &replid);

    let mut client = Client::new(Box::new(CookieJarConnectionMetadataFetcher {
        client: reqwest::Client::builder()
        .user_agent("Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/106.0.0.0 Safari/537.36")
        .default_headers(headers)
        .cookie_provider(jar)
        .build()?,
        replid,
    }));

    let mut chan0 = client.connect().await?;

    dbg!(chan0.id);

    tokio::spawn(async move {
        while let Ok(msg) = chan0.next().await {
            if let Some(body) = msg.body {
                match body {
                    Body::Ping(_) | Body::Pong(_) => {}
                    _ => {
                        debug!("{body:#?}")
                    }
                }
            }
        }
    });

    // I hate this but it's needed
    tokio::time::sleep(Duration::from_secs(3)).await;

    let gcsfiles = client.open("gcsfiles".into(), None, None).await?;
    dbg!(gcsfiles.id);

    let res = gcsfiles
        .request(Command {
            body: Some(Body::Readdir(goval::File {
                path: ".".to_string(),
                ..Default::default()
            })),
            ..Default::default()
        })
        .await
        .unwrap();

    dbg!(res);

    // client.close().await?;

    Ok(())
}
