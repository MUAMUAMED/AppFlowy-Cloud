use actix_web::{web, Scope};
use app_error::AppError;
use chrono::{DateTime, Days, Duration, NaiveDate, TimeZone, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use shared_entity::response::{AppResponse, JsonAppResponse};
use sqlx::{PgPool, Postgres, Row, Transaction};
use uuid::Uuid;

use crate::{biz::authentication::jwt::UserUuid, state::AppState};

const XP_SCHEDULED: i32 = 5;
const XP_EXTRA: i32 = 2;
const XP_RELEARNING: i32 = 1;
const XP_COMPLETION: i32 = 20;

type ReviewResult<T> = std::result::Result<T, AppError>;

pub fn review_scope() -> Scope {
  web::scope("/api/review")
    .service(
      web::resource("/{workspace_id}/dashboard")
        .route(web::get().to(get_dashboard_handler)),
    )
    .service(
      web::resource("/{workspace_id}/taxonomy")
        .route(web::get().to(list_taxonomy_handler))
        .route(web::post().to(create_taxonomy_handler)),
    )
    .service(
      web::resource("/{workspace_id}/taxonomy/sync-pages")
        .route(web::post().to(sync_page_taxonomy_handler)),
    )
    .service(
      web::resource("/{workspace_id}/cards")
        .route(web::get().to(list_cards_handler))
        .route(web::post().to(create_card_handler)),
    )
    .service(
      web::resource("/{workspace_id}/cards/import")
        .route(web::post().to(import_cards_handler)),
    )
    .service(
      web::resource("/{workspace_id}/cards/{card_id}")
        .route(web::patch().to(update_card_handler)),
    )
    .service(
      web::resource("/{workspace_id}/cards/{card_id}/review")
        .route(web::post().to(review_card_handler)),
    )
}

#[derive(Debug, Clone, Serialize)]
struct TaxonomyNode {
  taxonomy_id: Uuid,
  parent_id: Option<Uuid>,
  kind: String,
  name: String,
  source_view_id: Option<Uuid>,
}

#[derive(Debug, Deserialize)]
struct CreateTaxonomyRequest {
  parent_id: Option<Uuid>,
  kind: String,
  name: String,
}

#[derive(Debug, Deserialize)]
struct SyncPageTaxonomyRequest {
  #[serde(default)]
  subjects: Vec<PageSubjectInput>,
}

#[derive(Debug, Deserialize)]
struct PageSubjectInput {
  view_id: Uuid,
  name: String,
  #[serde(default)]
  topics: Vec<PageTopicInput>,
}

#[derive(Debug, Deserialize)]
struct PageTopicInput {
  view_id: Uuid,
  name: String,
  #[serde(default)]
  contents: Vec<PageContentInput>,
}

#[derive(Debug, Deserialize)]
struct PageContentInput {
  view_id: Uuid,
  name: String,
}

#[derive(Debug, Clone, Serialize)]
struct ReviewCard {
  card_id: Uuid,
  card_type: String,
  front: String,
  back: String,
  explanation: String,
  choices: Value,
  correct_answer: String,
  subject_id: Option<Uuid>,
  topic_id: Option<Uuid>,
  content_id: Option<Uuid>,
  tags: Vec<String>,
  source_view_id: Option<Uuid>,
  source_block_id: Option<String>,
  state: String,
  due_at: DateTime<Utc>,
  interval_seconds: i64,
  difficulty_score: f64,
  correct_count: i32,
  incorrect_count: i32,
}

#[derive(Debug, Clone, Deserialize)]
struct CreateCardRequest {
  card_type: String,
  front: String,
  #[serde(default)]
  back: String,
  #[serde(default)]
  explanation: String,
  #[serde(default = "empty_choices")]
  choices: Value,
  #[serde(default)]
  correct_answer: String,
  subject_id: Option<Uuid>,
  topic_id: Option<Uuid>,
  content_id: Option<Uuid>,
  #[serde(default)]
  tags: Vec<String>,
  source_view_id: Option<Uuid>,
  source_block_id: Option<String>,
  #[serde(default)]
  timezone_offset_minutes: i32,
  initial_difficulty: Option<i16>,
}

#[derive(Debug, Deserialize)]
struct ImportCardsRequest {
  cards: Vec<CreateCardRequest>,
}

#[derive(Debug, Deserialize)]
struct UpdateCardRequest {
  front: Option<String>,
  back: Option<String>,
  explanation: Option<String>,
  choices: Option<Value>,
  correct_answer: Option<String>,
  subject_id: Option<Uuid>,
  topic_id: Option<Uuid>,
  content_id: Option<Uuid>,
  tags: Option<Vec<String>>,
  suspended: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct CardListQuery {
  subject_id: Option<Uuid>,
  topic_id: Option<Uuid>,
  content_id: Option<Uuid>,
  tag: Option<String>,
  card_type: Option<String>,
  #[serde(default)]
  due_only: bool,
  #[serde(default)]
  timezone_offset_minutes: i32,
}

#[derive(Debug, Deserialize)]
struct DashboardQuery {
  #[serde(default)]
  timezone_offset_minutes: i32,
}

#[derive(Debug, Serialize)]
struct DashboardResponse {
  review_date: NaiveDate,
  goal: i64,
  completed: i64,
  remaining: i64,
  overdue: i64,
  new_cards: i64,
  learning: i64,
  reviewed_today: i64,
  xp_today: i64,
  total_xp: i64,
  level: i64,
  current_streak: i32,
  longest_streak: i32,
  completion_awarded: bool,
  queue: Vec<ReviewCard>,
}

#[derive(Debug, Deserialize)]
struct ReviewRequest {
  difficulty: i16,
  selected_answer: Option<String>,
  correct: Option<bool>,
  response_time_ms: Option<i32>,
  #[serde(default)]
  timezone_offset_minutes: i32,
}

#[derive(Debug, Serialize)]
struct ReviewResponse {
  correct: bool,
  xp_awarded: i32,
  completion_bonus: i32,
  total_xp: i64,
  due_at: DateTime<Utc>,
  interval_seconds: i64,
  state: String,
  daily_goal: i64,
  daily_completed: i64,
  current_streak: i32,
}

fn empty_choices() -> Value {
  Value::Array(Vec::new())
}

async fn get_dashboard_handler(
  user_uuid: UserUuid,
  path: web::Path<Uuid>,
  query: web::Query<DashboardQuery>,
  state: web::Data<AppState>,
) -> ReviewResult<JsonAppResponse<DashboardResponse>> {
  let workspace_id = path.into_inner();
  let uid = workspace_uid(&state.pg_pool, &state, &user_uuid, workspace_id).await?;
  let window = review_window(query.timezone_offset_minutes)?;
  materialize_daily_queue(&state.pg_pool, workspace_id, uid, &window).await?;
  let dashboard = load_dashboard(&state.pg_pool, workspace_id, uid, &window).await?;
  Ok(AppResponse::Ok().with_data(dashboard).into())
}

async fn list_taxonomy_handler(
  user_uuid: UserUuid,
  path: web::Path<Uuid>,
  state: web::Data<AppState>,
) -> ReviewResult<JsonAppResponse<Vec<TaxonomyNode>>> {
  let workspace_id = path.into_inner();
  let _uid = workspace_uid(&state.pg_pool, &state, &user_uuid, workspace_id).await?;
  let rows = sqlx::query(
    "SELECT taxonomy_id, parent_id, kind, name, source_view_id FROM af_review_taxonomy WHERE workspace_id = $1 ORDER BY kind, name",
  )
  .bind(workspace_id)
  .fetch_all(&state.pg_pool)
  .await?;
  let nodes = rows
    .into_iter()
    .map(|row| TaxonomyNode {
      taxonomy_id: row.get("taxonomy_id"),
      parent_id: row.get("parent_id"),
      kind: row.get("kind"),
      name: row.get("name"),
      source_view_id: row.get("source_view_id"),
    })
    .collect();
  Ok(AppResponse::Ok().with_data(nodes).into())
}

async fn create_taxonomy_handler(
  user_uuid: UserUuid,
  path: web::Path<Uuid>,
  payload: web::Json<CreateTaxonomyRequest>,
  state: web::Data<AppState>,
) -> ReviewResult<JsonAppResponse<TaxonomyNode>> {
  let workspace_id = path.into_inner();
  let uid = workspace_uid(&state.pg_pool, &state, &user_uuid, workspace_id).await?;
  let request = payload.into_inner();
  validate_taxonomy(&state.pg_pool, workspace_id, &request).await?;
  let name = request.name.trim();
  let normalized = name.to_lowercase();
  if let Some(row) = sqlx::query(
    r#"SELECT taxonomy_id, parent_id, kind, name, source_view_id FROM af_review_taxonomy
       WHERE workspace_id = $1 AND parent_id IS NOT DISTINCT FROM $2 AND kind = $3 AND normalized_name = $4"#,
  )
  .bind(workspace_id)
  .bind(request.parent_id)
  .bind(&request.kind)
  .bind(&normalized)
  .fetch_optional(&state.pg_pool)
  .await?
  {
    return Ok(
      AppResponse::Ok()
        .with_data(TaxonomyNode {
          taxonomy_id: row.get("taxonomy_id"),
          parent_id: row.get("parent_id"),
          kind: row.get("kind"),
          name: row.get("name"),
          source_view_id: row.get("source_view_id"),
        })
        .into(),
    );
  }
  let row = sqlx::query(
    r#"INSERT INTO af_review_taxonomy (workspace_id, parent_id, kind, name, normalized_name, created_by)
       VALUES ($1, $2, $3, $4, $5, $6)
       RETURNING taxonomy_id, parent_id, kind, name"#,
  )
  .bind(workspace_id)
  .bind(request.parent_id)
  .bind(request.kind)
  .bind(name)
  .bind(normalized)
  .bind(uid)
  .fetch_one(&state.pg_pool)
  .await?;
  Ok(
    AppResponse::Ok()
      .with_data(TaxonomyNode {
        taxonomy_id: row.get("taxonomy_id"),
        parent_id: row.get("parent_id"),
        kind: row.get("kind"),
        name: row.get("name"),
        source_view_id: None,
      })
      .into(),
  )
}

async fn sync_page_taxonomy_handler(
  user_uuid: UserUuid,
  path: web::Path<Uuid>,
  payload: web::Json<SyncPageTaxonomyRequest>,
  state: web::Data<AppState>,
) -> ReviewResult<JsonAppResponse<()>> {
  let workspace_id = path.into_inner();
  let uid = workspace_uid(&state.pg_pool, &state, &user_uuid, workspace_id).await?;
  let request = payload.into_inner();
  let item_count = request
    .subjects
    .iter()
    .map(|subject| {
      1 + subject
        .topics
        .iter()
        .map(|topic| 1 + topic.contents.len())
        .sum::<usize>()
    })
    .sum::<usize>();
  if item_count > 2_000 {
    return Err(AppError::InvalidRequest(
      "page taxonomy is limited to 2000 items".to_string(),
    ));
  }

  let mut tx = state.pg_pool.begin().await?;
  let mut synced_source_ids = Vec::with_capacity(item_count);
  for subject in request.subjects {
    let subject_id = upsert_page_taxonomy(
      &mut tx,
      workspace_id,
      uid,
      None,
      "subject",
      subject.view_id,
      &subject.name,
    )
    .await?;
    link_legacy_cards_to_subject(
      &mut tx,
      workspace_id,
      subject_id,
      subject.view_id,
      &subject.name,
    )
    .await?;
    synced_source_ids.push(subject.view_id);

    for topic in subject.topics {
      let topic_id = upsert_page_taxonomy(
        &mut tx,
        workspace_id,
        uid,
        Some(subject_id),
        "topic",
        topic.view_id,
        &topic.name,
      )
      .await?;
      synced_source_ids.push(topic.view_id);

      for content in topic.contents {
        upsert_page_taxonomy(
          &mut tx,
          workspace_id,
          uid,
          Some(topic_id),
          "content",
          content.view_id,
          &content.name,
        )
        .await?;
        synced_source_ids.push(content.view_id);
      }
    }
  }

  if synced_source_ids.is_empty() {
    sqlx::query(
      "DELETE FROM af_review_taxonomy WHERE workspace_id = $1 AND auto_managed",
    )
    .bind(workspace_id)
    .execute(&mut *tx)
    .await?;
  } else {
    sqlx::query(
      r#"DELETE FROM af_review_taxonomy
         WHERE workspace_id = $1 AND auto_managed
           AND NOT (source_view_id = ANY($2))"#,
    )
    .bind(workspace_id)
    .bind(&synced_source_ids)
    .execute(&mut *tx)
    .await?;
  }
  tx.commit().await?;
  Ok(AppResponse::Ok().into())
}

async fn create_card_handler(
  user_uuid: UserUuid,
  path: web::Path<Uuid>,
  payload: web::Json<CreateCardRequest>,
  state: web::Data<AppState>,
) -> ReviewResult<JsonAppResponse<ReviewCard>> {
  let workspace_id = path.into_inner();
  let uid = workspace_uid(&state.pg_pool, &state, &user_uuid, workspace_id).await?;
  let request = payload.into_inner();
  validate_card(&state.pg_pool, workspace_id, &request).await?;
  let window = review_window(request.timezone_offset_minutes)?;
  let mut tx = state.pg_pool.begin().await?;
  let card_id = insert_card(&mut tx, workspace_id, uid, &request, &window).await?;
  ensure_profile(&mut tx, workspace_id, uid, window.date).await?;
  tx.commit().await?;
  let card = load_card(&state.pg_pool, workspace_id, uid, card_id).await?;
  Ok(AppResponse::Ok().with_data(card).into())
}

async fn import_cards_handler(
  user_uuid: UserUuid,
  path: web::Path<Uuid>,
  payload: web::Json<ImportCardsRequest>,
  state: web::Data<AppState>,
) -> ReviewResult<JsonAppResponse<Vec<ReviewCard>>> {
  let workspace_id = path.into_inner();
  let uid = workspace_uid(&state.pg_pool, &state, &user_uuid, workspace_id).await?;
  let request = payload.into_inner();
  if request.cards.is_empty() || request.cards.len() > 200 {
    return Err(AppError::InvalidRequest(
      "an import must contain between 1 and 200 cards".to_string(),
    ));
  }
  for card in &request.cards {
    validate_card(&state.pg_pool, workspace_id, card).await?;
  }
  let window = review_window(request.cards[0].timezone_offset_minutes)?;
  let mut tx = state.pg_pool.begin().await?;
  let mut card_ids = Vec::with_capacity(request.cards.len());
  for card in &request.cards {
    card_ids.push(insert_card(&mut tx, workspace_id, uid, card, &window).await?);
  }
  ensure_profile(&mut tx, workspace_id, uid, window.date).await?;
  tx.commit().await?;

  let mut cards = Vec::with_capacity(card_ids.len());
  for card_id in card_ids {
    cards.push(load_card(&state.pg_pool, workspace_id, uid, card_id).await?);
  }
  Ok(AppResponse::Ok().with_data(cards).into())
}

async fn insert_card(
  tx: &mut Transaction<'_, Postgres>,
  workspace_id: Uuid,
  uid: i64,
  request: &CreateCardRequest,
  window: &ReviewWindow,
) -> Result<Uuid, AppError> {
  let tags = normalize_tags(request.tags.clone());
  let row = sqlx::query(
    r#"INSERT INTO af_flashcard
       (workspace_id, created_by, source_view_id, source_block_id, card_type, front, back,
        explanation, choices, correct_answer, subject_id, topic_id, content_id, tags)
       VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14)
       RETURNING card_id"#,
  )
  .bind(workspace_id)
  .bind(uid)
  .bind(request.source_view_id)
  .bind(request.source_block_id.as_deref())
  .bind(&request.card_type)
  .bind(request.front.trim())
  .bind(request.back.trim())
  .bind(request.explanation.trim())
  .bind(&request.choices)
  .bind(request.correct_answer.trim())
  .bind(request.subject_id)
  .bind(request.topic_id)
  .bind(request.content_id)
  .bind(tags)
  .fetch_one(&mut **tx)
  .await?;
  let card_id: Uuid = row.get("card_id");
  sqlx::query(
    "INSERT INTO af_flashcard_review_state (card_id, uid, due_at, difficulty_score) VALUES ($1, $2, NOW(), $3)",
  )
  .bind(card_id)
  .bind(uid)
  .bind(f64::from(request.initial_difficulty.unwrap_or(3)))
  .execute(&mut **tx)
  .await?;
  sqlx::query(
    r#"INSERT INTO af_review_daily_queue (review_date, workspace_id, uid, card_id, source)
       VALUES ($1, $2, $3, $4, 'new') ON CONFLICT DO NOTHING"#,
  )
  .bind(window.date)
  .bind(workspace_id)
  .bind(uid)
  .bind(card_id)
  .execute(&mut **tx)
  .await?;
  Ok(card_id)
}

async fn list_cards_handler(
  user_uuid: UserUuid,
  path: web::Path<Uuid>,
  query: web::Query<CardListQuery>,
  state: web::Data<AppState>,
) -> ReviewResult<JsonAppResponse<Vec<ReviewCard>>> {
  let workspace_id = path.into_inner();
  let uid = workspace_uid(&state.pg_pool, &state, &user_uuid, workspace_id).await?;
  let window = review_window(query.timezone_offset_minutes)?;
  let rows = sqlx::query(
    r#"SELECT c.*, s.state, s.due_at, s.interval_seconds, s.difficulty_score,
              s.correct_count, s.incorrect_count
       FROM af_flashcard c
       JOIN af_flashcard_review_state s ON s.card_id = c.card_id AND s.uid = $2
       WHERE c.workspace_id = $1 AND NOT c.suspended
         AND ($3::uuid IS NULL OR c.subject_id = $3)
         AND ($4::uuid IS NULL OR c.topic_id = $4)
         AND ($5::uuid IS NULL OR c.content_id = $5)
         AND ($6::text IS NULL OR $6 = ANY(c.tags))
         AND ($7::text IS NULL OR c.card_type = $7)
         AND (NOT $8::boolean OR s.due_at < $9)
       ORDER BY s.due_at, c.created_at"#,
  )
  .bind(workspace_id)
  .bind(uid)
  .bind(query.subject_id)
  .bind(query.topic_id)
  .bind(query.content_id)
  .bind(query.tag.as_deref().map(|tag| tag.trim().to_lowercase()))
  .bind(query.card_type.as_deref())
  .bind(query.due_only)
  .bind(window.end)
  .fetch_all(&state.pg_pool)
  .await?;
  Ok(
    AppResponse::Ok()
      .with_data(rows.into_iter().map(row_to_card).collect())
      .into(),
  )
}

async fn update_card_handler(
  user_uuid: UserUuid,
  path: web::Path<(Uuid, Uuid)>,
  payload: web::Json<UpdateCardRequest>,
  state: web::Data<AppState>,
) -> ReviewResult<JsonAppResponse<ReviewCard>> {
  let (workspace_id, card_id) = path.into_inner();
  let uid = workspace_uid(&state.pg_pool, &state, &user_uuid, workspace_id).await?;
  let request = payload.into_inner();
  let tags = request.tags.map(normalize_tags);
  let result = sqlx::query(
    r#"UPDATE af_flashcard SET
       front = COALESCE($3, front), back = COALESCE($4, back),
       explanation = COALESCE($5, explanation), choices = COALESCE($6, choices),
       correct_answer = COALESCE($7, correct_answer), subject_id = COALESCE($8, subject_id),
       topic_id = COALESCE($9, topic_id), content_id = COALESCE($10, content_id),
       tags = COALESCE($11, tags), suspended = COALESCE($12, suspended), updated_at = NOW()
       WHERE workspace_id = $1 AND card_id = $2"#,
  )
  .bind(workspace_id)
  .bind(card_id)
  .bind(request.front)
  .bind(request.back)
  .bind(request.explanation)
  .bind(request.choices)
  .bind(request.correct_answer)
  .bind(request.subject_id)
  .bind(request.topic_id)
  .bind(request.content_id)
  .bind(tags)
  .bind(request.suspended)
  .execute(&state.pg_pool)
  .await?;
  if result.rows_affected() == 0 {
    return Err(AppError::RecordNotFound("flashcard".to_string()));
  }
  let card = load_card(&state.pg_pool, workspace_id, uid, card_id).await?;
  Ok(AppResponse::Ok().with_data(card).into())
}

async fn review_card_handler(
  user_uuid: UserUuid,
  path: web::Path<(Uuid, Uuid)>,
  payload: web::Json<ReviewRequest>,
  state: web::Data<AppState>,
) -> ReviewResult<JsonAppResponse<ReviewResponse>> {
  let (workspace_id, card_id) = path.into_inner();
  let uid = workspace_uid(&state.pg_pool, &state, &user_uuid, workspace_id).await?;
  let request = payload.into_inner();
  if !(1..=5).contains(&request.difficulty) {
    return Err(AppError::InvalidRequest("difficulty must be between 1 and 5".to_string()));
  }
  let window = review_window(request.timezone_offset_minutes)?;
  materialize_daily_queue(&state.pg_pool, workspace_id, uid, &window).await?;
  let response = apply_review(&state.pg_pool, workspace_id, uid, card_id, request, &window).await?;
  Ok(AppResponse::Ok().with_data(response).into())
}

struct ReviewWindow {
  date: NaiveDate,
  start: DateTime<Utc>,
  end: DateTime<Utc>,
}

fn review_window(offset_minutes: i32) -> Result<ReviewWindow, AppError> {
  if !(-14 * 60..=14 * 60).contains(&offset_minutes) {
    return Err(AppError::InvalidRequest("invalid timezone offset".to_string()));
  }
  let offset = Duration::minutes(i64::from(offset_minutes));
  let local_now = Utc::now().naive_utc() + offset;
  let date = local_now.date();
  let local_start = date
    .and_hms_opt(0, 0, 0)
    .ok_or_else(|| AppError::Internal(anyhow::anyhow!("invalid review date")))?;
  let start = Utc.from_utc_datetime(&(local_start - offset));
  let next_date = date
    .checked_add_days(Days::new(1))
    .ok_or_else(|| AppError::Internal(anyhow::anyhow!("review date overflow")))?;
  let local_end = next_date
    .and_hms_opt(0, 0, 0)
    .ok_or_else(|| AppError::Internal(anyhow::anyhow!("invalid review date")))?;
  let end = Utc.from_utc_datetime(&(local_end - offset));
  Ok(ReviewWindow { date, start, end })
}

async fn workspace_uid(
  pool: &PgPool,
  state: &AppState,
  user_uuid: &UserUuid,
  workspace_id: Uuid,
) -> Result<i64, AppError> {
  let uid = state.user_cache.get_user_uid(user_uuid).await?;
  let allowed: bool = sqlx::query_scalar(
    "SELECT EXISTS(SELECT 1 FROM af_workspace_member WHERE workspace_id = $1 AND uid = $2)",
  )
  .bind(workspace_id)
  .bind(uid)
  .fetch_one(pool)
  .await?;
  if !allowed {
    return Err(AppError::NotEnoughPermissions);
  }
  Ok(uid)
}

async fn upsert_page_taxonomy(
  tx: &mut Transaction<'_, Postgres>,
  workspace_id: Uuid,
  uid: i64,
  parent_id: Option<Uuid>,
  kind: &str,
  source_view_id: Uuid,
  name: &str,
) -> Result<Uuid, AppError> {
  let name = name.trim();
  if name.is_empty() || name.chars().count() > 200 {
    return Err(AppError::InvalidRequest(
      "page taxonomy names must contain between 1 and 200 characters".to_string(),
    ));
  }
  let normalized_name = name.to_lowercase();
  let existing_id: Option<Uuid> = sqlx::query_scalar(
    r#"SELECT taxonomy_id FROM af_review_taxonomy
       WHERE workspace_id = $1 AND (
         source_view_id = $2 OR (
           source_view_id IS NULL AND parent_id IS NOT DISTINCT FROM $3
           AND kind = $4 AND normalized_name = $5
         )
       )
       ORDER BY (source_view_id = $2) DESC
       LIMIT 1"#,
  )
  .bind(workspace_id)
  .bind(source_view_id)
  .bind(parent_id)
  .bind(kind)
  .bind(&normalized_name)
  .fetch_optional(&mut **tx)
  .await?;

  if let Some(taxonomy_id) = existing_id {
    sqlx::query(
      r#"UPDATE af_review_taxonomy SET parent_id = $3, kind = $4, name = $5,
         normalized_name = $6, source_view_id = $7, auto_managed = TRUE
         WHERE workspace_id = $1 AND taxonomy_id = $2"#,
    )
    .bind(workspace_id)
    .bind(taxonomy_id)
    .bind(parent_id)
    .bind(kind)
    .bind(name)
    .bind(normalized_name)
    .bind(source_view_id)
    .execute(&mut **tx)
    .await?;
    return Ok(taxonomy_id);
  }

  let taxonomy_id = sqlx::query_scalar(
    r#"INSERT INTO af_review_taxonomy
       (workspace_id, parent_id, kind, name, normalized_name, created_by, source_view_id, auto_managed)
       VALUES ($1, $2, $3, $4, $5, $6, $7, TRUE)
       RETURNING taxonomy_id"#,
  )
  .bind(workspace_id)
  .bind(parent_id)
  .bind(kind)
  .bind(name)
  .bind(normalized_name)
  .bind(uid)
  .bind(source_view_id)
  .fetch_one(&mut **tx)
  .await?;
  Ok(taxonomy_id)
}

async fn link_legacy_cards_to_subject(
  tx: &mut Transaction<'_, Postgres>,
  workspace_id: Uuid,
  subject_id: Uuid,
  source_view_id: Uuid,
  subject_name: &str,
) -> Result<(), AppError> {
  sqlx::query(
    r#"UPDATE af_flashcard
       SET subject_id = $2, source_view_id = $3, updated_at = now()
       WHERE workspace_id = $1
         AND source_view_id IS NULL
         AND subject_id IS NULL
         AND EXISTS (
           SELECT 1 FROM unnest(tags) AS tag
           WHERE lower(btrim(tag)) = lower(btrim($4))
         )"#,
  )
  .bind(workspace_id)
  .bind(subject_id)
  .bind(source_view_id)
  .bind(subject_name)
  .execute(&mut **tx)
  .await?;
  Ok(())
}

async fn validate_taxonomy(
  pool: &PgPool,
  workspace_id: Uuid,
  request: &CreateTaxonomyRequest,
) -> Result<(), AppError> {
  if request.name.trim().is_empty() {
    return Err(AppError::InvalidRequest("taxonomy name is required".to_string()));
  }
  let expected_parent = match request.kind.as_str() {
    "subject" => None,
    "topic" => Some("subject"),
    "content" => Some("topic"),
    _ => return Err(AppError::InvalidRequest("invalid taxonomy kind".to_string())),
  };
  match (expected_parent, request.parent_id) {
    (None, None) => Ok(()),
    (Some(kind), Some(parent_id)) => {
      let parent_kind: Option<String> = sqlx::query_scalar(
        "SELECT kind FROM af_review_taxonomy WHERE workspace_id = $1 AND taxonomy_id = $2",
      )
      .bind(workspace_id)
      .bind(parent_id)
      .fetch_optional(pool)
      .await?;
      if parent_kind.as_deref() != Some(kind) {
        return Err(AppError::InvalidRequest(format!("{} requires a {} parent", request.kind, kind)));
      }
      Ok(())
    },
    _ => Err(AppError::InvalidRequest("invalid taxonomy parent".to_string())),
  }
}

async fn validate_card(
  pool: &PgPool,
  workspace_id: Uuid,
  request: &CreateCardRequest,
) -> Result<(), AppError> {
  if request.front.trim().is_empty() {
    return Err(AppError::InvalidRequest("front is required".to_string()));
  }
  if let Some(difficulty) = request.initial_difficulty {
    if !(1..=5).contains(&difficulty) {
      return Err(AppError::InvalidRequest(
        "initial_difficulty must be between 1 and 5".to_string(),
      ));
    }
  }
  match request.card_type.as_str() {
    "classic" => {
      if request.back.trim().is_empty() {
        return Err(AppError::InvalidRequest("classic cards require a back".to_string()));
      }
    },
    "multiple_choice" => {
      let choices = request
        .choices
        .as_array()
        .ok_or_else(|| AppError::InvalidRequest("choices must be an array".to_string()))?;
      if choices.len() < 2 || request.correct_answer.trim().is_empty() {
        return Err(AppError::InvalidRequest(
          "multiple choice cards require choices and a correct answer".to_string(),
        ));
      }
    },
    "true_false" => {
      if !matches!(request.correct_answer.to_lowercase().as_str(), "true" | "false") {
        return Err(AppError::InvalidRequest(
          "true/false cards require correct_answer true or false".to_string(),
        ));
      }
    },
    _ => return Err(AppError::InvalidRequest("invalid card type".to_string())),
  }
  for (id, kind) in [
    (request.subject_id, "subject"),
    (request.topic_id, "topic"),
    (request.content_id, "content"),
  ] {
    if let Some(id) = id {
      let valid: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM af_review_taxonomy WHERE workspace_id = $1 AND taxonomy_id = $2 AND kind = $3)",
      )
      .bind(workspace_id)
      .bind(id)
      .bind(kind)
      .fetch_one(pool)
      .await?;
      if !valid {
        return Err(AppError::InvalidRequest(format!("invalid {}", kind)));
      }
    }
  }
  Ok(())
}

