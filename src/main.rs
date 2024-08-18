use anyhow::Result;
use crosisdownload::download;
use log::*;
use replit_takeout::replit_graphql;
use reqwest::{cookie::Jar, header, Client, Url};
use std::error::Error;
use tokio::fs;

#[macro_use]
extern crate rocket;
mod managed_queue;

mod crosisdownload;

#[launch]
async fn rocket() -> _ {
    env_logger::init();
    dotenv::dotenv().ok();

    rocket::build()
        .attach(managed_queue::stage())
        .mount("/", routes![hello])
}

#[get("/")]
fn hello() -> &'static str {
    "Hello, world!"
}
