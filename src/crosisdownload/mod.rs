mod metadata;
mod util;

use std::{path::Path, sync::Arc, time::Duration};

use anyhow::{format_err, Result};
use crosis::{
    goval::{self, command::Body, Command},
    Channel, Client,
};
use log::{debug, error, info, warn};
use metadata::CookieJarConnectionMetadataFetcher;
use reqwest::{cookie::Jar, header::HeaderMap};
use ropey::Rope;
use serde::Serialize;
use time::OffsetDateTime;
use tokio::{fs, io::AsyncWriteExt};
use util::{do_ot, normalize_ts};

// Files to ignore for history and commits
static NO_GO: [&str; 8] = [
    "node_modules",
    ".venv",
    ".pythonlibs",
    "target",
    "vendor",
    ".upm",
    ".cache",
    ".config",
];
const MAX_FILE_PARALLELISM: usize = 20;

pub struct DownloadLocations {
    pub main: String,
    pub git: String,
    pub staging_git: String,
    pub ot: String,
}

pub async fn download(
    headers: HeaderMap,
    jar: Arc<Jar>,
    replid: String,
    replname: &str,
    download_locations: DownloadLocations,
    ts_offset: i64,
) -> Result<()> {
    debug!("https://replit.com/replid/{}", &replid);

    let mut client = Client::new(Box::new(CookieJarConnectionMetadataFetcher {
        client: reqwest::Client::builder()
        .user_agent("Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/106.0.0.0 Safari/537.36")
        .default_headers(headers)
        .cookie_provider(jar)
        .build()?,
        replid: replid.clone(),
    }));

    let mut chan0 = client.connect().await?;

    info!("Connected to {replid}::{replname}");

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
    info!("Obtained gcsfiles for {replid}::{replname}");

    let mut files_list = vec![];
    let mut is_git = false;

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
            .await?;
        let mut path = String::new();
        let mut to_check_dirs = vec![];

        loop {
            if let Some(Body::Files(files)) = fres.body {
                for file in files.files {
                    let fpath = if path.is_empty() {
                        file.path
                    } else {
                        path.clone() + "/" + &file.path
                    };

                    // Ignore no go files
                    if NO_GO.contains(&fpath.as_str()) {
                        continue;
                    }

                    match goval::file::Type::from_i32(file.r#type) {
                        Some(goval::file::Type::Directory) => {
                            if fpath == ".git" {
                                is_git = true;
                            }
                            to_check_dirs.push(fpath)
                        }
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
                    .await?;
                debug!("Obtained file tree for path `{path}`: {:#?}", fres);
            } else {
                break;
            }
        }
    }

    info!("Obtained file list for {replid}::{replname}");

    // Sadly have to clone if want main file downloads in parallel with ot downloads
    // Should test / benchmark if time is available.
    let files_list2 = files_list.clone();
    let main_download = download_locations.main.clone();
    let handle = tokio::spawn(async move {
        for path in &files_list2 {
            let download_path = format!("{main_download}{path}");
            let download_path = Path::new(&download_path);

            if let Some(parent) = download_path.parent() {
                fs::create_dir_all(parent).await?;
            }

            let res = gcsfiles
                .request(Command {
                    body: Some(Body::Read(goval::File {
                        path: path.clone(),
                        ..Default::default()
                    })),
                    ..Default::default()
                })
                .await?;

            let content = match res.body {
                Some(Body::File(goval::File { content, .. })) => content,
                _ => return Err(format_err!("Invalid File.Content: {:#?}", res.body)),
            };

            fs::write(download_path, content).await?;

            info!("Downloaded {path}");
        }

        Ok(())
    });

    if is_git {
        warn!("History -> git not currently supported for existing git repos")
    }

    let staging_loc = if is_git {
        None
    } else {
        Some(download_locations.staging_git)
    };

    // Chunk file fetching to not open like 500 channels at the same time
    for chunk in files_list.chunks(MAX_FILE_PARALLELISM) {
        let mut set = tokio::task::JoinSet::new();
        for file in chunk {
            let file_channel = client
                .open(
                    "ot".to_string(),
                    Some(format!("ot:{file}")),
                    Some(goval::open_channel::Action::AttachOrCreate),
                )
                .await?;

            set.spawn(handle_file(
                file_channel,
                format!("{}{file}", download_locations.ot),
                staging_loc.clone(),
                file.clone(),
                ts_offset,
            ));
        }

        client.poke_buf().await;

        while let Some(res) = set.join_next().await {
            res??
        }
    }

    info!("Read file history for {replid}::{replname}");

    handle.await??;

    // let repo = tokio::task::spawn_blocking(move || {
    //     match git2::Repository::open(&download_locations.main) {
    //         Err(err) => {
    //             dbg!(err.code());
    //             None
    //         }
    //         Ok(repo) => Some(repo),
    //     }
    // })
    // .await?;
    // client.destroy().await?;

    info!("Disconnected from {replid}::{replname}");

    Ok(())
}

pub async fn handle_file(
    mut channel: Channel,
    local_filename: String,
    staging_dir: Option<String>,
    filename: String,
    global_ts: i64,
) -> Result<()> {
    // TODO: do other stuff l8r
    if filename.starts_with(".git") {
        return Ok(());
    }

    let res = channel.next().await.unwrap().body;

    let otstatus = match res {
        Some(Body::Otstatus(otstatus)) => otstatus,
        _ => return Err(format_err!("Invalid Otstatus: {:#?}", res)),
    };

    let version = if otstatus.linked_file.is_some() {
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
            .await?;

        let linkfileres = match res.body {
            Some(Body::OtLinkFileResponse(linkfileres)) => linkfileres,
            _ => return Err(format_err!("Invalid OtLinkFileResponse: {:#?}", res.body)),
        };

        linkfileres.version
    };

    if version == 0 {
        let path = Path::new(&local_filename);

        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).await?;
        }

        info!(
            "{filename} has no history... {path:#?} {:#?}",
            path.parent()
        );
        fs::write(path, "[]").await?;

        return Ok(());
    }

    info!("{filename} is on version #{version}");

    let res = channel
        .request(Command {
            body: Some(Body::OtFetchRequest(goval::OtFetchRequest {
                version_from: 1,
                version_to: version,
            })),
            ..Default::default()
        })
        .await?;

    let history = match res.body {
        Some(Body::OtFetchResponse(history)) => history,
        _ => return Err(format_err!("Invalid OtFetchResponse: {:#?}", res.body)),
    };

    // GIT STUFF!
    if let Some(staging) = staging_dir {
        if !history.packets.is_empty() {
            let mut contents = Rope::new();

            let mut timestamp = normalize_ts(
                history
                    .packets
                    .first()
                    .expect("Has to exist")
                    .committed
                    .as_ref()
                    .map(|ts| ts.seconds)
                    .unwrap_or(0),
                global_ts,
            );

            for packet in &history.packets {
                let new_ts = normalize_ts(
                    packet.committed.as_ref().map(|ts| ts.seconds).unwrap_or(0),
                    global_ts,
                );

                if new_ts != timestamp {
                    let staging_ts_path = format!("{staging}{timestamp}/{filename}");
                    let path = Path::new(&staging_ts_path);

                    if let Some(parent) = path.parent() {
                        fs::create_dir_all(parent).await?;
                    }

                    let mut file_writer = fs::OpenOptions::new()
                        .write(true)
                        .create(true)
                        .truncate(true)
                        .open(path)
                        .await?;

                    for chunk in contents.chunks() {
                        let bytes = chunk.as_bytes();

                        file_writer.write_all(bytes).await?;
                    }

                    file_writer.flush().await?;
                    file_writer.sync_data().await?;

                    drop(file_writer);

                    timestamp = new_ts;
                }

                do_ot(&mut contents, packet)?;
            }

            let staging_ts_path_final = format!("{staging}{timestamp}/{filename}");
            let path = Path::new(&staging_ts_path_final);

            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).await?;
            }

            let mut file_writer = fs::OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .open(&path)
                .await?;

            for chunk in contents.chunks() {
                let bytes = chunk.as_bytes();

                file_writer.write_all(bytes).await?;
            }

            file_writer.flush().await?;
            file_writer.sync_data().await?;

            drop(file_writer);
        }
    }

    let mut new_history = vec![];

    for item in history.packets {
        let mut ops = vec![];
        for op in item.op {
            ops.push(match op.op_component.unwrap() {
                goval::ot_op_component::OpComponent::Skip(amount) => OtOp::Skip(amount),
                goval::ot_op_component::OpComponent::Delete(amount) => OtOp::Delete(amount),
                goval::ot_op_component::OpComponent::Insert(text) => OtOp::Insert(text),
            })
        }

        let timestamp = item.committed.map(|ts| ts.seconds).unwrap_or(0);
        new_history.push(OtFetchPacket {
            ops,
            crc32: item.crc32,
            timestamp,
            ts_string: format!(
                "{}",
                OffsetDateTime::from_unix_timestamp(normalize_ts(timestamp, global_ts))?
            ),
            version: item.version,
        })
    }

    let path = Path::new(&local_filename);

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).await?;
    }

    fs::write(path, serde_json::to_string(&new_history)?).await?;

    info!("Downloaded history for {filename}");
    Ok(())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct OtFetchPacket {
    ops: Vec<OtOp>,
    crc32: u32,
    timestamp: i64,
    ts_string: String,
    version: u32,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
enum OtOp {
    Insert(String),
    Skip(u32),
    Delete(u32),
}
