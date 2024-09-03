use airtable_api::Record;
use anyhow::Result;
use graphql_client::{GraphQLQuery, Response};
use log::*;
use reqwest::{
    cookie::Jar,
    header::{self, HeaderMap},
    Client, Url,
};
use std::sync::Arc;
use std::time::Duration;
use time::OffsetDateTime;
use tokio::fs;

use serde::{Deserialize, Serialize};

use crate::{
    airtable::{self, AirtableSyncedUser, ProcessState},
    crosisdownload::{make_zip, DownloadLocations, DownloadStatus, ReplInfo},
    email::emails::{send_partial_success_email, send_success_email},
    r2,
};

static REPLIT_GQL_URL: &str = "https://replit.com/graphql";

fn create_client_headers() -> HeaderMap {
    let mut headers = header::HeaderMap::new();
    headers.insert(
        "X-Requested-With",
        header::HeaderValue::from_static("XMLHttpRequest"),
    );
    headers.insert(
        reqwest::header::REFERER,
        header::HeaderValue::from_static("https://replit.com/~"),
    );

    headers
}

fn create_client_cookie_jar(token: &String) -> Arc<Jar> {
    let cookie = &format!("connect.sid={token}; Domain=replit.com");
    let url = REPLIT_GQL_URL.parse::<Url>().unwrap();

    let jar = Jar::default();
    jar.add_cookie_str(cookie, &url);

    Arc::new(jar)
}

fn create_client(token: &String, client: Option<Client>) -> Result<Client, reqwest::Error> {
    if let Some(client) = client {
        return Ok(client);
    }

    Client::builder()
        .user_agent(crate::utils::random_user_agent())
        .default_headers(create_client_headers())
        .cookie_provider(create_client_cookie_jar(token))
        .build()
}

#[derive(GraphQLQuery, Clone)]
#[graphql(
    schema_path = "src/graphql/schema 7.graphql",
    query_path = "src/graphql/quickuser-query.graphql",
    response_derives = "Debug,Clone"
)]
pub struct QuickUserQuery;

#[derive(Clone, Debug, Deserialize)]
pub struct QuickUser {
    pub id: i64,
    pub username: String,
}

impl QuickUser {
    pub async fn fetch(token: &String, client_opt: Option<Client>) -> Result<Self> {
        let client = create_client(token, client_opt)?;
        let user_data: String = client
            .post(REPLIT_GQL_URL)
            .json(&QuickUserQuery::build_query(quick_user_query::Variables {}))
            .send()
            .await?
            .text()
            .await?;

        debug!(
            "{}:{} Raw text quick user data: {user_data}",
            std::line!(),
            std::column!()
        );

        let user_data: Response<quick_user_query::ResponseData> = serde_json::from_str(&user_data)?;

        let user_data = user_data.data;
        let id = user_data
            .clone()
            .and_then(|d| d.current_user)
            .map(|u| u.id)
            .ok_or_else(|| anyhow::Error::msg("Missing user id"))?;
        let username = user_data
            .clone()
            .and_then(|d| d.current_user)
            .map(|u| u.username)
            .ok_or_else(|| anyhow::Error::msg("Missing username"))?;

        Ok(Self { id, username })
    }
}

type DateTime = String;
#[derive(GraphQLQuery)]
#[graphql(
    schema_path = "src/graphql/schema 7.graphql",
    query_path = "src/graphql/profilerepls-query.graphql",
    response_derives = "Debug"
)]
pub struct ProfileRepls;

impl ProfileRepls {
    /// Get one page of repls.
    pub async fn fetch(
        token: &String,
        id: i64,
        client_opt: Option<Client>,
        after: Option<String>,
    ) -> Result<(
        Vec<profile_repls::ProfileReplsUserProfileReplsItems>,
        Option<String>,
    )> {
        let client = create_client(token, client_opt)?;

        let repls_query = ProfileRepls::build_query(profile_repls::Variables { id, after });

        let repls_data: String = client
            .post(REPLIT_GQL_URL)
            .json(&repls_query)
            .send()
            .await?
            .text()
            .await?;
        debug!(
            "{}:{} Raw text repl data: {repls_data}",
            std::line!(),
            std::column!()
        );

        let repls_data_result =
            match serde_json::from_str::<Response<profile_repls::ResponseData>>(&repls_data) {
                Ok(data) => data.data,
                Err(e) => {
                    error!("Failed to deserialize JSON: {}", e);
                    return Err(anyhow::Error::new(e));
                }
            };

        let next_page = repls_data_result
            .as_ref()
            .and_then(|data| {
                data.user
                    .as_ref()
                    .map(|user| user.profile_repls.page_info.next_cursor.clone())
            })
            .ok_or(anyhow::Error::msg("Page Info not found during download"))?;

        let repls = repls_data_result
            .and_then(|data| data.user.map(|user| user.profile_repls.items))
            .ok_or(anyhow::Error::msg("Repls not found during download"))?;

        Ok((repls, next_page))
    }

