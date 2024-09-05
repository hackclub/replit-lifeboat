use replit_takeout::email;

#[tokio::main]
async fn main() {
    email::emails::send_greet_email("test@malted.dev", "test")
        .await
        .expect("an email to be sent");
}
