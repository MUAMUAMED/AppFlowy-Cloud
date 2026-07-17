use actix_web::web::Data;
use actix_web::{web, Scope};
use shared_entity::dto::server_info_dto::ServerInfoResponseItem;
use shared_entity::response::{AppResponse, JsonAppResponse};

use crate::state::AppState;

pub fn server_info_scope() -> Scope {
  web::scope("/api")
    .service(web::resource("/server").route(web::get().to(server_info_handler)))
    // Recent native clients probe this compatibility endpoint before opening
    // their realtime connection.
    .service(web::resource("/server-info").route(web::get().to(server_info_handler)))
}

async fn server_info_handler(
  state: Data<AppState>,
) -> actix_web::Result<JsonAppResponse<ServerInfoResponseItem>> {
  Ok(
    AppResponse::Ok()
      .with_data(ServerInfoResponseItem {
        supported_client_features: vec![],
        minimum_supported_client_version: None,
        appflowy_web_url: state.config.appflowy_web_url.clone(),
      })
      .into(),
  )
}
