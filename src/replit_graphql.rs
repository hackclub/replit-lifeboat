use graphql_client::{GraphQLQuery, Response};
use log::*;
use reqwest::{
    cookie::Jar,
    header::{self, HeaderMap},
    Client, Url,
};
use std::error::Error;
use std::sync::Arc;
use tokio::fs;

use serde::Deserialize;

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
    if client.is_some() {
        return Ok(client.expect("a client to be inside the option"));
    }

    Client::builder()
        .user_agent("Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/106.0.0.0 Safari/537.36")
        .default_headers(create_client_headers())
        .cookie_provider(create_client_cookie_jar(&token))
        .build()
}

#[derive(GraphQLQuery)]
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
        client_opt: Option<Client>,
    ) -> Result<Vec<profile_repls::ProfileReplsUserProfileReplsItems>, Box<dyn Error>> {
        let client = create_client(&token, client_opt)?;

        let current_user = QuickUser::fetch(&token, Some(client.clone())).await?;

        let repls_query = ProfileRepls::build_query(profile_repls::Variables {
            id: current_user.id,
            after: None,
        });

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

        if let Err(e) = repls_data_result {
            error!("Failed to deserialize JSON: {}", e);
            return Err(Box::new(e));
        }

        let repls = repls_data_result?
            .data
            .and_then(|data| data.user.map(|user| user.profile_repls.items))
            .ok_or_else(|| "Repls not found during download")?;

        Ok(repls)
    }

    pub async fn download(token: &String) -> Result<(), Box<dyn Error>> {
        let repls = Self::fetch(token, None).await?;

        fs::create_dir_all("repls").await?;

        for repl in repls {
            fs::create_dir(format!("repls/{}", repl.id)).await?;

            let location = format!("repls/{}/", &repl.id);

            crate::crosisdownload::download(
                create_client_headers(),
                create_client_cookie_jar(&token),
                repl.id.clone(),
                &repl.slug,
                location.clone(),
            )
            .await?;

            info!("Downloaded {} to {location}", repl.id)
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