fn normalize_tags(tags: Vec<String>) -> Vec<String> {
  let mut tags: Vec<String> = tags
    .into_iter()
    .map(|tag| tag.trim().to_lowercase())
    .filter(|tag| !tag.is_empty())
    .collect();
  tags.sort();
  tags.dedup();
  tags
}

async fn materialize_daily_queue(
  pool: &PgPool,
  workspace_id: Uuid,
  uid: i64,
  window: &ReviewWindow,
) -> Result<(), AppError> {
  let mut tx = pool.begin().await?;
  ensure_profile(&mut tx, workspace_id, uid, window.date).await?;
  sqlx::query(
    r#"INSERT INTO af_flashcard_review_state (card_id, uid, due_at)
       SELECT card_id, $2, created_at FROM af_flashcard
       WHERE workspace_id = $1 AND NOT suspended ON CONFLICT DO NOTHING"#,
  )
  .bind(workspace_id)
  .bind(uid)
  .execute(&mut *tx)
  .await?;
  sqlx::query(
    r#"INSERT INTO af_review_daily_queue (review_date, workspace_id, uid, card_id, source)
       SELECT $1, c.workspace_id, $2, c.card_id,
              CASE WHEN s.state = 'new' THEN 'new'
                   WHEN s.due_at < $3 THEN 'overdue' ELSE 'scheduled' END
       FROM af_flashcard c
       JOIN af_flashcard_review_state s ON s.card_id = c.card_id AND s.uid = $2
       WHERE c.workspace_id = $4 AND NOT c.suspended AND s.state <> 'suspended' AND s.due_at < $5
       ON CONFLICT DO NOTHING"#,
  )
  .bind(window.date)
  .bind(uid)
  .bind(window.start)
  .bind(workspace_id)
  .bind(window.end)
  .execute(&mut *tx)
  .await?;
  tx.commit().await?;
  Ok(())
}

