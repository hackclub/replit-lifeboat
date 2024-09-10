use std::collections::HashMap;

use anyhow::Result;
use dotenv::var;
use log::error;
use replit_takeout::replit::repls::Repl;

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::init();
    dotenv::dotenv().ok();

    let token = var("REPLIT_TEST_TOKEN")?;

    let repls = Repl::fetch(&token, None).await.expect("some repls");
    error!("got {} repls", repls.len());

    let mut map: HashMap<String, Repl> = HashMap::new();

    for repl in repls {
        if map.contains_key(&repl.id) {
            log::error!("ALREADY CONTAINS {:?}", repl.clone());
        }

        map.insert(repl.id.clone(), repl);
    }

    Ok(())
}
