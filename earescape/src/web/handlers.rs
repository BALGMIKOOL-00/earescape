use crate::collector::CollectorService;
use crate::db::repository::{
    get_products_by_ids, list_glossary_terms, list_products, CollectionLogEntry,
};
use crate::domain::product::{GlossaryTerm, Product, ProductFilter, TOP10_BRANDS};
use crate::domain::quiz::{evaluate_quiz, QuizInput, QuizRecommendationResult};
use crate::rag::pipeline::{RagAnswer, RagPipeline};
use askama_axum::Template;
use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::{
        sse::{Event, KeepAlive, Sse},
        IntoResponse, Response,
    },
    Form,
};
use chrono::NaiveDate;
use futures::stream::Stream;
use rusqlite::Connection;
use serde::Deserialize;
use std::convert::Infallible;
use std::sync::{Arc, Mutex};
use tokio_stream::wrappers::ReceiverStream;
use tokio_stream::StreamExt;

#[derive(Clone)]
pub struct AppState {
    pub db: Arc<Mutex<Connection>>,
    pub collector: Arc<CollectorService>,
    pub rag: Arc<RagPipeline>,
    pub reference_date: NaiveDate,
}

pub struct AppError(anyhow::Error);

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("서버 처리 중 오류가 발생했습니다: {}", self.0),
        )
            .into_response()
    }
}

impl<E: Into<anyhow::Error>> From<E> for AppError {
    fn from(err: E) -> Self {
        Self(err.into())
    }
}

fn latest_collected_label(collector: &CollectorService) -> String {
    collector
        .recent_logs(1)
        .ok()
        .and_then(|mut v| v.pop())
        .map(|l| l.triggered_at)
        .unwrap_or_else(|| "초기 시드 데이터".to_string())
}

#[derive(Template)]
#[template(path = "index.html")]
pub struct IndexTemplate {
    pub active_nav: String,
    pub total_products: usize,
    pub last_collected: String,
    pub llm_mode: String,
}

#[derive(Template)]
#[template(path = "chat.html")]
pub struct ChatTemplate {
    pub active_nav: String,
    pub last_collected: String,
    pub llm_mode: String,
}

#[derive(Template)]
#[template(path = "compare.html")]
pub struct CompareTemplate {
    pub active_nav: String,
    pub last_collected: String,
    pub all_products: Vec<Product>,
    pub selected_products: Vec<Product>,
    pub selected_json: String,
    pub selected_ids_csv: String,
}

#[derive(Template)]
#[template(path = "catalog.html")]
pub struct CatalogTemplate {
    pub active_nav: String,
    pub last_collected: String,
    pub brands: Vec<String>,
    pub products: Vec<Product>,
}

#[derive(Template)]
#[template(path = "partials/quiz_result.html")]
pub struct QuizResultTemplate {
    pub result: QuizRecommendationResult,
}

#[derive(Template)]
#[template(path = "partials/product_cards.html")]
pub struct ProductCardsTemplate {
    pub products: Vec<Product>,
}

#[derive(Template)]
#[template(path = "partials/rag_answer.html")]
pub struct RagAnswerTemplate {
    pub query: String,
    pub answer: RagAnswer,
}

#[derive(Template)]
#[template(path = "partials/compare_summary.html")]
pub struct CompareSummaryTemplate {
    pub summary_text: String,
    pub provider_used: String,
}

#[derive(Template)]
#[template(path = "partials/collection_logs.html")]
pub struct CollectionLogsTemplate {
    pub logs: Vec<CollectionLogEntry>,
}

#[derive(Template)]
#[template(path = "partials/glossary_list.html")]
pub struct GlossaryListTemplate {
    pub terms: Vec<GlossaryTerm>,
}

#[derive(Deserialize)]
pub struct RagAskForm {
    pub query: String,
}

#[derive(Deserialize)]
pub struct CompareQuery {
    pub ids: Option<String>,
}

#[derive(Deserialize)]
pub struct CompareSummaryForm {
    pub ids: String,
}

pub async fn home_page(State(state): State<AppState>) -> Result<IndexTemplate, AppError> {
    let total_products = {
        let conn = state.db.lock().map_err(|_| anyhow::anyhow!("DB 락 실패"))?;
        list_products(&conn, &ProductFilter::default(), state.reference_date)?.len()
    };
    Ok(IndexTemplate {
        active_nav: "home".to_string(),
        total_products,
        last_collected: latest_collected_label(&state.collector),
        llm_mode: state.rag.provider_label(),
    })
}

pub async fn quiz_recommend(
    State(state): State<AppState>,
    Form(input): Form<QuizInput>,
) -> Result<QuizResultTemplate, AppError> {
    let products = {
        let conn = state.db.lock().map_err(|_| anyhow::anyhow!("DB 락 실패"))?;
        list_products(&conn, &ProductFilter::default(), state.reference_date)?
    };
    let result = evaluate_quiz(&products, &input);
    Ok(QuizResultTemplate { result })
}