async fn ensure_profile(
  tx: &mut Transaction<'_, Postgres>,
  workspace_id: Uuid,
  uid: i64,
  date: NaiveDate,
) -> Result<(), AppError> {
  sqlx::query(
    "INSERT INTO af_review_profile (workspace_id, uid) VALUES ($1, $2) ON CONFLICT DO NOTHING",
  )
  .bind(workspace_id)
  .bind(uid)
  .execute(&mut **tx)
  .await?;
  sqlx::query(
    r#"INSERT INTO af_review_daily_progress (review_date, workspace_id, uid)
       VALUES ($1, $2, $3) ON CONFLICT DO NOTHING"#,
  )
  .bind(date)
  .bind(workspace_id)
  .bind(uid)
  .execute(&mut **tx)
  .await?;
  Ok(())
}

async fn load_card(pool: &PgPool, workspace_id: Uuid, uid: i64, card_id: Uuid) -> Result<ReviewCard, AppError> {
  let row = sqlx::query(
    r#"SELECT c.*, s.state, s.due_at, s.interval_seconds, s.difficulty_score,
              s.correct_count, s.incorrect_count
       FROM af_flashcard c
       JOIN af_flashcard_review_state s ON s.card_id = c.card_id AND s.uid = $2
       WHERE c.workspace_id = $1 AND c.card_id = $3"#,
  )
  .bind(workspace_id)
  .bind(uid)
  .bind(card_id)
  .fetch_one(pool)
  .await?;
  Ok(row_to_card(row))
}

