use anyhow::Result;
pub mod emails;
pub mod test_routes;

use lettre::{
    message::header::ContentType,
    transport::smtp::{
        authentication::{Credentials, Mechanism},
        PoolConfig,
    },
    AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor,
};

pub async fn send_email(
    to: &str,
    subject: String,
    body: String,
    content_type: ContentType,
) -> Result<()> {
    // "Malted <malted@hackclub.com>"
    let message = Message::builder()
        .from("Hack Club <malted@hackclub.com>".parse()?)
        .to(to.parse()?)
        .subject(subject)
        .header(content_type)
        .body(body)?;

    let creds = Credentials::new(
        dotenv::var("GMAIL_SMTP_USER")?,
        dotenv::var("GMAIL_SMTP_PASS")?,
    );

    let sender: AsyncSmtpTransport<Tokio1Executor> =
        AsyncSmtpTransport::<Tokio1Executor>::starttls_relay("smtp.gmail.com")?
            .credentials(creds)
            .authentication(vec![Mechanism::Plain])
            .pool_config(PoolConfig::new().max_size(20))
            .build();

    sender
        .send(message)
        .await
        .map(|_| ())
        .map_err(|err| err.into())
}
