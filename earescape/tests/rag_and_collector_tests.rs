use chrono::NaiveDate;
use ear_escape::collector::extract_html_title;
use ear_escape::db::repository::{
    init_database, insert_collection_log, list_products, load_seed_files_if_empty,
    recent_collection_logs, upsert_seed_products,
};
use ear_escape::domain::product::{
    validate_catalog_quota, ProductFilter, SeedProduct, TARGET_TOTAL_PRODUCTS,
};
use ear_escape::rag::pipeline::{
    cosine_similarity, decode_embedding, encode_embedding, sanitize_fts5_query, RagPipeline,
};
use std::sync::{Arc, Mutex};

#[test]
fn test_embedding_roundtrip_and_cosine() {
    let v = vec![0.6_f32, 0.8_f32, 0.0_f32];
    let bytes = encode_embedding(&v);
    let decoded = decode_embedding(&bytes);
    assert_eq!(v, decoded);
    let sim = cosine_similarity(&v, &decoded);
    assert!((sim - 1.0).abs() < 1e-5);
}

#[test]
fn test_fts5_query_sanitization_and_adversarial_inputs() {
    let q = sanitize_fts5_query("WF-1000XM6 vs SE215? \"저음\"");
    assert!(q.contains("\"WF1000XM6\"*"));
    assert!(q.contains("\"SE215\"*"));
    assert!(q.contains("\"저음\"*"));
    assert!(!q.contains('?'));

    assert_eq!(sanitize_fts5_query("   "), "");
    assert_eq!(sanitize_fts5_query("!@#$%\"'*---"), "");
}

#[test]
fn test_html_title_extraction() {
    let html = "<html><head><title>  Sony WF-1000XM6 Official  </title></head><body></body></html>";
    assert_eq!(
        extract_html_title(html),
        Some("Sony WF-1000XM6 Official".to_string())
    );
}

#[tokio::test]
async fn test_sqlite_seed_50_products_quota_idempotency_and_rag_e2e() {
    let reference = NaiveDate::from_ymd_opt(2026, 11, 23).unwrap();
    let mut conn = init_database(":memory:").expect("in-memory SQLite 초기화 성공");

    let (loaded_p, loaded_g) = load_seed_files_if_empty(
        &mut conn,
        "data/seed_products.json",
        "data/seed_glossary.json",
        reference,
    )
    .expect("50개 시드 데이터 로드 성공");

    assert_eq!(loaded_p, TARGET_TOTAL_PRODUCTS);
    assert_eq!(loaded_p, 50);
    assert_eq!(loaded_g, 12);

    // 멱등성 테스트: 재수집(업서트) 2회 실행 시에도 정확히 50개 유지
    let raw = std::fs::read_to_string("data/seed_products.json").unwrap();
    let seeds: Vec<SeedProduct> = serde_json::from_str(&raw).unwrap();
    let (upserted, filtered) = upsert_seed_products(&mut conn, &seeds, reference).unwrap();
    assert_eq!(upserted, 50);
    assert_eq!(filtered, 0);

    let all = list_products(&conn, &ProductFilter::default(), reference).unwrap();
    assert_eq!(all.len(), 50);

    // 핵심 쿼터 검증: TOP 10 브랜드 각 5종(일반 8개사 40종 최근 1년 + 슈어·웨스톤 10종 스테디셀러)
    validate_catalog_quota(&all, reference).expect("TOP 10 x 5종 = 50개 정책 쿼터 통과");

    insert_collection_log(&conn, "test-run-50", 50, 45, 5, 0, "partial", "50종 테스트 수집 완료")
        .unwrap();
    let logs = recent_collection_logs(&conn, 5).unwrap();
    assert_eq!(logs.len(), 1);
    assert_eq!(logs[0].total_checked, 50);

    let db = Arc::new(Mutex::new(conn));
    let rag = RagPipeline::new_offline(db, reference);
    let embedded = rag.ensure_embeddings().await.unwrap();
    assert!(embedded >= 50);

    // 슈어/웨스톤 스테디셀러 및 최신 1년 제품 검색 검증
    let answer = rag.answer("슈어 SE215 차음과 저음 알려줘", 4).await.unwrap();
    assert!(!answer.citations.is_empty());
    assert!(answer.citations.iter().any(|c| c.brand == "Shure"));

    let westone_ans = rag.answer("웨스톤 Pro X30 모니터링 이어폰", 4).await.unwrap();
    assert!(westone_ans.citations.iter().any(|c| c.brand == "Westone Audio"));

    // 적대적 특수문자 쿼리 안전 폴백 검증
    let safe_answer = rag.answer("!@#$%\"'*", 3).await.unwrap();
    assert!(!safe_answer.answer_text.is_empty());
}