fn row_to_card(row: sqlx::postgres::PgRow) -> ReviewCard {
  ReviewCard {
    card_id: row.get("card_id"),
    card_type: row.get("card_type"),
    front: row.get("front"),
    back: row.get("back"),
    explanation: row.get("explanation"),
    choices: row.get("choices"),
    correct_answer: row.get("correct_answer"),
    subject_id: row.get("subject_id"),
    topic_id: row.get("topic_id"),
    content_id: row.get("content_id"),
    tags: row.get("tags"),
    source_view_id: row.get("source_view_id"),
    source_block_id: row.get("source_block_id"),
    state: row.get("state"),
    due_at: row.get("due_at"),
    interval_seconds: row.get("interval_seconds"),
    difficulty_score: row.get("difficulty_score"),
    correct_count: row.get("correct_count"),
    incorrect_count: row.get("incorrect_count"),
  }
}

async fn load_dashboard(
  pool: &PgPool,
  workspace_id: Uuid,
  uid: i64,
  window: &ReviewWindow,
) -> Result<DashboardResponse, AppError> {
  let summary = sqlx::query(
    r#"SELECT COUNT(*)::bigint AS goal,
              COUNT(first_reviewed_at)::bigint AS completed,
              COUNT(*) FILTER (WHERE source = 'overdue')::bigint AS overdue,
              COUNT(*) FILTER (WHERE source = 'new')::bigint AS new_cards
       FROM af_review_daily_queue q JOIN af_flashcard c ON c.card_id = q.card_id
       WHERE q.review_date = $1 AND q.workspace_id = $2 AND q.uid = $3 AND NOT c.suspended"#,
  )
  .bind(window.date)
  .bind(workspace_id)
  .bind(uid)
  .fetch_one(pool)
  .await?;
  let progress = sqlx::query(
    r#"SELECT p.xp_earned, p.reviews_completed, p.completion_awarded,
              r.total_xp, r.current_streak, r.longest_streak
       FROM af_review_daily_progress p
       JOIN af_review_profile r ON r.workspace_id = p.workspace_id AND r.uid = p.uid
       WHERE p.review_date = $1 AND p.workspace_id = $2 AND p.uid = $3"#,
  )
  .bind(window.date)
  .bind(workspace_id)
  .bind(uid)
  .fetch_one(pool)
  .await?;
  let learning: i64 = sqlx::query_scalar(
    r#"SELECT COUNT(*) FROM af_flashcard c JOIN af_flashcard_review_state s ON s.card_id = c.card_id
       WHERE c.workspace_id = $1 AND s.uid = $2 AND s.state IN ('learning','relearning')"#,
  )
  .bind(workspace_id)
  .bind(uid)
  .fetch_one(pool)
  .await?;
  let queue_rows = sqlx::query(
    r#"SELECT c.*, s.state, s.due_at, s.interval_seconds, s.difficulty_score,
              s.correct_count, s.incorrect_count
       FROM af_review_daily_queue q
       JOIN af_flashcard c ON c.card_id = q.card_id
       JOIN af_flashcard_review_state s ON s.card_id = c.card_id AND s.uid = q.uid
       WHERE q.review_date = $1 AND q.workspace_id = $2 AND q.uid = $3
         AND q.first_reviewed_at IS NULL AND NOT c.suspended
       ORDER BY CASE q.source WHEN 'overdue' THEN 0 WHEN 'scheduled' THEN 1 ELSE 2 END, s.due_at
       LIMIT 100"#,
  )
  .bind(window.date)
  .bind(workspace_id)
  .bind(uid)
  .fetch_all(pool)
  .await?;
  let goal: i64 = summary.get("goal");
  let completed: i64 = summary.get("completed");
  let total_xp: i64 = progress.get("total_xp");
  Ok(DashboardResponse {
    review_date: window.date,
    goal,
    completed,
    remaining: goal.saturating_sub(completed),
    overdue: summary.get("overdue"),
    new_cards: summary.get("new_cards"),
    learning,
    reviewed_today: progress.get::<i32, _>("reviews_completed") as i64,
    xp_today: progress.get::<i32, _>("xp_earned") as i64,
    total_xp,
    level: 1 + (total_xp as f64 / 100.0).sqrt().floor() as i64,
    current_streak: progress.get("current_streak"),
    longest_streak: progress.get("longest_streak"),
    completion_awarded: progress.get("completion_awarded"),
    queue: queue_rows.into_iter().map(row_to_card).collect(),
  })
}

