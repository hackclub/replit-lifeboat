mod metadata;

use std::{sync::Arc, time::Duration};

use anyhow::{format_err, Result};
use crosis::{
    goval::{self, command::Body, Command},
    Channel, Client,
};
use log::{debug, error, info};
use metadata::CookieJarConnectionMetadataFetcher;
use reqwest::{cookie::Jar, header::HeaderMap};

// Files to ignore for history and commits
const NO_GO: [&str; 7] = [
    "node_modules",
    ".venv",
    ".pythonlibs",
    "target",
    "vendor",
    ".upm",
    ".cache",
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

    let mut files_list = vec![];

    // Scope all the temporary file stuff
    {
        let mut fres = gcsfiles
            .request(Command {
                body: Some(Body::Readdir(goval::File {
                    path: ".".to_string(),
                    ..Default::default()
                })),
                ..Default::default()
            })
            .await;
        let mut path = String::new();
        let mut to_check_dirs = vec![];

        loop {
            if let Some(Body::Files(files)) = fres.body {
                for file in files.files {
                    let fpath = if path.is_empty() {
                        file.path.clone()
                    } else {
                        path.clone() + "/" + &file.path
                    };

                    // Ignore no go files
                    if NO_GO.contains(&fpath.as_str()) {
                        continue;
                    }

                    match goval::file::Type::from_i32(file.r#type) {
                        Some(goval::file::Type::Directory) => to_check_dirs.push(fpath),
                        Some(goval::file::Type::Regular) => files_list.push(fpath),
                        _ => {
                            error!("bruh")
                        }
                    }
                }
            }

            if let Some(npath) = to_check_dirs.pop() {
                path = npath;

                fres = gcsfiles
                    .request(Command {
                        body: Some(Body::Readdir(goval::File {
                            path: path.clone(),
                            ..Default::default()
                        })),
                        ..Default::default()
                    })
                    .await;
                dbg!(&fres);
            } else {
                break;
            }
        }
    }

    let mut set = tokio::task::JoinSet::new();
    for file in files_list {
        let file_channel = client
            .open(
                "ot".to_string(),
                Some(format!("ot:{file}")),
                Some(goval::open_channel::Action::AttachOrCreate),
            )
            .await?;
        set.spawn(handle_file(file_channel, file, 0));
    }

    client.poke_buf().await;

    while let Some(res) = set.join_next().await {
        res??
    }

    client.destroy().await?;

    Ok(())
}

pub async fn handle_file(mut channel: Channel, filename: String, global_ts: u64) -> Result<()> {
    let otstatus = match channel.next().await.unwrap().body {
        Some(Body::Otstatus(otstatus)) => otstatus,
        _ => return Err(format_err!("Invalid Otstatus")),
    };

    let version = if let Some(linked_file) = otstatus.linked_file {
        otstatus.version
    } else {
        let res = channel
            .request(Command {
                body: Some(Body::OtLinkFile(goval::OtLinkFile {
                    file: Some(goval::File {
                        path: filename.clone(),
                        ..Default::default()
                    }),
                    ..Default::default()
                })),
                ..Default::default()
            })
            .await;

        let linkfileres = match channel.next().await.unwrap().body {
            Some(Body::OtLinkFileResponse(linkfileres)) => linkfileres,
            _ => return Err(format_err!("Invalid OtLinkFileResponse")),
        };

        linkfileres.version
    };

    info!("{filename} is on version #{version}");
    Ok(())
}
