use actix_web::{web, HttpRequest, Scope};
use crate::api::util::realtime_user_for_web_request;
use crate::biz::workspace::page_view::create_page;
use shared_entity::dto::workspace_dto::ViewLayout;
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
      web::resource("/{workspace_id}/lesson-import")
        .app_data(web::PayloadConfig::new(5 * 1024 * 1024))
        .route(web::post().to(import_lesson_handler)),
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

/// A complete lesson package emitted by the StudyFlash Gran extension.
/// The parent view is deliberately explicit: a caller can import straight into
/// any subject or topic page without relying on a fragile title match.
#[derive(Debug, Deserialize)]
struct LessonImportRequest {
  parent_view_id: Uuid,
  title: String,
  source_url: String,
  #[serde(default)]
  discipline: String,
  #[serde(default)]
  topic: String,
  #[serde(default)]
  transcript: String,
  #[serde(default)]
  summary: String,
  #[serde(default)]
  pocket_review: String,
  #[serde(default)]
  questions: Vec<Value>,
  #[serde(default)]
  mind_maps: Vec<String>,
  #[serde(default)]
  skipped_materials: Vec<String>,
  #[serde(default)]
  cards: Vec<CreateCardRequest>,
  subject_id: Option<Uuid>,
  topic_id: Option<Uuid>,
  content_id: Option<Uuid>,
  #[serde(default)]
  timezone_offset_minutes: i32,
}