async fn apply_review(
  pool: &PgPool,
  workspace_id: Uuid,
  uid: i64,
  card_id: Uuid,
  request: ReviewRequest,
  window: &ReviewWindow,
) -> Result<ReviewResponse, AppError> {
  let mut tx = pool.begin().await?;
  ensure_profile(&mut tx, workspace_id, uid, window.date).await?;
  let row = sqlx::query(
    r#"SELECT c.card_type, c.correct_answer, s.state, s.due_at, s.interval_seconds,
              s.difficulty_score, s.consecutive_correct, s.consecutive_incorrect
       FROM af_flashcard c JOIN af_flashcard_review_state s ON s.card_id = c.card_id AND s.uid = $2
       WHERE c.workspace_id = $1 AND c.card_id = $3 AND NOT c.suspended FOR UPDATE"#,
  )
  .bind(workspace_id)
  .bind(uid)
  .bind(card_id)
  .fetch_one(&mut *tx)
  .await?;
  let card_type: String = row.get("card_type");
  let correct_answer: String = row.get("correct_answer");
  let correct = determine_correct(&card_type, &correct_answer, &request)?;
  let interval_before: i64 = row.get("interval_seconds");
  let due_before: DateTime<Utc> = row.get("due_at");
  let (interval_after, next_state) = calculate_interval(
    interval_before,
    correct,
    request.difficulty,
    row.get("consecutive_correct"),
    row.get("consecutive_incorrect"),
  );
  let due_after = Utc::now() + Duration::seconds(interval_after);
  let old_score: f64 = row.get("difficulty_score");
  let new_score = (old_score * 0.8) + (f64::from(request.difficulty) * 0.2);
  sqlx::query(
    r#"UPDATE af_flashcard_review_state SET state = $3, due_at = $4, interval_seconds = $5,
       difficulty_score = $6, correct_count = correct_count + $7,
       incorrect_count = incorrect_count + $8, lapse_count = lapse_count + $8,
       consecutive_correct = CASE WHEN $7 = 1 THEN consecutive_correct + 1 ELSE 0 END,
       consecutive_incorrect = CASE WHEN $8 = 1 THEN consecutive_incorrect + 1 ELSE 0 END,
       last_reviewed_at = NOW(), updated_at = NOW() WHERE card_id = $1 AND uid = $2"#,
  )
  .bind(card_id)
  .bind(uid)
  .bind(&next_state)
  .bind(due_after)
  .bind(interval_after)
  .bind(new_score)
  .bind(if correct { 1_i32 } else { 0_i32 })
  .bind(if correct { 0_i32 } else { 1_i32 })
  .execute(&mut *tx)
  .await?;
  let queue_row = sqlx::query(
    r#"SELECT first_reviewed_at IS NOT NULL AS already_reviewed
       FROM af_review_daily_queue WHERE review_date = $1 AND workspace_id = $2 AND uid = $3 AND card_id = $4"#,
  )
  .bind(window.date)
  .bind(workspace_id)
  .bind(uid)
  .bind(card_id)
  .fetch_optional(&mut *tx)
  .await?;
  let scheduled = queue_row.is_some();
  let already_reviewed = queue_row
    .as_ref()
    .map(|row| row.get::<bool, _>("already_reviewed"))
    .unwrap_or(false);
  if scheduled && !already_reviewed {
    sqlx::query(
      r#"UPDATE af_review_daily_queue SET first_reviewed_at = NOW()
         WHERE review_date = $1 AND workspace_id = $2 AND uid = $3 AND card_id = $4"#,
    )
    .bind(window.date)
    .bind(workspace_id)
    .bind(uid)
    .bind(card_id)
    .execute(&mut *tx)
    .await?;
  }
  let extra_reviews_for_card_today: i64 = sqlx::query_scalar(
    r#"SELECT COUNT(*) FROM af_flashcard_review_log
       WHERE workspace_id = $1 AND uid = $2 AND card_id = $3 AND review_date = $4
         AND NOT scheduled"#,
  )
  .bind(workspace_id)
  .bind(uid)
  .bind(card_id)
  .bind(window.date)
  .fetch_one(&mut *tx)
  .await?;
  let scheduled_review = scheduled && !already_reviewed;
  let xp = calculate_review_xp(scheduled_review, extra_reviews_for_card_today);
  sqlx::query(
    r#"INSERT INTO af_flashcard_review_log
       (workspace_id, uid, card_id, review_date, selected_answer, correct, difficulty, scheduled,
        interval_before_seconds, interval_after_seconds, due_before, due_after, response_time_ms, xp_awarded)
       VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14)"#,
  )
  .bind(workspace_id)
  .bind(uid)
  .bind(card_id)
  .bind(window.date)
  .bind(request.selected_answer)
  .bind(correct)
  .bind(request.difficulty)
  .bind(scheduled_review)
  .bind(interval_before)
  .bind(interval_after)
  .bind(due_before)
  .bind(due_after)
  .bind(request.response_time_ms)
  .bind(xp)
  .execute(&mut *tx)
  .await?;
  sqlx::query(
    r#"UPDATE af_review_daily_progress SET xp_earned = xp_earned + $4,
       reviews_completed = reviews_completed + 1, updated_at = NOW()
       WHERE review_date = $1 AND workspace_id = $2 AND uid = $3"#,
  )
  .bind(window.date)
  .bind(workspace_id)
  .bind(uid)
  .bind(xp)
  .execute(&mut *tx)
  .await?;
  sqlx::query(
    "UPDATE af_review_profile SET total_xp = total_xp + $3, updated_at = NOW() WHERE workspace_id = $1 AND uid = $2",
  )
  .bind(workspace_id)
  .bind(uid)
  .bind(xp)
  .execute(&mut *tx)
  .await?;
  let goal: i64 = sqlx::query_scalar(
    r#"SELECT COUNT(*) FROM af_review_daily_queue q JOIN af_flashcard c ON c.card_id = q.card_id
       WHERE q.review_date = $1 AND q.workspace_id = $2 AND q.uid = $3 AND NOT c.suspended"#,
  )
  .bind(window.date)
  .bind(workspace_id)
  .bind(uid)
  .fetch_one(&mut *tx)
  .await?;
  let completed: i64 = sqlx::query_scalar(
    r#"SELECT COUNT(*) FROM af_review_daily_queue q JOIN af_flashcard c ON c.card_id = q.card_id
       WHERE q.review_date = $1 AND q.workspace_id = $2 AND q.uid = $3
         AND q.first_reviewed_at IS NOT NULL AND NOT c.suspended"#,
  )
  .bind(window.date)
  .bind(workspace_id)
  .bind(uid)
  .fetch_one(&mut *tx)
  .await?;
  let completion_bonus = maybe_award_completion(&mut tx, workspace_id, uid, window.date, goal, completed).await?;
  let profile = sqlx::query(
    "SELECT total_xp, current_streak FROM af_review_profile WHERE workspace_id = $1 AND uid = $2",
  )
  .bind(workspace_id)
  .bind(uid)
  .fetch_one(&mut *tx)
  .await?;
  tx.commit().await?;
  Ok(ReviewResponse {
    correct,
    xp_awarded: xp,
    completion_bonus,
    total_xp: profile.get("total_xp"),
    due_at: due_after,
    interval_seconds: interval_after,
    state: next_state,
    daily_goal: goal,
    daily_completed: completed,
    current_streak: profile.get("current_streak"),
  })
}

