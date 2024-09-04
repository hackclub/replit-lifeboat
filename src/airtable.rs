use airtable_api::{Airtable, Record};
use anyhow::Result;
use chrono::{DateTime, Utc};
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};

static AIRTABLE: Lazy<Airtable> = Lazy::new(Airtable::new_from_env);
static TABLE: &str = "tblZABr7qbdjjZo1G";

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AirtableSyncedUser {
    #[serde(rename = "ID")]
    pub id: i64,

    #[serde(rename = "Username")]
    pub username: String,

    #[serde(rename = "Connection Token")]
    pub token: String,

    #[serde(rename = "Email")]
    pub email: String,

    #[serde(rename = "Status")]
    pub status: ProcessState,

    #[serde(rename = "R2 Link")]
    pub r2_link: String,

    #[serde(rename = "Failed Repl IDs")]
    pub failed_ids: String,

    #[serde(rename = "Started At")]
    pub started_at: Option<DateTime<Utc>>,

    #[serde(rename = "Finished At")]
    pub finished_at: Option<DateTime<Utc>>,

    #[serde(rename = "Repl Count")]
    pub repl_count: Option<isize>,

    #[serde(rename = "File Count")]
    pub file_count: Option<isize>,
}

pub async fn add_user(user: AirtableSyncedUser) -> bool {
    let record: Record<AirtableSyncedUser> = Record {
        id: "".into(),
        fields: user,
        created_time: None,
    };

    // get_records().await;

    AIRTABLE.create_records(TABLE, vec![record]).await.is_ok()
}

pub async fn get_records() -> Result<Vec<Record<AirtableSyncedUser>>> {
    // Get the current records from a table.
    let records: Vec<Record<AirtableSyncedUser>> = AIRTABLE
        .list_records(
            TABLE,
            "Grid view",
            vec![
                "ID",
                "Connection Token",
                "Username",
                "Email",
                "Status",
                "R2 Link",
                "Failed Repl IDs",
            ],
        )
        .await?;

    // Iterate over the records.
    // for (i, record) in records.clone().iter().enumerate() {
    //     println!("{} - {:#?}", i, record);
    // }
    Ok(records)
}

pub async fn update_records(records: Vec<Record<AirtableSyncedUser>>) -> Result<()> {
    AIRTABLE.update_records(TABLE, records).await?;

    Ok(())
}

use std::fmt;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum ProcessState {
    #[serde(rename = "Registered")]
    Registered,
    #[serde(rename = "Collecting repls")]
    CollectingRepls,
    #[serde(rename = "Collected")]
    Collected,

    /// The repls have been uploaded to R2 but the email to the user with the link hasn't been sent yet.
    #[serde(rename = "Waiting in R2")]
    WaitingInR2,

    /// The repls are ready to be downloaded and the email to the user with the R2 download link has been sent.
    #[serde(rename = "R2 link email sent")]
    R2LinkEmailSent,

    /// The repls have been downloaded by the user!
    #[serde(rename = "Downloaded repls")]
    DownloadedRepls,

    /// Some of the repls failed, but we're still giving them the successful ones.
    #[serde(rename = "Partially downloaded repls")]
    PartiallyDownloadedRepls,

    /// Shit's fucked.
    #[serde(rename = "Errored")]
    Errored,

    // Errored the entire download function
    #[serde(rename = "ErroredMain")]
    ErroredMain,

    /// Errored while trying to upload to R2
    #[serde(rename = "ErroredR2")]
    ErroredR2,

    /// The user didn't have any repls to download
    #[serde(rename = "NoRepls")]
    NoRepls,
}
impl Default for ProcessState {
    fn default() -> Self {
        Self::Registered
    }
}
impl fmt::Display for ProcessState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let value = match self {
            ProcessState::Registered => "Registered",
            ProcessState::CollectingRepls => "Collecting repls",
            ProcessState::Collected => "Collected",
            ProcessState::WaitingInR2 => "Waiting in R2",
            ProcessState::R2LinkEmailSent => "R2 link email sent",
            ProcessState::DownloadedRepls => "Downloaded repls",
            ProcessState::PartiallyDownloadedRepls => "Partially downloaded repls",
            ProcessState::Errored => "Errored",
            ProcessState::ErroredMain => "ErroredMain",
            ProcessState::ErroredR2 => "ErroredR2",
            ProcessState::NoRepls => "NoRepls",
        };
        write!(f, "{}", value)
    }
}
