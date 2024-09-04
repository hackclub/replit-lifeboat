mod metadata;
pub mod util;

pub use util::make_zip;

use std::{
    io::ErrorKind,
    path::Path,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
};

use anyhow::{format_err, Result};
use crosis::{
    goval::{self, command::Body, Command, StatResult},
    Channel, Client,
};
use git2::{Repository, Signature, Time};
use log::{debug, error, trace, warn};
use metadata::CookieJarConnectionMetadataFetcher;
use ropey::Rope;
use serde::Serialize;
use time::OffsetDateTime;
// use serde::Serialize;
use tokio::{
    fs,
    io::AsyncWriteExt,
    sync::{OwnedSemaphorePermit, Semaphore},
};
use util::{do_ot, download_repl_zip, normalize_ts, recursively_flatten_dir};

// Files to ignore for history and commits
static NO_GO: [&str; 28] = [
    ".astro",
    ".cache",
    ".config",
    ".deno",
    ".DS_Store",
    ".next",
    ".pnp",
    ".pnp.js",
    ".pythonlibs",
    ".svelte-kit",
    ".venv",
    ".vercel",
    "__MACOSX",
    "__pycache__",
    "build",
    "coverage",
    "dist",
    "node_modules",
    "out",
    "package-lock.json",
    "pnpm-lock.yaml",
    "target",
    "tmp",
    "vendor",
    "venv",
    "yarn.lock",
    "zig-cache",
    "zig-out",
];

const MAX_FILE_PARALLELISM: usize = 20;

#[derive(Clone)]
pub struct DownloadLocations {
    pub main: String,
    pub git: String,
    pub staging_git: String,
    pub ot: String,
}

pub enum DownloadStatus {
    Full,
    NoHistory,
}

#[derive(Clone, Copy)]
pub struct ReplInfo<'a> {
    pub id: &'a str,

    /// The username of the owner of the repl
    pub username: &'a str,

    pub slug: &'a str,
}

pub async fn download(
    client: reqwest::Client,
    replinfo: ReplInfo<'_>,
    download_zip: &str,
    download_locations: DownloadLocations,
    ts_offset: i64,
    email: &str,
) -> Result<(DownloadStatus, usize)> {
    debug!("https://replit.com/replid/{}", replinfo.id);

    let file_count = Arc::new(AtomicUsize::new(0));

    if let Err(err) = download_crosis(
        client.clone(),
        replinfo,
        download_locations,
        ts_offset,
        email,
        file_count.clone(),
    )
    .await
    {
        warn!(
            "Failed to download repl history for {}::{} with error: {:#?}",
            replinfo.id, replinfo.slug, err
        );

        if let Err(err_download_zip) = download_repl_zip(client, replinfo, download_zip).await {
            if let Err(err_rm_zip) = fs::remove_file(download_zip).await {
                if err_rm_zip.kind() != ErrorKind::NotFound {
                    return Err(format_err!("Error downloading repl zip: {err_download_zip}, and error deleting failed download: {err_rm_zip}"));
                }
            }
            Err(format_err!(
                "Error downloading repl zip: {err_download_zip}"
            ))
        } else {
            Ok((
                DownloadStatus::NoHistory,
                file_count.load(Ordering::Relaxed),
            ))
        }
    } else {
        Ok((DownloadStatus::Full, file_count.load(Ordering::Relaxed)))
    }
}

pub async fn download_crosis(
    client: reqwest::Client,
    replinfo: ReplInfo<'_>,
    download_locations: DownloadLocations,
    ts_offset: i64,
    email: &str,
    file_count: Arc<AtomicUsize>,
) -> Result<()> {
    let client = Client::new(Box::new(CookieJarConnectionMetadataFetcher {
        client,
        replid: replinfo.id.to_string(),
    }));

    let close_watcher = client.close_recv.clone();

    tokio::select! {
        res = download_crosis_internal(client, replinfo, download_locations, ts_offset, email, file_count) => {
            res
        }
        data = close_watcher.recv() => {
            Err(format_err!("Websocket was closed: {data:#?}"))
        }
    }
}