fn determine_correct(card_type: &str, answer: &str, request: &ReviewRequest) -> Result<bool, AppError> {
  if card_type == "classic" {
    return request
      .correct
      .ok_or_else(|| AppError::InvalidRequest("classic cards require correct".to_string()));
  }
  let selected = request
    .selected_answer
    .as_deref()
    .ok_or_else(|| AppError::InvalidRequest("selected_answer is required".to_string()))?;
  Ok(selected.trim().eq_ignore_ascii_case(answer.trim()))
}

fn calculate_interval(
  current: i64,
  correct: bool,
  difficulty: i16,
  consecutive_correct: i32,
  consecutive_incorrect: i32,
) -> (i64, String) {
  const DAY: i64 = 86_400;
  if current == 0 {
    let seconds = if correct {
      [7 * DAY, 5 * DAY, 3 * DAY, 2 * DAY, DAY][(difficulty - 1) as usize]
    } else {
      [DAY, 12 * 3_600, 6 * 3_600, 2 * 3_600, 10 * 60][(difficulty - 1) as usize]
    };
    return (seconds, if correct { "review" } else { "learning" }.to_string());
  }
  if correct {
    let multipliers = [3.2, 2.6, 2.0, 1.5, 1.2];
    let history_bonus = 1.0 + (f64::from(consecutive_correct.min(5)) * 0.03);
    let seconds = ((current.max(DAY) as f64) * multipliers[(difficulty - 1) as usize] * history_bonus)
      .round() as i64;
    (seconds.max(DAY), "review".to_string())
  } else {
    let factors = [0.5, 0.4, 0.3, 0.2, 0.1];
    let caps = [5 * DAY, 4 * DAY, 3 * DAY, 2 * DAY, DAY];
    let minimums = [DAY, 12 * 3_600, 6 * 3_600, 2 * 3_600, 10 * 60];
    let index = (difficulty - 1) as usize;
    let lapse_penalty = 1.0 - (f64::from(consecutive_incorrect.min(3)) * 0.1);
    let reduced = ((current as f64) * factors[index] * lapse_penalty).round() as i64;
    (
      reduced.clamp(minimums[index], caps[index]),
      "relearning".to_string(),
    )
  }
}

