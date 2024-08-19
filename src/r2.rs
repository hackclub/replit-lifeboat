use awsregion::Region;
use s3::creds::Credentials;
use s3::error::S3Error;
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

pub async fn upload(file_path: String, contents: &[u8]) -> Result<(), S3Error> {
    let response_data = BUCKET.put_object(file_path, contents).await?;
    assert_eq!(response_data.status_code(), 200);

    // let response_data = bucket.get_object(file_path).await?;

    // let response_data = bucket
    //     .get_object_range(s3_path, 100, Some(1000))
    //     .await
    //     .unwrap();
    // assert_eq!(response_data.status_code(), 206);
    // let (head_object_result, code) = bucket.head_object(s3_path).await?;
    // assert_eq!(code, 200);
    // assert_eq!(
    //     head_object_result.content_type.unwrap_or_default(),
    //     "application/octet-stream".to_owned()
    // );

    // let response_data = bucket.delete_object(s3_path).await?;
    // assert_eq!(response_data.status_code(), 204);
    Ok(())
}

pub async fn get(r2_path: String, custom_filename: String) -> Result<String, S3Error> {
    let mut custom_queries = HashMap::new();
    custom_queries.insert(
        "response-content-disposition".into(),
        format!("attachment; filename=\"{custom_filename}\""),
    );

    // Valid for a week (in seconds) (60 * 60 * 24 * 7)
    BUCKET
        .presign_get(r2_path, 604_800, Some(custom_queries))
        .await
}