    pub async fn download(
        token: &String,
        mut synced_user: Record<AirtableSyncedUser>,
    ) -> Result<()> {
        synced_user.fields.status = ProcessState::CollectingRepls;
        airtable::update_records(vec![synced_user.clone()]).await?;

        let client = create_client(token, None)?;

        let current_user = QuickUser::fetch(token, Some(client.clone())).await?;

        fs::create_dir_all("repls").await?;
        fs::create_dir(format!("repls/{}", current_user.username)).await?;

        let mut repls = Vec::new();
        let mut cursor = None;

        loop {
            let (mut page_of_repls, new_cursor) =
                Self::fetch(token, current_user.id, None, cursor).await?;
            repls.append(&mut page_of_repls);

            if let Some(next_cursor) = new_cursor {
                cursor = Some(next_cursor);
            } else {
                break;
            }
        }

        let repl_count = repls.len();

        let mut progress = ExportProgress::new(repl_count);
        progress.report(&current_user); // Report the user's progress.

        if repl_count == 0 {
            if let Err(err) = crate::email::send_email(
                &synced_user.fields.email,
                "Your Replit⠕ export failed".into(),
                format!(
                    "Hey {}, you tried to export your repls, but you don't have any repls - what are you doing? Are you okay?? :)",
                    synced_user.fields.username
                ),
                lettre::message::header::ContentType::TEXT_PLAIN,
            ).await
            {
                error!("Couldn't send the 0 repl email to {}: {:?}", synced_user.fields.email, err);
            }

            synced_user.fields.status = ProcessState::NoRepls;
            airtable::update_records(vec![synced_user.clone()]).await?;
            return Ok(());
        }

        let mut total_download_count = 0;
        let mut successful_download_count = 0;
        let mut no_history_download_count = 0;

        let mut errored = vec![];
        for repl in repls {
            let main_location = format!("repls/{}/{}/", current_user.username, repl.slug);
            let git_location = format!("repls/{}/{}.git/", current_user.username, repl.slug);
            let staging_git_location =
                format!("repls/{}/{}.gitstaging/", current_user.username, repl.slug);
            let ot_location = format!("repls/{}/{}.otbackup/", current_user.username, repl.slug);

            fs::create_dir(&main_location).await?;
            fs::create_dir(&git_location).await?;
            fs::create_dir(&staging_git_location).await?;
            fs::create_dir(&ot_location).await?;

            let ts = OffsetDateTime::parse(
                &repl.time_created,
                &time::format_description::well_known::Rfc3339,
            )?;

            let download_zip = format!("repls/{}/{}.zip", current_user.username, repl.slug);
            let download_locations = DownloadLocations {
                main: main_location.clone(),
                git: git_location,
                staging_git: staging_git_location,
                ot: ot_location,
            };

            let download_job = crate::crosisdownload::download(
                client.clone(),
                ReplInfo {
                    id: &repl.id,
                    slug: &repl.slug,
                    username: &current_user.username,
                },
                &download_zip,
                download_locations.clone(),
                ts.unix_timestamp(),
                &synced_user.fields.email,
            );

            // At 30 minutes abandon the repl download
            match tokio::time::timeout(Duration::from_secs(60 * 30), download_job).await {
                Err(_) => {
                    error!(
                        "Downloading {}::{} timed out after 30 minutes",
                        repl.id, repl.slug
                    );
                    errored.push(repl.id);
                    progress.failed.timed_out += 1;
                }
                Ok(Err(err)) => {
                    error!(
                        "Downloading {}::{} failed with error: {err:#?}",
                        repl.id, repl.slug
                    );
                    errored.push(repl.id);
                    progress.failed.failed += 1;
                }
                Ok(Ok(DownloadStatus::NoHistory)) => {
                    info!(
                        "Downloaded {}::{} (without history) to {}",
                        repl.id, repl.slug, download_zip
                    );
                    no_history_download_count += 1;
                    progress.failed.no_history += 1;

                    if let Err(err) = fs::remove_dir_all(download_locations.git).await {
                        warn!(
                            "Error removing git temp dir for {}::{}: {err}",
                            repl.id, repl.slug
                        )
                    }

                    if let Err(err) = fs::remove_dir_all(download_locations.main).await {
                        warn!(
                            "Error removing main temp dir for {}::{}: {err}",
                            repl.id, repl.slug
                        )
                    }

                    if let Err(err) = fs::remove_dir_all(download_locations.ot).await {
                        warn!(
                            "Error removing ot temp dir for {}::{}: {err}",
                            repl.id, repl.slug
                        )
                    }

                    if let Err(err) = fs::remove_dir_all(download_locations.staging_git).await {
                        warn!(
                            "Error removing git staging temp dir for {}::{}: {err}",
                            repl.id, repl.slug
                        )
                    }
                }
                Ok(Ok(DownloadStatus::Full)) => {
                    info!("Downloaded {}::{} to {}", repl.id, repl.slug, main_location);
                    successful_download_count += 1;
                    progress.successful += 1;
                }
            }

            total_download_count += 1;

            info!(
                    "Download stats ({}): {successful_download_count} ({no_history_download_count} without history) correctly downloaded out of {total_download_count} total attempted downloads", current_user.username
                );

            progress.report(&current_user);
        }

        progress.completed = true;
        progress.report(&current_user);

        let path = format!("repls/{}", current_user.username);
        make_zip(path.clone(), format!("repls/{}.zip", current_user.username)).await?;
        fs::remove_dir_all(&path).await?;

        info!(
            "User repls have been zipped into repls/{}.zip",
            current_user.username
        );

        let zip_path = format!("repls/{}.zip", current_user.username); // Local
        let upload_path = format!("export/{}.zip", current_user.username); // Remote

        let upload_result = r2::upload(upload_path.clone(), zip_path.clone()).await;
        fs::remove_file(&zip_path).await?;
        synced_user.fields.status = ProcessState::WaitingInR2;
        airtable::update_records(vec![synced_user.clone()]).await?;

        if let Err(upload_err) = upload_result {
            synced_user.fields.status = ProcessState::ErroredR2;
            airtable::update_records(vec![synced_user.clone()]).await?;
            error!("Failed to upload {upload_path} to R2");
            return Err(upload_err);
        }

        let link = r2::get(upload_path, format!("{}.zip", current_user.username)).await?;

        synced_user.fields.r2_link = link.clone();

        // Hey, if even one repl was downloaded let's give it to them.
        if progress.successful + progress.failed.no_history > 0 {
            let full_success = progress.failed.failed + progress.failed.timed_out == 0;

            let email_result = if full_success {
                send_success_email(
                    &synced_user.fields.email,
                    &synced_user.fields.username,
                    repl_count,
                    &link,
                )
                .await
            } else {
                send_partial_success_email(
                    &synced_user.fields.email,
                    &synced_user.fields.username,
                    total_download_count,
                    &errored,
                    &link,
                )
                .await
            };

            match email_result {
                Ok(_) => {
                    synced_user.fields.status = ProcessState::R2LinkEmailSent;
                }
                Err(err) => {
                    error!(
                        "Couldn't send the (partial) success email to {}: {:?}",
                        synced_user.fields.email, err
                    );
                }
            }
        } else {
            // Shit's fucked.
            synced_user.fields.status = ProcessState::Errored;

            if let Err(err) = crate::email::send_email(
                &synced_user.fields.email,
                "Your Replit⠕ export failed".into(),
                format!(
                    "Hey {}, We have run into an issue processing your Replit⠕ takeout 🥡.
We've been notified, and will fix this! We'll get back to you about this.",
                    synced_user.fields.username
                ),
                lettre::message::header::ContentType::TEXT_PLAIN,
            )
            .await
            {
                error!(
                    "Couldn't send the failure email to {}: {:?}",
                    synced_user.fields.email, err
                );
            }
        }

        if !errored.is_empty() {
            synced_user.fields.failed_ids = errored.join(",");
        }
        airtable::update_records(vec![synced_user]).await?;

        Ok(())
    }
}

