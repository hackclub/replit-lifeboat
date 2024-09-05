use replit_takeout::email;

#[tokio::main]
async fn main() {
    email::emails::send_greet_email("test@malted.dev", "malted")
        .await
        .expect("an email to be sent");

    email::emails::send_partial_success_email(
        "test@malted.dev",
        "malted",
        5,
        &vec![String::from("foo")],
        "https://google.com",
    )
    .await
    .expect("an email to be sent");

    email::emails::send_success_email("test@malted.dev", "malted", 5, "https://google.com")
        .await
        .expect("an email to be sent");

    email::emails::send_failed_no_repls_email("test@malted.dev", "malted")
        .await
        .expect("an email to be sent");

    email::emails::send_failure_email("test@malted.dev", "malted")
        .await
        .expect("an email to be sent");
}
