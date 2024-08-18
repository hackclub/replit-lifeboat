use lettre::{
    message::header::ContentType,
    transport::smtp::{
        authentication::{Credentials, Mechanism},
        PoolConfig,
    },
    AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor,
};

// use askama::Template;
// #[derive(Template)]
// #[template(path = "hello.html")]
// pub struct HelloTemplate<'a> {
//     pub title: &'a str,
//     pub name: &'a str,
// }

pub async fn send_email(to: String, subject: String, body: String) -> bool {
    // "Malted <malted@hackclub.com>"
    let email = Message::builder()
        .from("Hack Club <malted@hackclub.com>".parse().unwrap())
        .to(to.parse().expect("a valid email"))
        .subject(subject)
        .header(ContentType::TEXT_PLAIN)
        .body(body)
        .expect("a body");

    let creds = Credentials::new(
        dotenv::var("GMAIL_SMTP_USER")
            .expect("a gmail user in .env")
            .into(),
        dotenv::var("GMAIL_SMTP_PASS")
            .expect("a gmail smtp password in .env")
            .into(),
    );

    let sender: AsyncSmtpTransport<Tokio1Executor> =
        AsyncSmtpTransport::<Tokio1Executor>::starttls_relay("smtp.gmail.com")
            .expect("to start the tls relay")
            .credentials(creds)
            .authentication(vec![Mechanism::Plain])
            .pool_config(PoolConfig::new().max_size(20))
            .build();

    sender.send(email).await.is_ok()
}
