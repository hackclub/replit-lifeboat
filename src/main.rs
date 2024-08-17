use graphql_client::{reqwest::post_graphql_blocking as post_graphql, GraphQLQuery, Response};
use log::*;
use reqwest::{blocking::Client, cookie::Jar, header, Url};
use std::error::Error;

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

    /*let mut current_user_res = client.post(REPLIT_GQL_URL).json(&UserQuery::build_query(user_query::Variables { })).send()?;
    let current_user_data: Response<user_query::ResponseData> = current_user_res.json()?;

    let current_user = current_user_data.data.and_then(|data| data.current_user).expect("the current user data");
    info!("{:#?}", current_user);

    let repls_query_vars = repls_query::Variables { id: Some(current_user.id) };*/
    let repls_query =
        ReplsDashboardReplFolderList::build_query(repls_dashboard_repl_folder_list::Variables {
            path: "".to_string(),
            starred: None,
            after: None,
        });

    let mut repls_q = client.post(REPLIT_GQL_URL).json(&repls_query); //.build().expect("a req built");
                                                                      //    trace!("{:#?}", repls_q.body());
    let mut repls_res = repls_q.send()?;
    let repls_data: Response<repls_dashboard_repl_folder_list::ResponseData> = repls_res.json()?;

    info!("{:#?}", repls_data);

    Ok(())
}
