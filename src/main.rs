use anyhow::Result;
use replit_takeout::{
    airtable::{self, ProcessState},
    email::send_email,
    replit_graphql::{ProfileRepls, QuickUser},
};
use std::time::Duration;

#[macro_use]
extern crate rocket;

mod crosisdownload;

#[launch]
async fn rocket() -> _ {
    env_logger::init();

    tokio::spawn(async {
        if let Err(err) = airtable_loop().await {
            error!("Airtable internal loop error, OH NO: {err}")
        }
    });

    dotenv::dotenv().ok();
    rocket::build().mount("/", routes![hello, signup])
}

#[get("/")]
fn hello() -> &'static str {
    "Hello, world!"
}

#[post("/signup?<token>&<custom_email>")]
async fn signup(token: String, custom_email: Option<String>) -> String {
    // Get the user info, add to the airtable, respond to them
    let user = match QuickUser::fetch(&token, None).await {
        Ok(user) => user,
        Err(e) => {
            log::error!(
                "Couldn't get the replit user info for token {}: {}",
                token,
                e
            );
            return "Sorry, but we couldn't get your replit user info".into();
        }
    };

    let email = custom_email.unwrap_or(user.email.clone());

    let at_user = user.clone();

    if !airtable::add_user(airtable::AirtableSyncedUser {
        id: user.id,
        username: at_user.username,
        token,
        email: email.clone(),
        status: airtable::ProcessState::Registered,
        r2_link: String::from("https://example.com"),
        failed_ids: String::from("none"),
    })
    .await
    {
        error!("Couldn't add {:?} to airtable", user);
        return format!("Sorry, {}! We couldn't add you to the queue for some reason. Please contact us at malted@hackclub.com!", user.username);
    }

    send_email(
        &email,
        "Your Replit⠕ export is on its way!".into(),
        format!("Heya {}!! Your Replit⠕ takeout 🥡 will be with you within a few days.\nPlease stand in line while our interns pack your order 📦.", user.username),
    )
    .await;

    format!(
        "You're signed up, {}! We've emailed you at {} with details.",
        user.username, email
    )
}

async fn airtable_loop() -> Result<()> {
    loop {
        let mut user;
        'mainloop: loop {
            let records = airtable::get_records().await?;
            for record in records {
                if record.fields.status == ProcessState::Registered {
                    user = record;
                    break 'mainloop;
                }
            }
            tokio::time::sleep(Duration::from_secs(30)).await;
        }

        if let Err(err) = ProfileRepls::download(&user.fields.token, user.clone()).await {
            error!("Error with `{}`'s download: {err:#?}", user.fields.username);

            user.fields.status = ProcessState::ErroredMain;
            airtable::update_records(vec![user.clone()]).await?;

            // user.fields.failed_ids = errored.join(",");

            send_email(
                &user.fields.email,
                "Your Replit⠕ export is slightly delayed :/".into(),
                format!("Hey {}, We have run into an issue processing your Replit⠕ takeout 🥡.\nWe will manually review and confirm that all your data is included. If you don't hear back again within a few days email malted@hackclub.com. Sorry for the inconvenience.", user.fields.username),
            )
            .await;
        }
    }
}
