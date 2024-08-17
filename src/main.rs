use graphql_client::{reqwest::post_graphql_blocking as post_graphql, GraphQLQuery, Response};
use log::*;
use reqwest::{blocking::Client, cookie::Jar, header, Url};
use std::error::Error;
use std::io::Write;

#[derive(GraphQLQuery)]
#[graphql(
    schema_path = "src/graphql/schema 7.graphql",
    query_path = "src/graphql/quickuser-query.graphql",
    response_derives = "Debug"
)]
pub struct QuickUserQuery;

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

fn main() -> Result<(), Box<dyn Error>> {
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
        .default_headers(headers)
        .cookie_provider(jar)
        .build()?;

    let user_data: Response<quick_user_query::ResponseData> = client
        .post(REPLIT_GQL_URL)
        .json(&QuickUserQuery::build_query(quick_user_query::Variables {}))
        .send()?
        .json()?;

    let username = user_data
        .data
        .and_then(|d| d.current_user)
        .map(|u| u.username);
    info!("Username: {:?}", username);

    let repls_query =
        ReplsDashboardReplFolderList::build_query(repls_dashboard_repl_folder_list::Variables {
            path: "".to_string(),
            starred: None,
            after: None,
        });

    // let repl_folder_data: Response<repls_dashboard_repl_folder_list::ResponseData> = client
    //     .post(REPLIT_GQL_URL)
    //     .json(&repls_query)
    //     .send()?
    //     .json()?;

    //#region Public repls
    let mut profile_repls_data: Response<profile_repls::ResponseData> = client
        .post(REPLIT_GQL_URL)
        .json(&ProfileRepls::build_query(profile_repls::Variables {
            after: None,
        }))
        .send()?
        .json()?;

    if let Some(profile_repls::ResponseData {
        user:
            Some(profile_repls::ProfileReplsUser {
                profile_repls: profile_repls::ProfileReplsUserProfileRepls { items, page_info },
            }),
    }) = profile_repls_data.data
    {
        std::fs::create_dir_all("repls")?;
        for repl in items.iter() {
            let url = format!("https://replit.com{}.zip", repl.url);
            info!("Downloading {} from {url}", repl.title);
            let bytes = client.get(url).send()?.bytes()?;
            debug!("{} is {} kB\n", repl.title, bytes.len() / 1_000);
            let mut file = std::fs::File::create(format!("repls/{}.zip", &repl.title))?;
            file.write_all(&bytes)?;
        }
    }

    //#endregion

    Ok(())
}
