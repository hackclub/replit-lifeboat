use graphql_client::{reqwest::post_graphql_blocking as post_graphql, GraphQLQuery, Response};
use log::*;
use reqwest::{blocking::Client, cookie::Jar, header, Url};
use std::error::Error;

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

    let mut repls_query = client.post(REPLIT_GQL_URL).json(&repls_query).send()?;
    let repls_data: Response<repls_dashboard_repl_folder_list::ResponseData> =
        repls_query.json()?;

    info!("{:#?}", repls_data);

    Ok(())
}
