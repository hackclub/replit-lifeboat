use replit_takeout::{
    airtable,
    email::send_email,
    replit_graphql::{ProfileRepls, QuickUser},
};

#[macro_use]
extern crate rocket;

mod crosisdownload;

#[launch]
async fn rocket() -> _ {
    env_logger::init();
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
        email: email.clone(),
        status: airtable::ProcessState::Registered,
    })
    .await
    {
        error!("Couldn't add {:?} to airtable", user);
        return format!("Sorry, {}! We couldn't add you to the queue for some reason. Please contact us at malted@hackclub.com!", user.username);
    }

    send_email(
        &email,
        "Your Replit⠕ export is on its way!".into(),
        format!("Heya {}!! Your Replit⠕ takeout 🥡 will be with you soon.\nPlease stand in line while our interns pack your order 📦.", user.username),
    )
    .await;

    format!(
        "You're signed up, {}! We've emailed you at {} with details.",
        user.username, email
    )
}
