use askama::Template;
use lettre::message::header::ContentType;

#[derive(Template)]
#[template(path = "test.html")]
pub struct TestTemplate<'a> {
    pub title: &'a str,
    pub name: &'a str,
}

#[derive(Template)]
#[template(path = "greet.html")]
pub struct GreetTemplate<'a> {
    pub username: &'a str,
}
pub async fn send_greet_email(to: &str, username: &str) -> Result<(), Box<dyn std::error::Error>> {
    super::send_email(
        to,
        "Your Replit⠕ export is on its way!".to_string(),
        GreetTemplate { username }.render()?,
        ContentType::TEXT_HTML,
    )
    .await
}

#[derive(Template)]
#[template(path = "partial_success.html")]
pub struct PartialSuccessTemplate<'a> {
    pub username: &'a str,
    pub repl_count_success: usize,
    pub repl_count_total: usize,
    pub link_export_download: &'a str,
    pub repl_ids_failed: Vec<String>,
}
pub async fn send_partial_success_email(
    to: &str,
    username: &str,
    repl_count_total: usize,
    repl_ids_failed: Vec<String>,
    link_export_download: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    super::send_email(
        to,
        "Your Replit⠕ export has (mostly) arrived!".to_string(),
        PartialSuccessTemplate {
            username,
            repl_count_success: repl_count_total - repl_ids_failed.len(),
            repl_count_total,
            link_export_download,
            repl_ids_failed,
        }
        .render()?,
        ContentType::TEXT_HTML,
    )
    .await
}
