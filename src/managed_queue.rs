use replit_takeout::airtable;
use replit_takeout::{
    email::send_email,
    replit_graphql::{ProfileRepls, QuickUser},
};
use rocket::fairing::AdHoc;
use rocket::State;
use std::io::Error;

struct Tx(flume::Sender<String>);
struct Rx(flume::Receiver<String>);

use anyhow::{format_err, Result};

#[get("/push?<token>")]
async fn push(token: String, tx: &State<Tx>) -> String {
    let user = QuickUser::fetch(&token, None).await.expect("a quick user");

    let at_user = user.clone();
    if !airtable::add_user(airtable::AirtableSyncedUser {
        id: at_user.id,
        username: at_user.username,
        email: at_user.email.unwrap(),
        status: airtable::ProcessState::Registered,
    })
    .await
    {
        error!("Failed to add user {} to airtable", user.id);
        return format!(
            "Sorry, {}. We couldn't register you due to an error. Please try again later!",
            user.username
        );
    }

    let user_email = user.email.unwrap();

    let email_success = send_email(
        &user_email,
        "Your Replit⠕ export is coming :3".into(),
        format!("Heya {}!! Your Replit⠕ takeout 🥡 will be with you soon OvO\nPlease stand in line while our interns pack your order 📦.\n\n\nP.S. Did you know your Replit user ID is {}? Well now you know! :)", &user.username, user.id),
    ).await;

    if !email_success {
        error!("Couldn't send the initial email to {user_email}");
    }

    let name = user.username;
    format!("You're signed up, {name}! We've emailed you at {user_email} with details.")
    // tx.0.try_send(event).map_err(|_| Status::ServiceUnavailable)
}

#[get("/pop")]
fn pop(rx: &State<Rx>) -> Option<String> {
    rx.0.try_recv().ok()
}

pub fn stage() -> AdHoc {
    AdHoc::on_ignite("Managed Queue", |rocket| async {
        let (tx, rx) = flume::bounded(32);
        rocket
            .mount("/queue", routes![push, pop])
            .manage(Tx(tx))
            .manage(Rx(rx))
    })
}
