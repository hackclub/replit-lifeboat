use anyhow::Result;
use awsregion::Region;
use futures::stream::{self, StreamExt};
use log::{debug, info};
use s3::creds::Credentials;
use s3::error::S3Error;
use s3::request::ResponseData;
use s3::Bucket;
use std::collections::HashMap;
use tokio::fs::File;
use tokio::io::{self, AsyncReadExt, AsyncSeekExt};

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

const CHUNK_SIZE: usize = 100 * 1024 * 1024; // 100 MiB
const CONCURRENT_UPLOADS: usize = 8;

pub async fn read_chunk(file_path: &str, start: usize, size: usize) -> io::Result<Box<[u8]>> {
    let mut file = File::open(file_path).await?;
    let mut buffer: Box<[u8]> = vec![0; size].into_boxed_slice();
    file.seek(io::SeekFrom::Start(start as u64)).await?;
    file.read_exact(&mut buffer).await?;

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

    let (part_tx, part_rx) = kanal::bounded_async(num_chunks + 1);
    let part_tx2 = part_tx.clone();

    let upload_tasks = stream::iter(0..num_chunks)
        .map(|chunk_index| {
            let owned_upload_id = upload_id.to_owned();
            let part_tx = part_tx2.clone();
            let local_path = local_path.clone();
            let remote_path = remote_path.clone();
            tokio::spawn(async move {
                let size = if num_chunks == chunk_index + 1 {
                    // Conversion is safe since the output would have to be < CHUNK_SIZE
                    // which fits into a usize
                    (file_size % CHUNK_SIZE as u64) as usize
                } else {
                    CHUNK_SIZE
                };
                let chunk = read_chunk(&local_path, chunk_index * CHUNK_SIZE, size)
                    .await
                    .unwrap();
                let amt = chunk.len();
                if !chunk.is_empty() {
                    debug!(
                        "Uploading: {amt}/{CHUNK_SIZE}={} - {}/{num_chunks}",
                        amt as f64 / CHUNK_SIZE as f64,
                        chunk_index + 1
                    );

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
                        Ok(part) => part_tx.send(Some(part)).await.unwrap(),
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

                    debug!(
                        "Uploaded: {amt}/{CHUNK_SIZE}={} - {}/{num_chunks}",
                        amt as f64 / CHUNK_SIZE as f64,
                        chunk_index + 1
                    );
                }
            })
        })
        .buffer_unordered(CONCURRENT_UPLOADS);

    info!("Starting upload of parts for {local_path} -> {remote_path}");

    upload_tasks
        .for_each(|res| async move {
            if let Err(e) = res {
                eprintln!("Error in upload task: {:?}", e);
            }
        })
        .await;

    info!("Uploading parts done for {local_path} -> {remote_path}");

    part_tx.send(None).await?;

    info!("Finalizing upload for {local_path} -> {remote_path}");

    let mut parts = Vec::with_capacity(num_chunks);
    while let Ok(Some(part)) = part_rx.recv().await {
        parts.push(part);
    }

    BUCKET
        .complete_multipart_upload(&remote_path, &upload_id, parts)
        .await?;

    info!("Upload complete for {local_path} -> {remote_path}");

    Ok(())
}

pub async fn upload_str(remote_path: &str, payload: &str) -> Result<ResponseData, S3Error> {
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