fn calculate_review_xp(scheduled_review: bool, extra_reviews_for_card_today: i64) -> i32 {
  if scheduled_review {
    XP_SCHEDULED
  } else if extra_reviews_for_card_today == 0 {
    XP_EXTRA
  } else if extra_reviews_for_card_today < 3 {
    XP_RELEARNING
  } else {
    0
  }
}

async fn maybe_award_completion(
  tx: &mut Transaction<'_, Postgres>,
  workspace_id: Uuid,
  uid: i64,
  date: NaiveDate,
  goal: i64,
  completed: i64,
) -> Result<i32, AppError> {
  if goal == 0 || completed < goal {
    return Ok(0);
  }
  let awarded: bool = sqlx::query_scalar(
    r#"UPDATE af_review_daily_progress SET completion_awarded = TRUE,
       completion_awarded_at = NOW(), xp_earned = xp_earned + $4, updated_at = NOW()
       WHERE review_date = $1 AND workspace_id = $2 AND uid = $3 AND NOT completion_awarded
       RETURNING completion_awarded"#,
  )
  .bind(date)
  .bind(workspace_id)
  .bind(uid)
  .bind(XP_COMPLETION)
  .fetch_optional(&mut **tx)
  .await?
  .unwrap_or(false);
  if !awarded {
    return Ok(0);
  }
  sqlx::query(
    r#"UPDATE af_review_profile SET total_xp = total_xp + $3,
       current_streak = CASE
         WHEN last_completed_date = $4::date - 1 THEN current_streak + 1
         WHEN last_completed_date = $4 THEN current_streak
         ELSE 1 END,
       longest_streak = GREATEST(longest_streak, CASE
         WHEN last_completed_date = $4::date - 1 THEN current_streak + 1
         WHEN last_completed_date = $4 THEN current_streak
         ELSE 1 END),
       last_completed_date = $4, updated_at = NOW()
       WHERE workspace_id = $1 AND uid = $2"#,
  )
  .bind(workspace_id)
  .bind(uid)
  .bind(XP_COMPLETION)
  .bind(date)
  .execute(&mut **tx)
  .await?;
  Ok(XP_COMPLETION)
}

