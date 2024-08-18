use crate::airtable;
use rocket::fairing::AdHoc;
use rocket::State;

use replit_takeout::{email::send_email, replit_graphql::QuickUser};

struct Tx(flume::Sender<String>);
struct Rx(flume::Receiver<String>);

#[get("/push?<token>")]
async fn push(token: String, tx: &State<Tx>) -> String {
    let user = QuickUser::fetch(token).await.unwrap();

    let at_user = user.clone();
    let at_success = airtable::add_user(airtable::AirtableSyncedUser {
        id: at_user.id,
        username: at_user.username,
        email: at_user.email,
        status: airtable::ProcessState::Registered,
    })
    .await;
    info!("at_success: {at_success}");

    let success = send_email(
        user.email.unwrap_or("malted+replittakeoutdropped@hackclub.com".into()),
        "Your Replit⠕ export is coming :3".into(),
        format!("Heya {}!! Your Replit⠕ takeout 🥡 will be with you soon OvO\nPlease stand in line while our interns pack your order 📦.\n\n\nP.S. Did you know your Replit user ID is {}? Well now you know! :)", user.username, user.id),
    ).await;

    if success {
        String::from("hiiii")
    } else {
        String::from(":(")
    }
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
