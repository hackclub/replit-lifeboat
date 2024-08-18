mod metadata;
mod util;

use std::{path::Path, sync::Arc, time::Duration};

use anyhow::{format_err, Result};
use crosis::{
    goval::{self, command::Body, Command, StatResult},
    Channel, Client,
};
use git2::{Repository, Signature, Time};
use log::{debug, error, info, warn};
use metadata::CookieJarConnectionMetadataFetcher;
use reqwest::{cookie::Jar, header::HeaderMap};
use ropey::Rope;
// use serde::Serialize;
use tokio::{fs, io::AsyncWriteExt};
use util::{do_ot, normalize_ts, recursively_flatten_dir};

// Files to ignore for history and commits
static NO_GO: [&str; 10] = [
    "node_modules",
    ".venv",
    ".pythonlibs",
    "target",
    "vendor",
    ".upm",
    ".cache",
    ".config",
    "zig-cache",
    "zig-out",
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

    let gcsfiles_scan = client.open("gcsfiles".into(), None, None).await?;
    info!("Obtained 1st gcsfiles for {replid}::{replname}");

    tokio::spawn(async move {
        let mut files_list = vec![];
        let mut is_git = false;

        let mut fres = gcsfiles_scan
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
                        Some(goval::file::Type::Regular) => {
                            let res = gcsfiles_scan
                                .request(Command {
                                    body: Some(Body::Stat(goval::File {
                                        path: fpath.clone(),
                                        ..Default::default()
                                    })),
                                    ..Default::default()
                                })
                                .await?;

                            let size = match res.body {
                                Some(Body::StatRes(StatResult { size, .. })) => size,
                                _ => return Err(format_err!("Invalid StatRes: {:#?}", res.body)),
                            };

                            if size > 50000000 {
                                warn!("{fpath} is larger than max download size of 50mb");
                            } else {
                                files_list.push(fpath);
                            }
                        }
                        _ => {
                            error!("bruh")
                        }
                    }
                }
            }

            if let Some(npath) = to_check_dirs.pop() {
                path = npath;

                fres = gcsfiles_scan
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

        Ok(())
    });

    info!("Obtained file list for {replid}::{replname}");

    let gcsfiles_download = client.open("gcsfiles".into(), None, None).await?;
    info!("Obtained 1st gcsfiles for {replid}::{replname}");
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

            let res = gcsfiles_download
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
        Some(download_locations.staging_git.clone())
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

    info!("Downloaded final file contents for {replid}::{replname}");

    if staging_loc.is_some() {
        build_git(
            download_locations.staging_git,
            download_locations.git,
            ts_offset,
        )
        .await?;

        info!("Built git repo from history snapshots for {replid}::{replname}");
    }

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

        info!("{filename} has no history...");
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

    // let mut new_history = vec![];

    // for item in history.packets {
    //     let mut ops = vec![];
    //     for op in item.op {
    //         ops.push(match op.op_component.unwrap() {
    //             goval::ot_op_component::OpComponent::Skip(amount) => OtOp::Skip(amount),
    //             goval::ot_op_component::OpComponent::Delete(amount) => OtOp::Delete(amount),
    //             goval::ot_op_component::OpComponent::Insert(text) => OtOp::Insert(text),
    //         })
    //     }

    //     let timestamp = item.committed.map(|ts| ts.seconds).unwrap_or(0);
    //     new_history.push(OtFetchPacket {
    //         ops,
    //         crc32: item.crc32,
    //         timestamp,
    //         ts_string: format!(
    //             "{}",
    //             OffsetDateTime::from_unix_timestamp(normalize_ts(timestamp, global_ts))?
    //         ),
    //         version: item.version,
    //     })
    // }

    // let path = Path::new(&local_filename);

    // if let Some(parent) = path.parent() {
    //     fs::create_dir_all(parent).await?;
    // }

    // fs::write(path, serde_json::to_string(&new_history)?).await?;

    info!("Downloaded history for {filename}");
    Ok(())
}

pub async fn build_git(staging_dir: String, git_dir: String, global_ts: i64) -> Result<()> {
    let git_dir2 = git_dir.clone();
    let mut repo = tokio::task::spawn_blocking(move || -> Result<Repository> {
        let repo = git2::Repository::init(&git_dir2)?;
        {
            let author = Signature::new(
                "Replit Takeout",
                "malted@hackclub.com",
                &Time::new(global_ts, 0),
            )?;

            let mut index = repo.index()?;
            let oid = index.write_tree()?;
            let tree = repo.find_tree(oid)?;

            repo.commit(Some("HEAD"), &author, &author, "Initial Commit", &tree, &[])?;
        }

        Ok(repo)
    })
    .await??;

    let mut timestamps: Vec<i64> = vec![];
    let mut reader = fs::read_dir(&staging_dir).await?;
    while let Some(entry) = reader.next_entry().await? {
        if entry.file_type().await?.is_dir() {
            timestamps.push(
                entry
                    .file_name()
                    .into_string()
                    .expect("This is only [0-9]*")
                    .parse()
                    .expect("Garunteed to parse"),
            )
        }
    }

    timestamps.sort_unstable();

    for snapshot in timestamps {
        let head = format!("{staging_dir}{snapshot}");
        let files = recursively_flatten_dir(head.clone()).await?;
        let head_prefix = head.clone() + "/";

        for file in files {
            let file = file.strip_prefix(&head_prefix).unwrap_or(&file);

            let to_buf = format!("{git_dir}{file}");
            let to = Path::new(&to_buf);

            if let Some(parent) = to.parent() {
                fs::create_dir_all(parent).await?;
            }

            fs::rename(format!("{head}/{file}"), to).await?;
        }

        repo = tokio::task::spawn_blocking(move || -> Result<Repository> {
            {
                let mut index = repo.index()?;
                index.add_all(["."], git2::IndexAddOption::DEFAULT, None)?;
                index.write()?;

                let oid = index.write_tree()?;
                let parent_commit = repo.head()?.peel_to_commit()?;
                let tree = repo.find_tree(oid)?;

                // TODO: Put real email here
                let author = Signature::new(
                    "Replit Takeout",
                    "malted@hackclub.com",
                    &Time::new(snapshot, 0),
                )?;

                repo.commit(
                    Some("HEAD"),
                    &author,
                    &author,
                    "History snapshot",
                    &tree,
                    &[&parent_commit],
                )?;
            }

            Ok(repo)
        })
        .await??;
    }

    fs::remove_dir_all(&staging_dir).await?;

    Ok(())
}

// #[derive(Serialize)]
// #[serde(rename_all = "camelCase")]
// struct OtFetchPacket {
//     ops: Vec<OtOp>,
//     crc32: u32,
//     timestamp: i64,
//     ts_string: String,
//     version: u32,
// }

// #[derive(Serialize)]
// #[serde(rename_all = "camelCase")]
// enum OtOp {
//     Insert(String),
//     Skip(u32),
//     Delete(u32),
// }
