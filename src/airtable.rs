use airtable_api::{Airtable, Record};
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};

static AIRTABLE: Lazy<Airtable> = Lazy::new(|| Airtable::new_from_env());
static TABLE: &str = "tblZABr7qbdjjZo1G";

enum Status {}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AirtableSyncedUser {
    #[serde(rename = "ID")]
    pub id: i64,

    #[serde(rename = "Username")]
    pub username: String,

    #[serde(rename = "Email")]
    pub email: String,

    #[serde(rename = "Status")]
    pub status: ProcessState,
}

pub async fn add_user(user: AirtableSyncedUser) -> bool {
    let record: Record<AirtableSyncedUser> = Record {
        id: "".into(),
        fields: AirtableSyncedUser {
            id: user.id,
            username: user.username,
            email: user.email,
            status: user.status,
        },
        created_time: None,
    };

    get_records().await;

    AIRTABLE.create_records(TABLE, vec![record]).await.is_ok()
}

pub async fn get_records() {
    // Get the current records from a table.
    let records: Vec<Record<AirtableSyncedUser>> = AIRTABLE
        .list_records(
            TABLE,
            "Grid view",
            vec!["ID", "Username", "Email", "Status"],
        )
        .await
        .unwrap();

    // Iterate over the records.
    for (i, record) in records.clone().iter().enumerate() {
        println!("{} - {:#?}", i, record);
    }
}

use std::fmt;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ProcessState {
    #[serde(rename = "Registered")]
    Registered,
    #[serde(rename = "Enqueued")]
    Enqueued,
    #[serde(rename = "Collecting repls")]
    CollectingRepls,
    #[serde(rename = "Collected")]
    Collected,
    #[serde(rename = "Waiting in R2")]
    WaitingInR2,
    #[serde(rename = "R2 link email sent")]
    R2LinkEmailSent,
    #[serde(rename = "Downloaded repls")]
    DownloadedRepls,
}

impl fmt::Display for ProcessState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let value = match self {
            ProcessState::Registered => "Registered",
            ProcessState::Enqueued => "Enqueued",
            ProcessState::CollectingRepls => "Collecting repls",
            ProcessState::Collected => "Collected",
            ProcessState::WaitingInR2 => "Waiting in R2",
            ProcessState::R2LinkEmailSent => "R2 link email sent",
            ProcessState::DownloadedRepls => "Downloaded repls",
        };
        write!(f, "{}", value)
    }
}
