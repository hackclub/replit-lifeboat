use airtable_api::Record;
use anyhow::Result;
use chrono::Utc;
use dotenv::var;
use replit_takeout::{
    airtable::{self, AirtableSyncedUser, ProcessState},
    replit_graphql::ProfileRepls,
};

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::init();
    dotenv::dotenv().ok();

    let token = var("REPLIT_TEST_TOKEN")?;

    let fields = AirtableSyncedUser {
        id: 29999230,
        token,
        username: "malted".into(),
        status: ProcessState::Registered,
        email: "test@malted.dev".into(),
        r2_link: "http://example.com".into(),
        failed_ids: "none".into(),
        started_at: Some(Utc::now()),
        finished_at: None,
        repl_count: 0,
        file_count: 0,
        statistics: vec!["recpWEjc0zLoKEtZP".into()],
    };

    let mut user = Record {
        id: String::new(),
        fields,
        created_time: None,
    };

    log::info!("Starting...");
    if let Err(err) = ProfileRepls::download(&user.fields.token, user.clone()).await {
        log::error!("Error with `{}`'s download: {err:#?}", user.fields.username);

        user.fields.status = ProcessState::ErroredMain;
        airtable::update_records(vec![user.clone()]).await?;

        // send_email(
        //     &user.fields.email,
        //     "Your Replit⠕ export is slightly delayed :/".into(),
        //     format!("Hey {}, We have run into an issue processing your Replit⠕ takeout 🥡.\nWe will manually review and confirm that all your data is included. If you don't hear back again within a few days email malted@hackclub.com. Sorry for the inconvenience.", user.fields.username),
        // )
        // .await;
    }

    Ok(())
}
