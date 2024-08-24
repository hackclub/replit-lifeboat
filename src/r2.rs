use awsregion::Region;
use bytes::BytesMut;
use s3::creds::Credentials;
use s3::error::S3Error;
use s3::serde_types::Part;
use s3::Bucket;
use std::pin::Pin;
use std::{collections::HashMap, sync::Arc};
use tokio::{
    io::{AsyncRead, AsyncReadExt},
    sync::Semaphore,
};

use anyhow::Result;

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

pub async fn upload(path: String, mut payload: Pin<&mut impl AsyncRead>) -> Result<()> {
    let start_mu_up = BUCKET
        .initiate_multipart_upload(&path, "application/octet-stream")
        .await?;

    let upload_id = start_mu_up.upload_id;

    // Max 1 GiB of upload in mem
    let semaphore = Arc::new(Semaphore::new(10));
    let mut handles: tokio::task::JoinSet<Result<Part>> = tokio::task::JoinSet::new();

    let mut idx = 0;
    loop {
        let permit = semaphore.clone().acquire_owned().await?;

        // 100 MiB chunks
        let mut buf = BytesMut::with_capacity(100 * 1024 * 1024);
        let amount = payload.read_buf(&mut buf).await?;

        if amount == 0 {
            break;
        }

        let path = path.clone();
        let upload_id = upload_id.clone();

        handles.spawn(async move {
            let chunk = &buf[0..amount];
            let res = BUCKET
                .put_multipart_chunk(
                    chunk.to_vec(),
                    &path,
                    (idx + 1) as u32,
                    &upload_id,
                    "application/octet-stream",
                )
                .await?;

            let _ = permit;

            Ok(res)
        });

        idx += 1;
    }

    let mut parts = vec![];

    while let Some(response) = handles.join_next().await {
        if response.is_err() {
            BUCKET.abort_upload(&path, &upload_id).await?;
        }

        parts.push(response??);
    }

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
