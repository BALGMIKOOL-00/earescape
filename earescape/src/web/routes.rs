use crate::web::handlers::{
    catalog_page, chat_page, collect_logs_partial, collect_run_sse, compare_page, compare_summary,
    glossary_partial, home_page, products_partial, quiz_recommend, rag_ask, AppState,
};
use axum::{
    routing::{get, post},
    Router,
};
use tower_http::services::ServeDir;
use tower_http::trace::TraceLayer;

pub fn create_router(state: AppState) -> Router {
    Router::new()
        .route("/", get(home_page))
        .route("/chat", get(chat_page))
        .route("/compare", get(compare_page))
        .route("/catalog", get(catalog_page))
        .route("/api/quiz/recommend", post(quiz_recommend))
        .route("/api/rag/ask", post(rag_ask))
        .route("/api/compare/summary", post(compare_summary))
        .route("/api/products", get(products_partial))
        .route("/api/collect/run", post(collect_run_sse))
        .route("/api/collect/logs", get(collect_logs_partial))
        .route("/api/glossary", get(glossary_partial))
        .nest_service("/static", ServeDir::new("static"))
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}
