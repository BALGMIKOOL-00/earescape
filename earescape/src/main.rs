use ear_escape::collector::CollectorService;
use ear_escape::config::AppConfig;
use ear_escape::db::repository::{init_database, load_seed_files_if_empty};
use ear_escape::rag::pipeline::RagPipeline;
use ear_escape::web::handlers::AppState;
use ear_escape::web::routes::create_router;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .init();

    let config = AppConfig::from_env();
    let mut conn = init_database(&config.database_path)?;

    let (loaded_products, loaded_glossary) = load_seed_files_if_empty(
        &mut conn,
        "data/seed_products.json",
        "data/seed_glossary.json",
        config.reference_date,
    )?;

    if loaded_products > 0 || loaded_glossary > 0 {
        tracing::info!(
            "초기 시드 데이터 적재 완료: 제품 {}개, 음향 용어 {}개",
            loaded_products,
            loaded_glossary
        );
    }

    let db = Arc::new(Mutex::new(conn));
    let rag = Arc::new(RagPipeline::from_config(db.clone(), &config)?);
    let embedded = rag.ensure_embeddings().await?;
    if embedded > 0 {
        tracing::info!("RAG 검색용 벡터 임베딩 {}건 생성 완료", embedded);
    }

    let collector = Arc::new(CollectorService::new(
        db.clone(),
        rag.clone(),
        "data/seed_products.json".to_string(),
        config.reference_date,
    )?);

    let state = AppState {
        db,
        collector,
        rag,
        reference_date: config.reference_date,
    };

    let app = create_router(state);
    let addr = SocketAddr::from(([0, 0, 0, 0], config.port));

    println!("\n========================================================");
    println!("  막귀탈출 (EarEscape) 웹 서버가 시작되었습니다!");
    println!("  브라우저에서 http://localhost:{} 로 접속하세요.", config.port);
    println!("  기준일: {} | TOP 10 브랜드 큐레이션 활성화", config.reference_date);
    println!("========================================================\n");

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}
