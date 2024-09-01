use anyhow::Result;
use replit_takeout::r2;

#[tokio::main]
async fn main() -> Result<()> {
    let file = "styx-test/testing.bin";
    r2::upload(file.to_string(), "test-upload-10gb.bin".to_string()).await?;

    dbg!(r2::get(file.to_string(), file.to_string()).await?);
    Ok(())
}