#[derive(Debug, Serialize)]
struct LessonImportResponse {
  view_id: Uuid,
  imported_cards: usize,
  material_pages: usize,
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

async fn import_lesson_handler(
  user_uuid: UserUuid,
  path: web::Path<Uuid>,
  payload: web::Json<LessonImportRequest>,
  state: web::Data<AppState>,
  req: HttpRequest,
) -> ReviewResult<JsonAppResponse<LessonImportResponse>> {
  let workspace_id = path.into_inner();
  let uid = workspace_uid(&state.pg_pool, &state, &user_uuid, workspace_id).await?;
  let mut request = payload.into_inner();
  let title = request.title.trim().to_owned();
  if title.is_empty() || title.len() > 500 {
    return Err(AppError::InvalidRequest("lesson title must contain 1 to 500 characters".to_string()));
  }
  if request.source_url.trim().is_empty() || request.source_url.len() > 4_000 {
    return Err(AppError::InvalidRequest("a valid lesson source URL is required".to_string()));
  }
  if request.cards.len() > 200 {
    return Err(AppError::InvalidRequest("a lesson import is limited to 200 flashcards".to_string()));
  }

  let resolved_taxonomy = resolve_source_taxonomy(&state.pg_pool, workspace_id, request.parent_view_id).await?;
  let subject_id = request.subject_id.or(resolved_taxonomy.subject_id);
  let mut topic_id = request.topic_id.or(resolved_taxonomy.topic_id);
  let mut content_id = request.content_id.or(resolved_taxonomy.content_id);

  // Validate the entire package before creating its document. This prevents an
  // invalid card from leaving an otherwise empty imported lesson behind.
  for card in &mut request.cards {
    card.subject_id = subject_id;
    card.topic_id = topic_id;
    card.content_id = content_id;
    card.timezone_offset_minutes = request.timezone_offset_minutes;
    validate_card(&state.pg_pool, workspace_id, card).await?;
  }

  let user = realtime_user_for_web_request(req.headers(), uid)?;
  let page_data = lesson_page_data(&request);
  let page = create_page(
    &state,
    user,
    workspace_id,
    &request.parent_view_id,
    &ViewLayout::Document,
    Some(&title),
    Some(&page_data),
    None,
    None,
  )
  .await?;

  // Textual learning materials are real child pages of the lesson.  This
  // mirrors the normal StudyFlash/AppFlowy hierarchy: Topic -> Lesson ->
  // Summary, Transcript, and so on. Questions and flashcards deliberately do
  // not become copied text here; they are represented by their native Review
  // card formats below.
  let mut material_pages = 0;
  for (name, data) in lesson_material_pages(&request) {
    create_page(
      &state,
      user.clone(),
      workspace_id,
      &page.view_id,
      &ViewLayout::Document,
      Some(name),
      Some(&data),
      None,
      None,
    )
    .await?;
    material_pages += 1;
  }

  // The document now exists. Tie every imported card to it so Review can open
  // the lesson again through the existing "Rever conteúdo" flow.
  for card in &mut request.cards {
    card.source_view_id = Some(page.view_id);
    card.source_block_id = None;
  }

  // An imported lesson is itself a topic when placed in a subject and content
  // when placed in a topic. This keeps the page tree and Review taxonomy in
  // lockstep without requiring a separate classification action afterwards.
  let needs_taxonomy = (subject_id.is_some() && topic_id.is_none())
    || (topic_id.is_some() && content_id.is_none());
  if !request.cards.is_empty() || needs_taxonomy {
    let mut tx = state.pg_pool.begin().await?;
    let taxonomy_name: String = title.chars().take(200).collect();
    if topic_id.is_some() && content_id.is_none() {
      content_id = Some(
        upsert_page_taxonomy(
          &mut tx,
          workspace_id,
          uid,
          topic_id,
          "content",
          page.view_id,
          &taxonomy_name,
        )
        .await?,
      );
    } else if subject_id.is_some() && topic_id.is_none() {
      topic_id = Some(
        upsert_page_taxonomy(
          &mut tx,
          workspace_id,
          uid,
          subject_id,
          "topic",
          page.view_id,
          &taxonomy_name,
        )
        .await?,
      );
    }
    for card in &request.cards {
      let mut card = card.clone();
      card.subject_id = subject_id;
      card.topic_id = topic_id;
      card.content_id = content_id;
      let window = review_window(card.timezone_offset_minutes)?;
      insert_card(&mut tx, workspace_id, uid, &card, &window).await?;
    }
    if !request.cards.is_empty() {
      let window = review_window(request.timezone_offset_minutes)?;
      ensure_profile(&mut tx, workspace_id, uid, window.date).await?;
    }
    tx.commit().await?;
  }
  Ok(
    AppResponse::Ok()
      .with_data(LessonImportResponse {
        view_id: page.view_id,
        imported_cards: request.cards.len(),
        material_pages,
      })
      .into(),
  )
}

#[derive(Default)]
struct SourceTaxonomy {
  subject_id: Option<Uuid>,
  topic_id: Option<Uuid>,
  content_id: Option<Uuid>,
}

/// Maps the destination page selected in the extension back to the StudyFlash
/// taxonomy.  A lesson imported into a subject, topic, or content page therefore
/// becomes available in exactly the same area of Review without asking the user
/// to classify it a second time.
async fn resolve_source_taxonomy(
  pool: &PgPool,
  workspace_id: Uuid,
  source_view_id: Uuid,
) -> Result<SourceTaxonomy, AppError> {
  let row = sqlx::query(
    "SELECT taxonomy_id, parent_id, kind FROM af_review_taxonomy WHERE workspace_id = $1 AND source_view_id = $2",
  )
  .bind(workspace_id)
  .bind(source_view_id)
  .fetch_optional(pool)
  .await?;
  let Some(row) = row else { return Ok(SourceTaxonomy::default()); };

  let taxonomy_id: Uuid = row.get("taxonomy_id");
  let parent_id: Option<Uuid> = row.get("parent_id");
  let kind: String = row.get("kind");
  match kind.as_str() {
    "subject" => Ok(SourceTaxonomy { subject_id: Some(taxonomy_id), ..Default::default() }),
    "topic" => Ok(SourceTaxonomy {
      subject_id: parent_id,
      topic_id: Some(taxonomy_id),
      ..Default::default()
    }),
    "content" => {
      let topic_id = parent_id;
      let subject_id = if let Some(topic_id) = topic_id {
        sqlx::query_scalar(
          "SELECT parent_id FROM af_review_taxonomy WHERE workspace_id = $1 AND taxonomy_id = $2 AND kind = 'topic'",
        )
        .bind(workspace_id)
        .bind(topic_id)
        .fetch_optional(pool)
        .await?
        .flatten()
      } else {
        None
      };
      Ok(SourceTaxonomy { subject_id, topic_id, content_id: Some(taxonomy_id) })
    },
    _ => Ok(SourceTaxonomy::default()),
  }
}

fn lesson_page_data(request: &LessonImportRequest) -> Value {
  let mut children = Vec::new();
  push_document_text(&mut children, &format!("Aula importada do Gran\nOrigem: {}", request.source_url));
  if !request.discipline.trim().is_empty() {
    push_document_text(&mut children, &format!("Matéria: {}", request.discipline.trim()));
  }
  if !request.topic.trim().is_empty() {
    push_document_text(&mut children, &format!("Tópico: {}", request.topic.trim()));
  }
  push_document_text(&mut children, "Os resumos, a transcrição e os mapas mentais estão organizados em páginas filhas desta aula.");
  if !request.cards.is_empty() {
    push_document_text(&mut children, &format!("{} cartão(ões), incluindo questões quando disponíveis, foram criados no formato nativo da Revisão e vinculados a esta aula.", request.cards.len()));
  }
  if !request.skipped_materials.is_empty() {
    push_document_heading(&mut children, "MATERIAIS NÃO DISPONÍVEIS NESTA IMPORTAÇÃO");
    push_document_text(&mut children, &request.skipped_materials.join("\n"));
  }

  serde_json::json!({ "type": "page", "children": children })
}

/// Builds the documents nested directly below the imported lesson. Empty
/// materials do not produce blank pages.
fn lesson_material_pages(request: &LessonImportRequest) -> Vec<(&'static str, Value)> {
  let mut pages = Vec::new();
  if !request.summary.trim().is_empty() {
    pages.push(("Resumo", document_text_data(&request.summary)));
  }
  if !request.pocket_review.trim().is_empty() {
    pages.push(("Revisão de bolso", document_text_data(&request.pocket_review)));
  }
  if !request.transcript.trim().is_empty() {
    pages.push(("Transcrição", document_text_data(&request.transcript)));
  }
  if !request.mind_maps.is_empty() {
    let mut children = Vec::new();
    for map in &request.mind_maps {
      children.push(serde_json::json!({
        "type": "image",
        "data": { "url": map, "align": "center", "image_type": 1 }
      }));
    }
    pages.push(("Mapas mentais", serde_json::json!({ "type": "page", "children": children })));
  }
  pages
}

fn document_text_data(text: &str) -> Value {
  let mut children = Vec::new();
  push_document_text(&mut children, text);
  serde_json::json!({ "type": "page", "children": children })
}

fn push_document_heading(children: &mut Vec<Value>, text: &str) {
  children.push(serde_json::json!({
    "type": "heading",
    "data": { "level": 2, "delta": [{ "insert": text }] }
  }));
}

fn push_document_text(children: &mut Vec<Value>, text: &str) {
  for line in text.lines().map(str::trim).filter(|line| !line.is_empty()) {
    let (block_type, line) = if let Some(item) = line.strip_prefix("- ").or_else(|| line.strip_prefix("• ")) {
      ("bulleted_list", item)
    } else if let Some(item) = numbered_list_item(line) {
      ("numbered_list", item)
    } else {
      ("paragraph", line)
    };
    children.push(serde_json::json!({
      "type": block_type,
      "data": { "delta": [{ "insert": line }] }
    }));
  }
}

fn numbered_list_item(line: &str) -> Option<&str> {
  let number_end = line.find(|character: char| !character.is_ascii_digit())?;
  let separator = *line.as_bytes().get(number_end)?;
  if number_end == 0 || !matches!(separator, b'.' | b')') {
    return None;
  }
  line.get(number_end + 1..).map(str::trim).filter(|item| !item.is_empty())
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
  use super::{
    calculate_interval, calculate_review_xp, lesson_material_pages, lesson_page_data,
    numbered_list_item, push_document_text, CreateCardRequest, LessonImportRequest,
  };
  use serde_json::json;
  use uuid::Uuid;

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

  #[test]
  fn imported_text_preserves_simple_lists_as_editor_blocks() {
    let mut children = Vec::new();
    push_document_text(&mut children, "Introdução\n- ponto importante\n2. segunda etapa");
    assert_eq!(children[0]["type"], "paragraph");
    assert_eq!(children[1]["type"], "bulleted_list");
    assert_eq!(children[2]["type"], "numbered_list");
    assert_eq!(numbered_list_item("3) exemplo"), Some("exemplo"));
    assert_eq!(numbered_list_item("texto comum"), None);
  }

  #[test]
  fn full_lesson_package_uses_child_pages_and_native_review_cards() {
    let request = LessonImportRequest {
      parent_view_id: Uuid::new_v4(),
      title: "Morfologia III".to_string(),
      source_url: "https://www.grancursosonline.com.br/aluno/curso/video/example".to_string(),
      discipline: "Língua Portuguesa".to_string(),
      topic: "Morfologia".to_string(),
      transcript: "Texto da transcrição".to_string(),
      summary: "Resumo da aula".to_string(),
      pocket_review: "Revisão rápida".to_string(),
      questions: vec![json!({
        "statement": "Qual é a resposta?",
        "correct": "A",
        "alternatives": [{ "letter": "A", "text": "Certa", "correct": true, "explanation": "Porque está certa." }]
      })],
      mind_maps: vec!["https://cdn.example.com/mapa.png".to_string()],
      skipped_materials: vec!["Resumo de bolso: não disponível nesta aula".to_string()],
      cards: vec![CreateCardRequest {
        card_type: "classic".to_string(),
        front: "Frente".to_string(),
        back: "Verso".to_string(),
        explanation: String::new(),
        choices: json!([]),
        correct_answer: String::new(),
        subject_id: None,
        topic_id: None,
        content_id: None,
        tags: Vec::new(),
        source_view_id: None,
        source_block_id: None,
        timezone_offset_minutes: 0,
        initial_difficulty: None,
      }],
      subject_id: None,
      topic_id: None,
      content_id: None,
      timezone_offset_minutes: 0,
    };

    let materials = lesson_material_pages(&request);
    assert_eq!(materials.len(), 4);
    assert_eq!(materials[0].0, "Resumo");
    assert_eq!(materials[1].0, "Revisão de bolso");
    assert_eq!(materials[2].0, "Transcrição");
    assert_eq!(materials[3].0, "Mapas mentais");
    assert_eq!(materials[3].1["children"][0]["type"], "image");

    let lesson = lesson_page_data(&request);
    let lesson_text = lesson["children"].to_string();
    assert!(lesson_text.contains("páginas filhas"));
    assert!(lesson_text.contains("formato nativo da Revisão"));
    assert!(!lesson_text.contains("Questão 1"));
    assert!(!lesson_text.contains("Frente: Frente"));
  }
}
