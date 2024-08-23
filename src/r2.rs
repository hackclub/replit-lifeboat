use awsregion::Region;
use s3::creds::Credentials;
use s3::error::S3Error;
use s3::serde_types::Part;
use s3::Bucket;
use std::collections::HashMap;

use once_cell::sync::Lazy;

static BUCKET: Lazy<Bucket> = Lazy::new(|| {
    let credentials = Credentials::new(None, None, None, None, None).expect("credentials");

    Bucket::new(
        "replit-takeout",
        Region::R2 {
            account_id: "90e2da927f7b2f6c30f10f86d1b5e679".to_string(),
        },
        credentials,
    )
    .expect("a bucket")
    .with_path_style()
});

pub async fn upload(path: String, payload: &[u8]) -> Result<(), S3Error> {
    let start_mu_up = BUCKET
        .initiate_multipart_upload(&path, "application/octet-stream")
        .await?;

    let upload_id = start_mu_up.upload_id;

    let mut etags = Vec::new();
    let mut handles = vec![];

    // 100 MiB chunks
    for (idx, chunk) in payload.chunks(100 * 1024 * 1024).enumerate() {
        handles.push(BUCKET.put_multipart_chunk(
            chunk.to_vec(),
            &path,
            (idx + 1) as u32,
            &upload_id,
            "application/octet-stream",
        ));
    }
    let responses = futures::future::join_all(handles).await;

    for response in responses {
        if response.is_err() {
            BUCKET.abort_upload(&path, &upload_id).await?;
        }

        etags.push(response?.etag);
    }

    let parts = etags
        .clone()
        .into_iter()
        .enumerate()
        .map(|(i, x)| Part {
            etag: x,
            part_number: i as u32 + 1,
        })
        .collect::<Vec<Part>>();

    BUCKET
        .complete_multipart_upload(&path, &upload_id, parts)
        .await?;

    Ok(())
}

pub async fn get(r2_path: String, custom_filename: String) -> Result<String, S3Error> {
    let mut custom_queries = HashMap::new();
    custom_queries.insert(
        "response-content-disposition".into(),
        format!("attachment; filename=\"{custom_filename}\""),
    );

    // Valid for 7 days
    BUCKET
        .presign_get(r2_path, 604_800, Some(custom_queries))
        .await
}
