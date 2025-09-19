use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(sqlx::FromRow, Serialize, Deserialize, Debug, Clone)]
pub struct Course {
    pub id: Uuid,
    pub title: String,
    pub org_identifier: Option<String>,
    pub launch_href: String,
    pub base_path: String, // relative to DATA_DIR, e.g. "courses/<uuid>"
    pub created_at: DateTime<Utc>,
}

#[derive(sqlx::FromRow, Serialize, Deserialize, Debug, Clone)]
pub struct Sco {
    pub id: Uuid,
    pub course_id: Uuid,
    pub identifier: String,
    pub launch_href: String,
    pub parameters: Option<String>,
    pub created_at: DateTime<Utc>,
}

#[derive(sqlx::FromRow, Serialize, Deserialize, Debug, Clone)]
pub struct Attempt {
    pub id: Uuid,
    pub course_id: Uuid,
    pub learner_id: String,
    pub sco_id: Option<Uuid>,
    pub status: String,
    pub started_at: Option<DateTime<Utc>>,
    pub finished_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct CreateAttemptReq {
    pub course_id: Uuid,
    pub learner_id: String,
    pub sco_id: Option<Uuid>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct RuntimeSetReq {
    pub element: String,
    pub value: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct RuntimeGetReq {
    pub element: String,
}

