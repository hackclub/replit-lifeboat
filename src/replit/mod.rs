use reqwest::{
    cookie::Jar,
    header::{self, HeaderMap},
    Client, Url,
};
use std::sync::Arc;

pub mod repls;

pub static REPLIT_GQL_URL: &str = "https://replit.com/graphql";

pub fn create_client(token: &String, client: Option<Client>) -> Result<Client, reqwest::Error> {
    if let Some(client) = client {
        return Ok(client);
    }

    Client::builder()
        .user_agent(crate::utils::random_user_agent())
        .default_headers(create_client_headers())
        .cookie_provider(create_client_cookie_jar(token))
        .build()
}

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
