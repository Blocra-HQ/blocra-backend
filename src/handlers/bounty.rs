use crate::{db::DbPool, models::*, utils::jwt, errors::AppError};
use actix_web::HttpRequest;
use uuid::Uuid;

pub async fn list_bounties(pool: &DbPool) -> Result<Vec<Bounty>, AppError> {
    let bounties = sqlx::query_as::<_, Bounty>(
        "SELECT * FROM bounties ORDER BY created_at DESC"
    )
    .fetch_all(pool)
    .await?;

    Ok(bounties)
}

pub async fn create_bounty(
    pool: &DbPool,
    req: &HttpRequest,
    payload: bounty::CreateBounty,
) -> Result<Bounty, AppError> {
    let user_id = jwt::extract_user_id(req)?;
    let bounty_id = Uuid::new_v4().to_string();

    let bounty = sqlx::query_as::<_, Bounty>(
        "INSERT INTO bounties (id, title, description, reward_amount, reward_token, 
         difficulty, category, created_by, deadline, max_participants, requirements)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
         RETURNING *"
    )
    .bind(&bounty_id)
    .bind(&payload.title)
    .bind(&payload.description)
    .bind(payload.reward_amount)
    .bind(&payload.reward_token)
    .bind(&payload.difficulty)
    .bind(&payload.category)
    .bind(&user_id)
    .bind(payload.deadline)
    .bind(payload.max_participants)
    .bind(payload.requirements)
    .fetch_one(pool)
    .await?;

    Ok(bounty)
}

pub async fn get_bounty(pool: &DbPool, id: &str) -> Result<Bounty, AppError> {
    let bounty = sqlx::query_as::<_, Bounty>(
        "SELECT * FROM bounties WHERE id = ?"
    )
    .bind(id)
    .fetch_one(pool)
    .await?;

    Ok(bounty)
}

pub async fn update_bounty(
    pool: &DbPool,
    id: &str,
    payload: bounty::UpdateBounty,
) -> Result<Bounty, AppError> {
    let mut query = String::from("UPDATE bounties SET updated_at = CURRENT_TIMESTAMP");
    let mut params: Vec<String> = vec![];

    if let Some(title) = payload.title {
        query.push_str(", title = ?");
        params.push(title);
    }
    if let Some(description) = payload.description {
        query.push_str(", description = ?");
        params.push(description);
    }
    if let Some(status) = payload.status {
        query.push_str(", status = ?");
        params.push(status);
    }

    query.push_str(" WHERE id = ? RETURNING *");
    
    let bounty = sqlx::query_as::<_, Bounty>(&query)
        .bind(id)
        .fetch_one(pool)
        .await?;

    Ok(bounty)
}

pub async fn delete_bounty(pool: &DbPool, id: &str) -> Result<(), AppError> {
    sqlx::query("DELETE FROM bounties WHERE id = ?")
        .bind(id)
        .execute(pool)
        .await?;

    Ok(())
}

pub async fn join_bounty(
    pool: &DbPool,
    req: &HttpRequest,
    bounty_id: &str,
) -> Result<BountyParticipant, AppError> {
    let user_id = jwt::extract_user_id(req)?;
    let participant_id = Uuid::new_v4().to_string();

    let participant = sqlx::query_as::<_, BountyParticipant>(
        "INSERT INTO bounty_participants (id, bounty_id, user_id)
         VALUES (?, ?, ?)
         RETURNING *"
    )
    .bind(&participant_id)
    .bind(bounty_id)
    .bind(&user_id)
    .fetch_one(pool)
    .await?;

    Ok(participant)
}

pub async fn get_participants(pool: &DbPool, bounty_id: &str) -> Result<Vec<BountyParticipant>, AppError> {
    let participants = sqlx::query_as::<_, BountyParticipant>(
        "SELECT * FROM bounty_participants WHERE bounty_id = ?"
    )
    .bind(bounty_id)
    .fetch_all(pool)
    .await?;

    Ok(participants)
}

pub async fn get_submissions(pool: &DbPool, bounty_id: &str) -> Result<Vec<Submission>, AppError> {
    let submissions = sqlx::query_as::<_, Submission>(
        "SELECT * FROM submissions WHERE bounty_id = ?"
    )
    .bind(bounty_id)
    .fetch_all(pool)
    .await?;

    Ok(submissions)
}

pub async fn submit_bounty(
    pool: &DbPool,
    req: &HttpRequest,
    bounty_id: &str,
    payload: serde_json::Value,
) -> Result<Submission, AppError> {
    let user_id = jwt::extract_user_id(req)?;
    let submission_id = Uuid::new_v4().to_string();
    let content = payload.get("content")
        .and_then(|v| v.as_str())
        .ok_or(AppError::BadRequest("Content required".to_string()))?;

    let submission = sqlx::query_as::<_, Submission>(
        "INSERT INTO submissions (id, bounty_id, user_id, content)
         VALUES (?, ?, ?, ?)
         RETURNING *"
    )
    .bind(&submission_id)
    .bind(bounty_id)
    .bind(&user_id)
    .bind(content)
    .fetch_one(pool)
    .await?;

    Ok(submission)
}
