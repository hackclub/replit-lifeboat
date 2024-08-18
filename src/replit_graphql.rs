use graphql_client::{GraphQLQuery, Response};
use log::*;
use reqwest::{cookie::Jar, header, Client, Url};
use std::error::Error;
use std::io::Write;

use serde::Deserialize;

static REPLIT_GQL_URL: &str = "https://replit.com/graphql";

#[derive(GraphQLQuery)]
#[graphql(
    schema_path = "src/graphql/schema 7.graphql",
    query_path = "src/graphql/quickuser-query.graphql",
    response_derives = "Debug,Clone"
)]
pub struct QuickUserQuery;

#[derive(Clone, Deserialize)]
pub struct QuickUser {
    pub id: i64,
    pub username: String,
    pub email: Option<String>,
}
impl QuickUser {
    pub async fn fetch(token: String) -> Result<Self, Box<dyn std::error::Error>> {
        let cookie = &format!("connect.sid={token}; Domain=replit.com");
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

        let user_data: reqwest::Response = client
            .post(REPLIT_GQL_URL)
            .json(&QuickUserQuery::build_query(quick_user_query::Variables {}))
            .send()
            .await?;
        let user_data_raw_text = user_data.text().await?;
        debug!(
            "{}:{} user_data_raw_text: {user_data_raw_text}",
            std::line!(),
            std::column!()
        );
        let user_data: Response<quick_user_query::ResponseData> =
            serde_json::from_str(&user_data_raw_text)?;
        let user_data = user_data.data;
        let id = user_data.clone().and_then(|d| d.current_user).map(|u| u.id);
        let username = user_data
            .clone()
            .and_then(|d| d.current_user)
            .map(|u| u.username);
        let email = user_data
            .clone()
            .and_then(|d| d.current_user)
            .map(|u| u.email);

        Ok(Self {
            id: id.expect("an id"),
            username: username.expect("a username"),
            email,
        })

        // Ok(username.unwrap_or("NOT FOUND :(((".to_string()))
    }
}

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

// #[tokio::main]
// async fn main() -> Result<(), Box<dyn Error>> {
//     let connect_sid = std::env::args().nth(1).expect("a token");

//     //#region Public repls
//     let mut profile_repls_data: Response<profile_repls::ResponseData> = client
//         .post(REPLIT_GQL_URL)
//         .json(&ProfileRepls::build_query(profile_repls::Variables {
//             after: None,
//         }))
//         .send()
//         .await?
//         .json()
//         .await?;

//     if let Some(profile_repls::ResponseData {
//         user:
//             Some(profile_repls::ProfileReplsUser {
//                 profile_repls: profile_repls::ProfileReplsUserProfileRepls { items, page_info },
//             }),
//     }) = profile_repls_data.data
//     {
//         std::fs::create_dir_all("repls")?;
//         for repl in items.iter() {
//             let url = format!("https://replit.com{}.zip", repl.url);
//             info!("Downloading {} from {url}", repl.title);
//             let bytes = client.get(url).send().await?.bytes().await?;
//             debug!("{} is {} kB\n", repl.title, bytes.len() / 1_000);
//             let mut file = std::fs::File::create(format!("repls/{}.zip", &repl.title))?;
//             file.write_all(&bytes)?;
//         }
//     }

//     //#endregion

//     Ok(())
// }
