use anyhow::Result;
use crosisdownload::{download, DownloadLocations};
use graphql_client::{GraphQLQuery, Response};
use log::*;
use reqwest::{cookie::Jar, header, Client, Url};
use time::{format_description::well_known::Rfc3339, OffsetDateTime};
use tokio::fs;

mod crosisdownload;

#[derive(GraphQLQuery)]
#[graphql(
    schema_path = "src/graphql/schema 7.graphql",
    query_path = "src/graphql/quickuser-query.graphql",
    response_derives = "Debug"
)]
pub struct QuickUserQuery;

type DateTime = String;

#[derive(GraphQLQuery)]
#[graphql(
    schema_path = "src/graphql/schema 7.graphql",
    query_path = "src/graphql/profilerepls-query.graphql",
    response_derives = "Debug"
)]
pub struct ProfileRepls;

#[derive(GraphQLQuery)]
#[graphql(
    schema_path = "src/graphql/schema 7.graphql",
    query_path = "src/graphql/replfolders-query.graphql",
    response_derives = "Debug"
)]
pub struct ReplsDashboardReplFolderList;

static REPLIT_GQL_URL: &str = "https://replit.com/graphql";

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::init();

    let connect_sid = std::env::args().nth(1).expect("a token");

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
        .user_agent("Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/106.0.0.0 Safari/537.36")
        .default_headers(headers.clone())
        .cookie_provider(jar.clone())
        .build()?;

    let user_data: Response<quick_user_query::ResponseData> = client
        .post(REPLIT_GQL_URL)
        .json(&QuickUserQuery::build_query(quick_user_query::Variables {}))
        .send()
        .await?
        .json()
        .await?;

    let current_user = match user_data.data.and_then(|d| d.current_user) {
        Some(user) => user,
        None => todo!(),
    };

    info!("Username: {:?}", current_user.username);

    // let repls_query =
    //     ReplsDashboardReplFolderList::build_query(repls_dashboard_repl_folder_list::Variables {
    //         path: "".to_string(),
    //         starred: None,
    //         after: None,
    //     });

    // let repl_folder_data: Response<repls_dashboard_repl_folder_list::ResponseData> = client
    //     .post(REPLIT_GQL_URL)
    //     .json(&repls_query)
    //     .send()?
    //     .json()?;

    //#region Public repls
    let mut after = None;
    loop {
        let profile_repls_data: Response<profile_repls::ResponseData> = client
            .post(REPLIT_GQL_URL)
            .json(&ProfileRepls::build_query(profile_repls::Variables {
                after,
                id: current_user.id,
            }))
            .send()
            .await?
            .json()
            .await?;

        if let Some(profile_repls::ResponseData {
            user:
                Some(profile_repls::ProfileReplsUser {
                    profile_repls: profile_repls::ProfileReplsUserProfileRepls { items, page_info },
                }),
        }) = profile_repls_data.data
        {
            fs::create_dir("repls").await?;
            fs::create_dir(format!("repls/{}", current_user.username)).await?;

            for repl in items {
                // #[allow(deprecated)]
                // let repl = ProfileReplsUserProfileReplsItems {
                //     id: "19581655-7384-47c6-b100-d670b5349cb5".to_string(),
                //     slug: "Zig".to_string(),
                //     title: "Zig".to_string(),
                //     url: "/@PotentialStyx/Zig".to_string(),
                //     description: Some("Zig is a general-purpose programming language and toolchain for maintaining robust, optimal and reusable software.".to_string()),
                //     is_renamed: Some(true),
                //     is_always_on: false,
                //     is_project_fork: false,
                //     like_count: 7,
                //     language: "nix".to_string(),
                //     time_created: "2022-07-14T01:51:32.586Z".to_string(),
                // };
                // // TODO: remove this
                // if &repl.slug == "crosis-3" {
                //     continue; // Takes too long and makes testing a pita
                // }

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

                let ts = OffsetDateTime::parse(&repl.time_created, &Rfc3339)?;

                dbg!(ts);

                download(
                    headers.clone(),
                    jar.clone(),
                    repl.id.clone(),
                    &repl.slug,
                    DownloadLocations {
                        main: main_location.clone(),
                        git: git_location.clone(),
                        staging_git: staging_git_location.clone(),
                        ot: ot_location.clone(),
                    },
                    ts.unix_timestamp(),
                )
                .await?;

                info!(
                "Downloaded {} to {main_location} with git (if not already existant) in {git_location} and ots in {ot_location}",
                repl.id
            );

                // let url = format!("https://replit.com{}.zip", repl.url);
                // info!("Downloading {} from {url}", repl.title);
                // let bytes = client.get(url).send().await?.bytes().await?;
                // debug!("{} is {} kB\n", repl.title, bytes.len() / 1_000);
                // let mut file = std::fs::File::create(format!("repls/{}.zip", &repl.title))?;
                // file.write_all(&bytes)?;
            }
            if page_info.has_next_page {
                if let Some(cursor) = page_info.next_cursor {
                    after = Some(cursor);
                } else {
                    break;
                }
            } else {
                break;
            }
        } else {
            break;
        }
    }

    //#endregion

    Ok(())
}
