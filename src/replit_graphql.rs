use airtable_api::Record;
use graphql_client::{GraphQLQuery, Response};
use log::*;
use reqwest::{
    cookie::Jar,
    header::{self, HeaderMap},
    Client, Url,
};
use std::sync::Arc;
use std::{error::Error, time::Duration};
use time::OffsetDateTime;
use tokio::fs;

use serde::Deserialize;

use crate::{
    airtable::{self, AirtableSyncedUser, ProcessState},
    crosisdownload::make_zip,
    email::send_email,
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
        .user_agent("Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/106.0.0.0 Safari/537.36")
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
    pub email: String,
}

impl QuickUser {
    pub async fn fetch(token: &String, client_opt: Option<Client>) -> Result<Self, String> {
        let client = create_client(token, client_opt).map_err(|e| e.to_string())?;
        let user_data: String = client
            .post(REPLIT_GQL_URL)
            .json(&QuickUserQuery::build_query(quick_user_query::Variables {}))
            .send()
            .await
            .map_err(|e| e.to_string())?
            .text()
            .await
            .map_err(|e| e.to_string())?;

        debug!(
            "{}:{} Raw text quick user data: {user_data}",
            std::line!(),
            std::column!()
        );

        let user_data: Response<quick_user_query::ResponseData> =
            serde_json::from_str(&user_data).map_err(|e| format!("Failed to parse JSON: {}", e))?;

        let user_data = user_data.data;
        let id = user_data
            .clone()
            .and_then(|d| d.current_user)
            .map(|u| u.id)
            .ok_or_else(|| "Missing user id".to_string())?;
        let username = user_data
            .clone()
            .and_then(|d| d.current_user)
            .map(|u| u.username)
            .ok_or_else(|| "Missing username".to_string())?;
        let email = user_data
            .clone()
            .and_then(|d| d.current_user)
            .map(|u| u.email)
            .ok_or_else(|| "Missing email".to_string())?;

        Ok(Self {
            id,
            username,
            email,
        })
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
    pub async fn fetch(
        token: &String,
        id: i64,
        client_opt: Option<Client>,
        after: Option<String>,
    ) -> Result<
        (
            Vec<profile_repls::ProfileReplsUserProfileReplsItems>,
            Option<String>,
        ),
        Box<dyn Error + Sync + Send>,
    > {
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
            serde_json::from_str::<Response<profile_repls::ResponseData>>(&repls_data);

        let repls_data_result_2 = match repls_data_result {
            Ok(data) => data.data,
            Err(e) => {
                error!("Failed to deserialize JSON: {}", e);
                return Err(Box::new(e));
            }
        };

        let next_page = repls_data_result_2
            .as_ref()
            .and_then(|data| {
                data.user
                    .as_ref()
                    .map(|user| user.profile_repls.page_info.next_cursor.clone())
            })
            .ok_or("Page Info not found during download")?;

        let repls = repls_data_result_2
            .and_then(|data| data.user.map(|user| user.profile_repls.items))
            .ok_or("Repls not found during download")?;

        Ok((repls, next_page))
    }

    pub async fn download(
        token: &String,
        mut synced_user: Record<AirtableSyncedUser>,
    ) -> Result<(), Box<dyn Error + Sync + Send>> {
        synced_user.fields.status = ProcessState::CollectingRepls;
        airtable::update_records(vec![synced_user.clone()]).await?;

        let client = create_client(token, None)?;

        let current_user = QuickUser::fetch(token, Some(client.clone())).await?;

        fs::create_dir_all("repls").await?;
        fs::create_dir(format!("repls/{}", current_user.username)).await?;

        let (mut repls, mut cursor) = Self::fetch(token, current_user.id, None, None).await?;
        let mut i = 0;
        let mut j = 0;
        let mut errored = vec![];
        loop {
            fs::create_dir_all("repls").await?;

            for repl in repls {
                let main_location = format!("repls/{}/{}/", current_user.username, repl.slug);
                let git_location = format!("repls/{}/{}.git/", current_user.username, repl.slug);
                let staging_git_location =
                    format!("repls/{}/{}.gitstaging/", current_user.username, repl.slug);
                let ot_location =
                    format!("repls/{}/{}.otbackup/", current_user.username, repl.slug);

                fs::create_dir(&main_location).await?;
                fs::create_dir(&git_location).await?;
                fs::create_dir(&staging_git_location).await?;
                fs::create_dir(&ot_location).await?;

                let ts = OffsetDateTime::parse(
                    &repl.time_created,
                    &time::format_description::well_known::Rfc3339,
                )?;

                dbg!(ts);

                let download_job = crate::crosisdownload::download(
                    client.clone(),
                    repl.id.clone(),
                    &repl.slug,
                    crate::crosisdownload::DownloadLocations {
                        main: main_location.clone(),
                        git: git_location,
                        staging_git: staging_git_location,
                        ot: ot_location,
                    },
                    ts.unix_timestamp(),
                    current_user.email.clone(),
                );

                // At 30 minutes abandon the repl download
                match tokio::time::timeout(Duration::from_secs(60 * 30), download_job).await {
                    Err(_) => {
                        error!(
                            "Downloading {}::{} timed out after 30 minutes",
                            repl.id, repl.slug
                        );
                        errored.push(repl.id);
                    }
                    Ok(Err(err)) => {
                        error!(
                            "Downloading {}::{} failed with error: {err:#?}",
                            repl.id, repl.slug
                        );
                        errored.push(repl.id);
                    }
                    Ok(Ok(_)) => {
                        info!("Downloaded {}::{} to {}", repl.id, repl.slug, main_location);
                        j += 1;
                    }
                }

                i += 1;

                info!(
                    "Download stats ({}): {j} correctly downloaded out of {i} total attempted downloads", current_user.username
                );
            }

            if let Some(cursor_extracted) = cursor {
                let (repls2, cursor2) =
                    Self::fetch(token, current_user.id, None, Some(cursor_extracted)).await?;

                repls = repls2;
                cursor = cursor2;
            } else {
                break;
            }
        }

        let did_error = !errored.is_empty();

        if did_error {
            synced_user.fields.status = ProcessState::Errored;
            synced_user.fields.failed_ids = errored.join(",");

            send_email(
                &synced_user.fields.email,
                "Your Replit⠕ export is slightly delayed :/".into(),
                format!("Hey {}, We have run into an issue processing your Replit⠕ takeout 🥡.\nWe will manually review and confirm that all your data is included. If you don't hear back again within a few days email malted@hackclub.com. Sorry for the inconvenience.", synced_user.fields.username),
            )
            .await;
        } else {
            synced_user.fields.status = ProcessState::DownloadedRepls;
        }

        airtable::update_records(vec![synced_user.clone()]).await?;

        let path = format!("repls/{}", current_user.username);
        make_zip(path.clone(), format!("repls/{}.zip", current_user.username)).await?;
        fs::remove_dir_all(&path).await?;

        warn!(
            "User repls have been zipped into repls/{}.zip",
            current_user.username
        );

        if !did_error {
            synced_user.fields.status = ProcessState::WaitingInR2;
        }

        let zip_path = format!("repls/{}.zip", current_user.username);

        r2::upload(
            format!("export/{}.zip", current_user.username),
            &fs::read(&zip_path).await?,
        )
        .await?;

        fs::remove_file(&zip_path).await?;

        let link = r2::get(
            format!("export/{}.zip", current_user.username),
            format!("{}.zip", current_user.username),
        )
        .await?;

        synced_user.fields.r2_link = link.clone();

        airtable::update_records(vec![synced_user.clone()]).await?;

        if !did_error {
            send_email(
                &synced_user.fields.email,
                "Your Replit⠕ export is ready!".into(),
                format!("Heya {}!! Your Replit⠕ takeout 🥡 is ready to download.\n\nA zip file with all of your repls can be found at {}. This link will be valid for 7 days.", synced_user.fields.username, link),
            )
            .await;
            synced_user.fields.status = ProcessState::R2LinkEmailSent;

            airtable::update_records(vec![synced_user]).await?;
        }

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
