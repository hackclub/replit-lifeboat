use super::emails::{GreetTemplate, PartialSuccessTemplate, TestTemplate};
use askama::Template;
use rocket::{get, response::content::RawHtml};

#[get("/test")]
pub fn test_test_email() -> RawHtml<String> {
    let hello = TestTemplate {
        title: "Hi there, tester!",
        name: "malted",
    };
    let html = hello
        .render()
        .unwrap_or("failed to render template".to_string());

    RawHtml(html)
}

#[get("/greet")]
pub fn greet_test_email() -> RawHtml<String> {
    let hello = GreetTemplate {
        username: "TestUser",
    };
    let html = hello
        .render()
        .unwrap_or("failed to render template".to_string());

    RawHtml(html)
}

#[get("/partial-success")]
pub fn partial_success_test_email() -> RawHtml<String> {
    let hello = PartialSuccessTemplate {
        username: "TestUser",
        repl_count_success: 20,
        repl_count_total: 22,
        link_export_download: "https://google.com",
        repl_ids_failed: vec!["one".into(), "two".into()],
    };
    let html = hello
        .render()
        .unwrap_or("failed to render template".to_string());

    RawHtml(html)
}