async fn download_crosis_internal(
    mut client: Client,
    ReplInfo {
        id: replid,
        slug: replname,
        ..
    }: ReplInfo<'_>,
    download_locations: DownloadLocations,
    ts_offset: i64,
    email: &str,
    file_count: Arc<AtomicUsize>,
) -> Result<()> {
    // Will take up to a max of 2 minutes until it fails if ratelimited
    let mut chan0 = client.connect_max_retries_and_backoff(5, 3000, 2).await?;

    trace!("Connected to {replid}::{replname}");
    {
        let (connected_send, connected_read) = kanal::oneshot_async();
        let mut connected_send = Some(connected_send);
        tokio::spawn(async move {
            while let Ok(msg) = chan0.next().await {
                if let Some(body) = msg.body {
                    match body {
                        Body::Ping(_) | Body::Pong(_) => {}
                        Body::BootStatus(goval::BootStatus { stage, .. }) => {
                            if goval::boot_status::Stage::from_i32(stage)
                                == Some(goval::boot_status::Stage::Complete)
                            {
                                if let Some(send) = connected_send.take() {
                                    send.send(()).await.expect("Sender is alive");
                                }
                            }
                        }
                        _ => {
                            debug!("{body:#?}")
                        }
                    }
                }
            }
        });

        connected_read.recv().await?;
    }

    let gcsfiles_scan = client.open("gcsfiles".into(), None, None).await?;
    trace!("Obtained 1st gcsfiles for {replid}::{replname}");

    let (file_list_writer, file_list_reader) = kanal::unbounded_async();
    let (file_list_writer2, file_list_reader2) = kanal::unbounded_async();
    let file_finder_handle = tokio::spawn(async move {
        // let mut files_list = vec![];
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
                        format!("{}/{}", path.clone(), &file.path)
                    };

                    // Ignore no go files
                    if NO_GO.contains(&fpath.as_str()) {
                        continue;
                    }

                    match goval::file::Type::from_i32(file.r#type) {
                        Some(goval::file::Type::Directory) => {
                            if fpath == ".git" {
                                is_git = true;
                            } else if fpath == ".replit-takeout-otbackup" {
                                file_list_writer.send(None).await?;
                                return Err(format_err!(
                                    "Repl cannot already have `.replit-takeout-otbackup/` dir"
                                ));
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

                            if size > 50_000_000 {
                                warn!("{fpath} is larger than max download size of 50mb");
                            } else {
                                file_list_writer.send(Some(fpath.clone())).await?;

                                file_list_writer2.send(Some(fpath)).await?;
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

        file_list_writer.send(None).await?;

        // trace!("Obtained file list for {replid}::{replname}");

        Ok((file_list_writer, is_git))
    });

    let gcsfiles_download = client.open("gcsfiles".into(), None, None).await?;
    let file_list_reader3 = file_list_reader2.clone();
    trace!("Obtained 2nd gcsfiles for {replid}::{replname}");
    // Sadly have to clone if want main file downloads in parallel with ot downloads
    // Should test / benchmark if time is available.
    let main_download = download_locations.main.clone();
    let handle = tokio::spawn(async move {
        while let Ok(Some(path)) = file_list_reader2.recv().await {
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

            trace!("Downloaded {path}");
        }

        file_list_reader2.close();

        Ok(())
    });

    let gcsfiles_download = client.open("gcsfiles".into(), None, None).await?;
    trace!("Obtained 3rd gcsfiles for {replid}::{replname}");
    // Sadly have to clone if want main file downloads in parallel with ot downloads
    // Should test / benchmark if time is available.
    let main_download = download_locations.main.clone();
    let handle2 = tokio::spawn(async move {
        while let Ok(Some(path)) = file_list_reader3.recv().await {
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

            trace!("Downloaded {path}");
        }

        Ok(())
    });

    // if is_git {
    //     warn!("History -> git not currently supported for existing git repos")
    // }

    let semaphore = Arc::new(Semaphore::new(MAX_FILE_PARALLELISM));
    let mut set = tokio::task::JoinSet::new();

    while let Ok(Some(file)) = file_list_reader.recv().await {
        let permit = semaphore.clone().acquire_owned().await?;

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
            download_locations.staging_git.clone(),
            file.clone(),
            ts_offset,
            permit,
            file_count.clone(),
        ));

        // Poke 👉
        client.poke_buf().await;
    }

    // Poke 👉
    client.poke_buf().await;

    while let Some(res) = set.join_next().await {
        res??
    }

    let (writer, is_git) = file_finder_handle.await??;

    trace!("Read file history for {replid}::{replname}");

    handle.await??;
    handle2.await??;

    let secrets = client
        .open(
            "secrets".to_string(),
            Some("secretser".to_string()),
            Some(goval::open_channel::Action::AttachOrCreate),
        )
        .await?;

    let res = secrets
        .request(Command {
            body: Some(Body::SecretsGetRequest(goval::SecretsGetRequest {})),
            ..Default::default()
        })
        .await?;

    let dotenv_content = match res.body {
        Some(Body::SecretsGetResponse(goval::SecretsGetResponse { contents, .. })) => {
            Some(contents)
        }
        _ => {
            warn!("Invalid .env SecretsGetResponse: {:#?}", res.body);
            None
        }
    };

    trace!("Downloaded final file contents for {replid}::{replname}");
    writer.close();

    // if !is_git.load(atomic::Ordering::Relaxed) {
    build_git(
        download_locations.main,
        download_locations.staging_git,
        download_locations.git,
        download_locations.ot,
        ts_offset,
        email.to_string(),
        is_git,
        dotenv_content,
    )
    .await?;

    trace!("Built git repo from history snapshots for {replid}::{replname}");
    // } else {
    //     fs::remove_dir_all(download_locations.staging_git).await?;
    //     fs::remove_dir_all(download_locations.git).await?;
    // }

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
    client.destroy().await?;

    trace!("Disconnected from {replid}::{replname}");

    Ok(())
}

async fn handle_file(
    mut channel: Channel,
    local_filename: String,
    staging_dir: String,
    filename: String,
    global_ts: i64,
    permit: OwnedSemaphorePermit,
    file_count: Arc<AtomicUsize>,
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

        trace!("{filename} has no history...");
        fs::write(path, "[]").await?;

        return Ok(());
    }

    trace!("{filename} is on version #{version}");

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
                let staging_ts_path = format!("{staging_dir}{timestamp}/{filename}");
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

        let staging_ts_path_final = format!("{staging_dir}{timestamp}/{filename}");
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

    trace!("Downloaded history for {filename}");

    drop(permit);

    file_count.fetch_add(1, Ordering::Relaxed);

    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn build_git(
    main_dir: String,
    staging_dir: String,
    git_dir: String,
    ot_dir: String,
    global_ts: i64,
    email: String,
    git_already_exists: bool,
    dotenv_content: Option<String>,
) -> Result<()> {
    if git_already_exists {
        let files = recursively_flatten_dir(main_dir.clone()).await?;

        for file in files {
            let file = file.strip_prefix(&main_dir).unwrap_or(&file);

            let to_buf = format!("{git_dir}{file}");
            let to = Path::new(&to_buf);

            if let Some(parent) = to.parent() {
                fs::create_dir_all(parent).await?;
            }

            fs::rename(format!("{main_dir}/{file}"), to).await?;
        }
    }

    let git_dir2 = git_dir.clone();
    let email2 = email.clone();
    let mut repo = tokio::task::spawn_blocking(move || -> Result<Repository> {
        let repo;
        let author = Signature::new("Replit Takeout", &email2, &Time::new(global_ts, 0))?;

        if !git_already_exists {
            repo = git2::Repository::init(&git_dir2)?;

            let mut index = repo.index()?;
            let oid = index.write_tree()?;
            let tree = repo.find_tree(oid)?;

            repo.commit(Some("HEAD"), &author, &author, "Initial Commit", &tree, &[])?;
        } else {
            repo = git2::Repository::open(&git_dir2)?;

            let mut index = repo.index()?;
            let oid = index.write_tree()?;
            let tree = repo.find_tree(oid)?;

            let commit_oid = repo.commit(None, &author, &author, "Initial Commit", &tree, &[])?;

            let commit = repo.find_commit(commit_oid)?;

            let _ = repo.branch("replit-takeout-history", &commit, false)?;

            let refname = "replit-takeout-history";
            let (object, reference) = repo.revparse_ext(refname).expect("Object not found");

            repo.checkout_tree(&object, None)
                .expect("Failed to checkout");

            match reference {
                // gref is an actual reference like branches or tags
                Some(gref) => repo.set_head(gref.name().unwrap()),
                // this is a commit, not a reference
                None => repo.set_head_detached(object.id()),
            }
            .expect("Failed to set HEAD");
        };

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

    let mut last_snapshot = global_ts;
    for snapshot in timestamps {
        last_snapshot = snapshot;
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

        let email2 = email.clone();

        repo = tokio::task::spawn_blocking(move || -> Result<Repository> {
            {
                let mut index = repo.index()?;
                index.add_all(["."], git2::IndexAddOption::DEFAULT, None)?;
                index.write()?;

                let oid = index.write_tree()?;
                let parent_commit = repo.head()?.peel_to_commit()?;
                let tree = repo.find_tree(oid)?;

                // TODO: Put real email here
                let author = Signature::new("Replit Takeout", &email2, &Time::new(snapshot, 0))?;

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

    let files = recursively_flatten_dir(main_dir.clone()).await?;

    for file in files {
        let file = file.strip_prefix(&main_dir).unwrap_or(&file);

        let to_buf = format!("{git_dir}{file}");
        let to = Path::new(&to_buf);

        if let Some(parent) = to.parent() {
            fs::create_dir_all(parent).await?;
        }

        fs::rename(format!("{main_dir}/{file}"), to).await?;
    }

    let gitignore_path = format!("{git_dir}.gitignore");
    let gitignore_path = Path::new(&gitignore_path);

    if fs::try_exists(gitignore_path).await? {
        let mut writer = fs::OpenOptions::new()
            .append(true)
            .open(gitignore_path)
            .await?;

        writer
            .write_all(
                "\n\n# Replit Takeout Special Files\n.replit-takeout-otbackup/\n.env".as_bytes(),
            )
            .await?;
    } else {
        fs::write(
            &gitignore_path,
            "# Replit Takeout Special Files\n.replit-takeout-otbackup/\n.env",
        )
        .await?;
    }

    let email2 = email.clone();
    tokio::task::spawn_blocking(move || -> Result<()> {
        let mut index = repo.index()?;
        index.add_all(["."], git2::IndexAddOption::DEFAULT, None)?;
        index.write()?;

        let oid = index.write_tree()?;
        let parent_commit = repo.head()?.peel_to_commit()?;
        let tree = repo.find_tree(oid)?;

        // TODO: Put real email here
        let author = Signature::new("Replit Takeout", &email2, &Time::new(last_snapshot, 0))?;

        repo.commit(
            Some("HEAD"),
            &author,
            &author,
            "Final history snapshot",
            &tree,
            &[&parent_commit],
        )?;

        Ok(())
    })
    .await??;

    let dotenv_path = format!("{git_dir}.env");
    let dotenv_path = Path::new(&dotenv_path);

    if let Some(dotenv) = dotenv_content {
        fs::write(dotenv_path, &dotenv).await?;
    } else {
        fs::write(dotenv_path, b"").await?;
    }

    fs::remove_dir_all(&main_dir).await?;

    fs::rename(&git_dir, &main_dir).await?;

    fs::rename(&ot_dir, format!("{main_dir}/.replit-takeout-otbackup/")).await?;

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