#[cfg(test)]
mod tests {
  use super::{calculate_interval, calculate_review_xp};

  const DAY: i64 = 86_400;

  #[test]
  fn correct_result_expands_interval_by_difficulty() {
    let very_easy = calculate_interval(10 * DAY, true, 1, 0, 0).0;
    let very_hard = calculate_interval(10 * DAY, true, 5, 0, 0).0;
    assert_eq!(very_easy, 32 * DAY);
    assert_eq!(very_hard, 12 * DAY);
  }

  #[test]
  fn incorrect_result_dominates_even_when_marked_easy() {
    let very_easy = calculate_interval(10 * DAY, false, 1, 0, 0).0;
    let very_hard = calculate_interval(10 * DAY, false, 5, 0, 0).0;
    assert_eq!(very_easy, 5 * DAY);
    assert_eq!(very_hard, DAY);
  }

  #[test]
  fn new_cards_use_initial_learning_steps() {
    assert_eq!(calculate_interval(0, true, 3, 0, 0).0, 3 * DAY);
    assert_eq!(calculate_interval(0, false, 5, 0, 0).0, 10 * 60);
  }

  #[test]
  fn xp_rewards_habit_without_allowing_unlimited_repetition() {
    assert_eq!(calculate_review_xp(true, 0), 5);
    assert_eq!(calculate_review_xp(false, 0), 2);
    assert_eq!(calculate_review_xp(false, 1), 1);
    assert_eq!(calculate_review_xp(false, 2), 1);
    assert_eq!(calculate_review_xp(false, 3), 0);
  }
}
