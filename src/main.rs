use replit_takeout::airtable;

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
