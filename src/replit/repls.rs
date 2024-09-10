use std::collections::HashSet;

use super::{create_client, REPLIT_GQL_URL};
use anyhow::Result;
use graphql_client::{GraphQLQuery, Response};
use log::{debug, info, trace, warn};
use reqwest::{Client, StatusCode};
use tokio::time::{sleep, Duration};

type DateTime = String;
#[derive(GraphQLQuery)]
#[graphql(
    schema_path = "src/graphql/schema 7.graphql",
    query_path = "src/graphql/repls-query.graphql",
    response_derives = "Debug"
)]
struct ReplList;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Repl {
    pub id: String,
    pub title: String,
    pub slug: String,
    pub private: bool,
    pub url: String,
    pub time_created: String,
}
impl Repl {
    pub async fn fetch(token: &str, client_opt: Option<Client>) -> Result<HashSet<Repl>> {
        let client = create_client(&token.into(), client_opt)?;
        let mut all_repls = HashSet::new();
        let mut visited_folder_ids = HashSet::new();

        Self::fetch_recursive("", "", &client, &mut all_repls, &mut visited_folder_ids).await?;

        info!("got {} repls", all_repls.len());

        Ok(all_repls)
    }

    async fn fetch_recursive(
        path: &str,
        folder_id: &str,
        client: &Client,
        all_repls: &mut HashSet<Repl>,
        visited_folder_ids: &mut HashSet<String>,
    ) -> Result<()> {
        if !folder_id.is_empty() && visited_folder_ids.contains(folder_id) {
            info!("Skipping already visited folder: {} ({})", path, folder_id);
            return Ok(());
        }

        if !folder_id.is_empty() {
            visited_folder_ids.insert(folder_id.to_string());
        }

        info!("Traversing {} ({folder_id})", path);

        let mut cursor = None;
        let mut retry_count = 0;
        let max_retries = 5;

        loop {
            let folder_query = ReplList::build_query(repl_list::Variables {
                path: path.to_string(),
                starred: None,
                after: cursor.clone(),
            });

            let folder_data = loop {
                if retry_count >= max_retries {
                    return Err(anyhow::anyhow!("Max retries reached for path {path}"));
                }

                let response = client.post(REPLIT_GQL_URL).json(&folder_query).send().await;

                match response {
                    Ok(res) if res.status() == StatusCode::TOO_MANY_REQUESTS => {
                        let wait_time = Duration::from_secs(2u64.pow(retry_count));
                        warn!("Rate-limited - waiting {:?} before retrying", wait_time);
                        sleep(wait_time).await;
                        retry_count = (retry_count + 1).min(max_retries);
                        continue;
                    }
                    Ok(res) => break res,
                    Err(e) => {
                        warn!("Error fetching data: {:?}", e);
                        let wait_time = Duration::from_secs(2u64.pow(retry_count));
                        warn!("Waiting {:?} before retrying", wait_time);
                        sleep(wait_time).await;
                        retry_count = (retry_count + 1).min(max_retries);

                        continue;
                    }
                }
            };

            let folder_data = folder_data.text().await?;

            let folder: Response<repl_list::ResponseData> = serde_json::from_str(&folder_data)?;
            log::trace!("{path}-{:#?}", folder);

            let folder = folder
                .data
                .and_then(|data| data.current_user)
                .and_then(|user| user.repl_folder_by_path)
                .ok_or_else(|| anyhow::anyhow!("Failed to get folder data"))?;

            // Process subfolders
            for subfolder in folder.folders {
                Box::pin(Self::fetch_recursive(
                    &subfolder.pathnames.join("/"),
                    &subfolder.id,
                    client,
                    all_repls,
                    visited_folder_ids,
                ))
                .await?;
            }

            for repl in folder.repls.items {
                all_repls.insert(Repl {
                    id: repl.id,
                    title: repl.title,
                    slug: repl.slug,
                    private: repl.is_private,
                    url: repl.url,
                    time_created: repl.time_created,
                });

            }

            sleep(Duration::from_millis(250)).await;

            // Check for next page
            match folder.repls.page_info.next_cursor {
                Some(next_cursor) => cursor = Some(next_cursor),
                None => break,
            }
        }

        Ok(())
    }
}
