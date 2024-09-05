use replit_takeout::airtable;

#[tokio::main]
async fn main() {
    let stats = airtable::aggregates().await.expect("some aggregate stats");
    println!("aggs: {:#?}", stats);
}
