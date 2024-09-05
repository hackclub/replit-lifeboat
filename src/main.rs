#[macro_use]
extern crate rocket;
use anyhow::Result;
use base64::{
    alphabet,
    engine::{self, general_purpose},
    Engine as _,
};
use rand::Rng;
use replit_takeout::{
    airtable::{self, AggregateStats, ProcessState},
    replit_graphql::{ExportProgress, ProfileRepls, QuickUser},
};
use rocket::serde::json::Json;
use std::{collections::HashMap, time::Duration};
mod r2;

struct State {
    token_to_id_cache: tokio::sync::RwLock<HashMap<String, i64>>, // <token, id>
}

#[derive(serde::Serialize)]
struct SignupResponse {
    success: bool,
    message: String,
}
impl SignupResponse {
    fn good(message: String) -> Json<Self> {
        Json(Self {
            success: true,
            message,
        })
    }

    fn bad(message: String) -> Json<Self> {
        Json(Self {
            success: false,
            message,
        })
    }
}

#[launch]
async fn rocket() -> _ {
    env_logger::init();
    dotenv::dotenv().ok();

    // airtable::aggregates().await.expect("fialed to get aggs");
    tokio::spawn(async {
        loop {
            if let Err(err) = airtable_loop().await {
                error!("Airtable internal loop error (restarting): {err}");
            }
        }
    });

    rocket::build()
        .mount("/", routes![hello, signup, get_progress, get_stats])
        .manage(State {
            token_to_id_cache: tokio::sync::RwLock::new(HashMap::new()),
        })
}

#[get("/")]
fn hello() -> String {
    info!("Hit root route");

    format!(
        "Running {} v{}\n",
        env!("CARGO_PKG_NAME"),
        env!("CARGO_PKG_VERSION")
    )
}

#[post("/signup?<token>&<email>")]
async fn signup(token: String, email: String) -> Json<SignupResponse> {
    let parts: Vec<&str> = token.split('.').collect();

    if parts.len() != 3
        || engine::GeneralPurpose::new(&alphabet::URL_SAFE, general_purpose::NO_PAD)
            .decode(parts.get(1).unwrap())
            .is_err()
    {
        return SignupResponse::bad("That's not a Replit connect.sid".to_string());
    }

    // Get the user info, add to the airtable, respond to them
    let user = match QuickUser::fetch(&token, None).await {
        Ok(user) => user,
        Err(e) => {
            log::error!(
                "Couldn't get the replit user info for token {}: {}",
                token,
                e
            );
            return SignupResponse::bad("Couldn't get Replit user info".to_string());
        }
    };

    let at_user = user.clone();

    if !airtable::add_user(airtable::AirtableSyncedUser {
        id: user.id,
        username: at_user.username,
        token,
        email: email.clone(),
        status: airtable::ProcessState::Registered,
        r2_link: String::from("https://example.com"),
        failed_ids: String::from("none"),
        ..Default::default()
    })
    .await
    {
        error!("Couldn't add {:?} to airtable", user);
        return SignupResponse::bad(format!("Sorry, {}! We couldn't add you to the queue for some reason. Please contact us at malted@hackclub.com!", user.username));
    }

    if let Err(err) = replit_takeout::email::emails::send_greet_email(&email, &user.username).await
    {
        error!("Couldn't send the greeting email to {:?}: {:?}", user, err);
    }

    SignupResponse::good(format!("Check your email, {}!", user.username))
}

#[get("/progress?<token>")]
async fn get_progress(token: String, state: &rocket::State<State>) -> Option<Json<ExportProgress>> {
    let mut should_insert = false;

    let id = if let Some(id) = state.token_to_id_cache.read().await.get(&token) {
        *id
    } else {
        match QuickUser::fetch(&token, None).await {
            Ok(user) => {
                should_insert = true;
                user.id
            }
            Err(err) => {
                log::error!(
                    "Couldn't get the replit user info for token {token}: {:?}",
                    err
                );
                return None;
            }
        }
    };

    if should_insert {
        state.token_to_id_cache.write().await.insert(token, id);
    }

    if let Some(bytes) = r2::get_file_contents(format!("progress/{id}")).await {
        let str = std::str::from_utf8(&bytes).ok()?;
        let progress: ExportProgress = serde_json::from_str(str).ok()?;
        Some(Json(progress))
    } else {
        None
    }
}

#[get("/stats")]
async fn get_stats() -> Option<Json<AggregateStats>> {
    Some(Json(airtable::aggregates().await.ok()?))
}

async fn airtable_loop() -> Result<()> {
    let initial_wait = rand::thread_rng().gen_range(0..60);
    tokio::time::sleep(Duration::from_secs(initial_wait)).await;

    loop {
        let mut user;
        'mainloop: loop {
            debug!("Getting airtable records");
            let records = airtable::get_records().await?;
            for record in records {
                if record.fields.status == ProcessState::Registered {
                    user = record;
                    break 'mainloop;
                }
            }
            tokio::time::sleep(Duration::from_secs(10)).await;
        }

        if let Err(err) = ProfileRepls::download(&user.fields.token, user.clone()).await {
            error!("Error with `{}`'s download: {err:#?}", user.fields.username);

            user.fields.status = ProcessState::ErroredMain;
            airtable::update_records(vec![user.clone()]).await?;

            // user.fields.failed_ids = errored.join(",");

            // send_email(
            //     &user.fields.email,
            //     "Your Replit⠕ export is slightly delayed :/".into(),
            //     format!("Hey {}, We have run into an issue processing your Replit⠕ takeout 🥡.\nWe will manually review and confirm that all your data is included. If you don't hear back again within a few days email malted@hackclub.com. Sorry for the inconvenience.", user.fields.username),
            // )
            // .await;
        }
    }
}
