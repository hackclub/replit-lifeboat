use anyhow::Result;
use awsregion::Region;
use futures::stream::{self, StreamExt};
use s3::creds::Credentials;
use s3::error::S3Error;
use s3::request::ResponseData;
use s3::Bucket;
use std::collections::HashMap;
use std::error::Error;
use std::sync::Arc;
use tokio::fs::File;
use tokio::io::{self, AsyncReadExt, AsyncSeekExt};
use tokio::sync::mpsc;

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

const CHUNK_SIZE: usize = 250 * 1024 * 1024; // 250 MiB
const CONCURRENT_UPLOADS: usize = 8;

pub async fn read_chunk(file_path: &str, start: usize) -> io::Result<Vec<u8>> {
    let mut file = File::open(file_path).await?;
    let mut buffer = vec![0; start + CHUNK_SIZE];

    file.seek(io::SeekFrom::Start(start as u64)).await?;
    file.read(&mut buffer).await?;

    Ok(buffer)
}

pub async fn upload(remote_path: String, local_path: String) -> Result<()> {
    /* Start the multipart upload. With the S3 multipart API, you start a
     * multipart upload, send the chunks (in any order - they have indices),
     * and then close out the upload. (Fun fact: S3 doesn't impose any limits
     * on how long this can take, but R2 imposes a 7 day limit.) */
    let upload_id = BUCKET
        .initiate_multipart_upload(&remote_path, "application/octet-stream")
        .await?
        .upload_id;

    let file_size = File::open(local_path.clone())
        .await?
        .metadata()
        .await?
        .len();
    let num_chunks = (file_size as f64 / CHUNK_SIZE as f64).ceil() as usize;

    let (part_tx, mut part_rx) = mpsc::channel(num_chunks);

    let upload_tasks = stream::iter(0..num_chunks)
        .map(|chunk_index| {
            let owned_upload_id = upload_id.to_owned();
            let part_tx = part_tx.clone();
            let local_path = local_path.clone();
            let remote_path = remote_path.clone();
            tokio::spawn(async move {
                let chunk = read_chunk(&local_path, chunk_index * CHUNK_SIZE).await.unwrap();

                if chunk.len() > 0 {
                    match BUCKET
                        .put_multipart_chunk(
                            chunk.to_vec(),
                            &remote_path,
                            (chunk_index + 1) as u32,
                            &owned_upload_id,
                            "application/octet-stream",
                        )
                        .await
                    {
                        Ok(part) => part_tx.send(part).await.unwrap(),
                        Err(put_multipart_chunk_err) => {
                            log::error!("Failed to put multipart chunk for {remote_path} (chunk {chunk_index} of {num_chunks}): {:?}", put_multipart_chunk_err);

                                if let Err(abort_upload_err) =
                                                        BUCKET.abort_upload(&remote_path, &owned_upload_id).await
                                                    {
                                                        log::error!(
                                                            "Failed to abort upload for {remote_path} (chunk {chunk_index} of {num_chunks}): {:?}",
                                                            abort_upload_err
                                                        );
                                                    }
                        }
                    }
                }
            })
        })
        .buffer_unordered(CONCURRENT_UPLOADS);

    upload_tasks
        .for_each(|res| async {
            if let Err(e) = res {
                eprintln!("Error in upload task: {:?}", e);
            }
        })
        .await;

    let mut parts = Vec::with_capacity(num_chunks);
    while let Some(part) = part_rx.recv().await {
        parts.push(part);
    }

    BUCKET
        .complete_multipart_upload(&remote_path, &upload_id, parts)
        .await?;

    Ok(())
}

pub async fn upload_string(remote_path: &str, payload: String) -> Result<ResponseData, S3Error> {
    BUCKET.put_object(remote_path, payload.as_bytes()).await
}

pub async fn get_file_contents<'a>(path: String) -> Option<Vec<u8>> {
    BUCKET
        .get_object(path)
        .await
        .map(|x| Some(x.to_vec()))
        .unwrap_or(None)
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