pub async fn chat_page(State(state): State<AppState>) -> Result<ChatTemplate, AppError> {
    Ok(ChatTemplate {
        active_nav: "chat".to_string(),
        last_collected: latest_collected_label(&state.collector),
        llm_mode: state.rag.provider_label(),
    })
}

pub async fn rag_ask(
    State(state): State<AppState>,
    Form(form): Form<RagAskForm>,
) -> Result<RagAnswerTemplate, AppError> {
    let trimmed = form.query.trim().to_string();
    let answer = state.rag.answer(&trimmed, 4).await?;
    Ok(RagAnswerTemplate {
        query: trimmed,
        answer,
    })
}

pub async fn compare_page(
    State(state): State<AppState>,
    Query(q): Query<CompareQuery>,
) -> Result<CompareTemplate, AppError> {
    let (all_products, selected_products) = {
        let conn = state.db.lock().map_err(|_| anyhow::anyhow!("DB 락 실패"))?;
        let all = list_products(&conn, &ProductFilter::default(), state.reference_date)?;
        let ids: Vec<String> = q
            .ids
            .unwrap_or_default()
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        let selected = if ids.is_empty() {
            // 첫 방문 시 과제 시연에 좋은 대표 3종 기본 비교
            get_products_by_ids(
                &conn,
                &[
                    "sony-wf-1000xm6".to_string(),
                    "sennheiser-momentum-true-wireless-5".to_string(),
                    "shure-se215".to_string(),
                ],
            )?
        } else {
            get_products_by_ids(&conn, &ids)?
        };
        (all, selected)
    };

    let selected_ids_csv = selected_products
        .iter()
        .map(|p| p.id.clone())
        .collect::<Vec<_>>()
        .join(",");
    let selected_json = serde_json::to_string(&selected_products)?;

    Ok(CompareTemplate {
        active_nav: "compare".to_string(),
        last_collected: latest_collected_label(&state.collector),
        all_products,
        selected_products,
        selected_json,
        selected_ids_csv,
    })
}

pub async fn compare_summary(
    State(state): State<AppState>,
    Form(form): Form<CompareSummaryForm>,
) -> Result<CompareSummaryTemplate, AppError> {
    let ids: Vec<String> = form
        .ids
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    let selected = {
        let conn = state.db.lock().map_err(|_| anyhow::anyhow!("DB 락 실패"))?;
        get_products_by_ids(&conn, &ids)?
    };
    let summary_text = state.rag.summarize_comparison(&selected).await?;
    Ok(CompareSummaryTemplate {
        summary_text,
        provider_used: state.rag.provider_label(),
    })
}

pub async fn catalog_page(State(state): State<AppState>) -> Result<CatalogTemplate, AppError> {
    let products = {
        let conn = state.db.lock().map_err(|_| anyhow::anyhow!("DB 락 실패"))?;
        list_products(&conn, &ProductFilter::default(), state.reference_date)?
    };
    Ok(CatalogTemplate {
        active_nav: "catalog".to_string(),
        last_collected: latest_collected_label(&state.collector),
        brands: TOP10_BRANDS.iter().map(|s| s.to_string()).collect(),
        products,
    })
}

pub async fn products_partial(
    State(state): State<AppState>,
    Query(filter): Query<ProductFilter>,
) -> Result<ProductCardsTemplate, AppError> {
    let products = {
        let conn = state.db.lock().map_err(|_| anyhow::anyhow!("DB 락 실패"))?;
        list_products(&conn, &filter, state.reference_date)?
    };
    Ok(ProductCardsTemplate { products })
}

pub async fn collect_run_sse(
    State(state): State<AppState>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let (tx, rx) = tokio::sync::mpsc::channel(64);
    let collector = state.collector.clone();

    tokio::spawn(async move {
        let _ = collector.run_collection(tx).await;
    });

    let stream = ReceiverStream::new(rx).map(|ev| {
        let event = Event::default()
            .json_data(ev)
            .unwrap_or_else(|_| Event::default().data("{}"));
        Ok(event)
    });

    Sse::new(stream).keep_alive(KeepAlive::default())
}

pub async fn collect_logs_partial(
    State(state): State<AppState>,
) -> Result<CollectionLogsTemplate, AppError> {
    let logs = state.collector.recent_logs(5)?;
    Ok(CollectionLogsTemplate { logs })
}

pub async fn glossary_partial(
    State(state): State<AppState>,
) -> Result<GlossaryListTemplate, AppError> {
    let terms = {
        let conn = state.db.lock().map_err(|_| anyhow::anyhow!("DB 락 실패"))?;
        list_glossary_terms(&conn)?
    };
    Ok(GlossaryListTemplate { terms })
}