#[derive(GraphQLQuery)]
#[graphql(
    schema_path = "src/graphql/schema 7.graphql",
    query_path = "src/graphql/replfolders-query.graphql",
    response_derives = "Debug"
)]
pub struct ReplsDashboardReplFolderList;

#[derive(Default, Debug, Serialize, Deserialize)]
pub struct ExportProgress {
    completed: bool,

    /// The total number of repls the user has.
    repl_count: usize,
    successful: usize,
    failed: ExportProgressFailures,
}

impl ExportProgress {
    fn new(repl_count: usize) -> Self {
        Self {
            repl_count,
            ..Default::default()
        }
    }

    fn report(&self, user: &QuickUser) {
        let task_usr = user.clone();
        let progress = serde_json::to_string(self).expect("a serialised progress string");

        tokio::spawn(async move {
            if let Err(err) = r2::upload_str(&format!("progress/{}", task_usr.id), &progress).await
            {
                error!(
                    "Couldn't upload {}'s progress report ({progress}) to R2: {:?}",
                    task_usr.username, err
                );
            }
        });
    }
}

#[derive(Default, Debug, Serialize, Deserialize)]
struct ExportProgressFailures {
    /// The number of repls that have failed to download due to hitting the download timeout threshold.
    timed_out: usize,

    /// The number of repls that have failed to download for any other reason.
    failed: usize,

    /// The number of repls that have failed to download history, but a zip was successfully downloaded
    no_history: usize,
}
impl ExportProgressFailures {
    fn total(&self) -> usize {
        self.timed_out + self.failed + self.no_history
    }
}
