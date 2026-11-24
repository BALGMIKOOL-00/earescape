use crate::db::repository::{
    insert_collection_log, mark_product_verified, recent_collection_logs, upsert_seed_products,
    CollectionLogEntry,
};
use crate::domain::product::{passes_release_policy, SeedProduct};
use crate::rag::pipeline::RagPipeline;
use chrono::{Local, NaiveDate};
use rusqlite::Connection;
use scraper::{Html, Selector};
use serde::Serialize;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::sync::mpsc;

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "stage", rename_all = "snake_case")]
pub enum CollectEvent {
    Started {
        run_id: String,
        total: usize,
    },
    Checking {
        index: usize,
        total: usize,
        brand: String,
        product_name: String,
        target_url: String,
    },
    ItemDone {
        index: usize,
        total: usize,
        brand: String,
        product_name: String,
        policy_label: String,
        status: String,
        http_status: Option<u16>,
        extracted_title: Option<String>,
        message: String,
        elapsed_ms: u64,
    },
    Completed {
        run_id: String,
        total_checked: usize,
        live_ok: usize,
        fallback_seed: usize,
        filtered_out: usize,
        summary_message: String,
    },
    Failed {
        message: String,
    },
}

pub struct CollectorService {
    http: reqwest::Client,
    db: Arc<Mutex<Connection>>,
    rag: Arc<RagPipeline>,
    seed_products_path: String,
    reference_date: NaiveDate,
    is_running: AtomicBool,
}

impl CollectorService {
    pub fn new(
        db: Arc<Mutex<Connection>>,
        rag: Arc<RagPipeline>,
        seed_products_path: String,
        reference_date: NaiveDate,
    ) -> anyhow::Result<Self> {
        let http = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(3))
            .timeout(Duration::from_secs(6))
            .user_agent("EarEscape-AudioGuide/2.0 (Educational Rust Collector)")
            .build()?;

        Ok(Self {
            http,
            db,
            rag,
            seed_products_path,
            reference_date,
            is_running: AtomicBool::new(false),
        })
    }

    pub async fn run_collection(&self, tx: mpsc::Sender<CollectEvent>) -> anyhow::Result<()> {
        if self
            .is_running
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
        {
            let _ = tx
                .send(CollectEvent::Failed {
                    message: "이미 데이터 수집이 진행 중입니다. 완료 후 다시 시도해 주세요.".to_string(),
                })
                .await;
            return Ok(());
        }

        let res = self.run_collection_inner(tx).await;
        self.is_running.store(false, Ordering::SeqCst);
        res
    }

    async fn run_collection_inner(&self, tx: mpsc::Sender<CollectEvent>) -> anyhow::Result<()> {
        let run_id = format!("run-{}", Local::now().format("%Y%m%d-%H%M%S"));

        let raw = match std::fs::read_to_string(&self.seed_products_path) {
            Ok(content) => content,
            Err(e) => {
                let msg = format!("시드 파일 로드 실패: {}", e);
                let _ = tx.send(CollectEvent::Failed { message: msg.clone() }).await;
                return Err(anyhow::anyhow!(msg));
            }
        };

        let all_seeds: Vec<SeedProduct> = serde_json::from_str(&raw)?;
        let (upserted, filtered_out) = {
            let mut conn = self.db.lock().map_err(|_| anyhow::anyhow!("DB 락 획득 실패"))?;
            upsert_seed_products(&mut conn, &all_seeds, self.reference_date)?
        };

        let valid_seeds: Vec<SeedProduct> = all_seeds
            .into_iter()
            .filter(|p| passes_release_policy(&p.brand, p.release_date.as_deref(), self.reference_date))
            .collect();

        let total = valid_seeds.len();
        let _ = tx
            .send(CollectEvent::Started {
                run_id: run_id.clone(),
                total,
            })
            .await;

        let mut live_ok = 0usize;
        let mut fallback_seed = 0usize;

        for (idx, item) in valid_seeds.iter().enumerate() {
            let index = idx + 1;
            let _ = tx
                .send(CollectEvent::Checking {
                    index,
                    total,
                    brand: item.brand.clone(),
                    product_name: item.name.clone(),
                    target_url: item.source_url.clone(),
                })
                .await;

            let started = Instant::now();
            let (status, http_status, extracted_title, message) = match self
                .http
                .get(&item.source_url)
                .send()
                .await
            {
                Ok(resp) => {
                    let code = resp.status().as_u16();
                    if resp.status().is_success() {
                        let body = resp.text().await.unwrap_or_default();
                        let title = extract_html_title(&body);
                        live_ok += 1;
                        if let Ok(conn) = self.db.lock() {
                            let _ = mark_product_verified(&conn, &item.id);
                        }
                        (
                            "live_ok".to_string(),
                            Some(code),
                            title,
                            format!("공식 페이지 응답 확인 ({} · 정책 통과)", item.release_policy.label_ko()),
                        )
                    } else {
                        fallback_seed += 1;
                        (
                            "fallback_seed".to_string(),
                            Some(code),
                            None,
                            format!("HTTP {} 응답 — 검증된 시드 데이터 유지", code),
                        )
                    }
                }
                Err(_) => {
                    fallback_seed += 1;
                    (
                        "fallback_seed".to_string(),
                        None,
                        None,
                        "네트워크 제한/타임아웃 — 검증된 로컬 시드 데이터로 안전 반영".to_string(),
                    )
                }
            };

            let elapsed_ms = started.elapsed().as_millis() as u64;
            let _ = tx
                .send(CollectEvent::ItemDone {
                    index,
                    total,
                    brand: item.brand.clone(),
                    product_name: item.name.clone(),
                    policy_label: item.release_policy.label_ko().to_string(),
                    status,
                    http_status,
                    extracted_title,
                    message,
                    elapsed_ms,
                })
                .await;
        }

        let _ = self.rag.ensure_embeddings().await;

        let log_status = if live_ok == total && total > 0 {
            "success"
        } else if live_ok > 0 {
            "partial"
        } else {
            "fallback"
        };

        let summary_message = format!(
            "TOP 10 브랜드 {}개 제품 수집·검증 완료 (공식 페이지 확인 {}건 / 시드 폴백 {}건 / 정책 제외 {}건)",
            upserted, live_ok, fallback_seed, filtered_out
        );

        {
            let conn = self.db.lock().map_err(|_| anyhow::anyhow!("DB 락 획득 실패"))?;
            insert_collection_log(
                &conn,
                &run_id,
                total,
                live_ok,
                fallback_seed,
                filtered_out,
                log_status,
                &summary_message,
            )?;
        }

        let _ = tx
            .send(CollectEvent::Completed {
                run_id,
                total_checked: total,
                live_ok,
                fallback_seed,
                filtered_out,
                summary_message,
            })
            .await;

        Ok(())
    }

    pub fn recent_logs(&self, limit: usize) -> anyhow::Result<Vec<CollectionLogEntry>> {
        let conn = self.db.lock().map_err(|_| anyhow::anyhow!("DB 락 획득 실패"))?;
        recent_collection_logs(&conn, limit)
    }
}

/// 주의: scraper::Html은 !Send이므로 반드시 동기 함수 내부에서만 파싱 후 String 반환
pub fn extract_html_title(html: &str) -> Option<String> {
    let document = Html::parse_document(html);
    let selector = Selector::parse("title").ok()?;
    let title = document
        .select(&selector)
        .next()?
        .text()
        .collect::<Vec<_>>()
        .join(" ");
    let trimmed = title.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.chars().take(120).collect())
    }
}
