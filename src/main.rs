use std::error::Error;
use graphql_client::{reqwest::post_graphql_blocking as post_graphql, GraphQLQuery, Response};
use reqwest::{cookie::Jar, Url, blocking::Client, header};
use log::*;

#[derive(GraphQLQuery)]
#[graphql(
    schema_path = "src/graphql/schema 7.graphql",
    query_path = "src/graphql/user-query.graphql",
    response_derives = "Debug",
)]
pub struct UserQuery;

#[derive(GraphQLQuery)]
#[graphql(
    schema_path = "src/graphql/schema 7.graphql",
    query_path = "src/graphql/repls-query.graphql",
    response_derives = "Debug",
)]
pub struct ReplsQuery;

static REPLIT_GQL_URL: &str = "https://replit.com/graphql";

fn main() -> Result<(), Box<dyn Error>> {
    env_logger::init();

    let connect_sid = std::env::args().nth(1).expect("a token");

    let cookie = &format!("connect.sid={connect_sid}; Domain=replit.com");
    let url = "https://replit.com/graphql".parse::<Url>().unwrap();

    let jar = std::sync::Arc::new(Jar::default());
    jar.add_cookie_str(cookie, &url);

    let mut headers = header::HeaderMap::new();
    headers.insert("X-Requested-With", header::HeaderValue::from_static("XMLHttpRequest"));
    headers.insert(reqwest::header::REFERER, header::HeaderValue::from_static("https://replit.com/~"));

    let client = Client::builder()
        .user_agent("Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/106.0.0.0 Safari/537.36")
        .default_headers(headers)
        .cookie_provider(jar)
        .build()?;

    let mut current_user_res = client.post(REPLIT_GQL_URL).json(&UserQuery::build_query(user_query::Variables { })).send()?;
    let current_user_data: Response<user_query::ResponseData> = current_user_res.json()?;

    let current_user = current_user_data.data.and_then(|data| data.current_user).expect("the current user data");
    info!("{:#?}", current_user);
   
    let repls_query_vars = repls_query::Variables { id: Some(current_user.id) };
    let repls_query = ReplsQuery::build_query(repls_query_vars);

    let mut repls_res = client.post(REPLIT_GQL_URL).json(&repls_query).send()?;
    let repls_data: Response<repls_query::ResponseData> = repls_res.json()?;

    info!("{:#?}", repls_data);

    /*
    let response_body =
        post_graphql::<RepoView, _>(&client, "https://replit.com/graphql", variables).unwrap();

    info!("{:?}", response_body);

    let response_data: repo_view::ResponseData = response_body.data.expect("missing response data");

    let stars: Option<i64> = response_data
        .repository
        .as_ref()
        .map(|repo| repo.stargazers.total_count);

    println!("{}", stars.unwrap_or(0),);
*/
    Ok(())
}
