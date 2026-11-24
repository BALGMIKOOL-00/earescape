use crate::domain::product::{
    passes_release_policy, FormFactor, GlossaryTerm, Product, ProductFilter, ReleasePolicy,
    SeedProduct, SoundProfile,
};
use anyhow::Context;
use chrono::NaiveDate;
use rusqlite::{params, Connection};
use serde::Serialize;
use std::path::Path;

const INIT_SQL: &str = include_str!("../../migrations/001_init.sql");

#[derive(Debug, Clone, Serialize)]
pub struct CollectionLogEntry {
    pub id: i64,
    pub run_id: String,
    pub triggered_at: String,
    pub total_checked: i64,
    pub live_ok_count: i64,
    pub fallback_count: i64,
    pub filtered_out_count: i64,
    pub status: String,
    pub summary_message: String,
}

#[derive(Debug, Clone)]
pub struct ChunkRecord {
    pub id: i64,
    pub product_id: String,
    pub product_name: String,
    pub brand: String,
    pub release_policy: ReleasePolicy,
    pub content: String,
    pub source_url: String,
    pub embedding: Option<Vec<u8>>,
}

pub fn init_database(path: &str) -> anyhow::Result<Connection> {
    if path != ":memory:" {
        if let Some(parent) = Path::new(path).parent() {
            std::fs::create_dir_all(parent)?;
        }
    }
    let conn = Connection::open(path).with_context(|| format!("SQLite DB 열기 실패: {}", path))?;
    conn.execute_batch(
        "PRAGMA journal_mode = WAL; PRAGMA busy_timeout = 5000; PRAGMA foreign_keys = ON;",
    )?;
    conn.execute_batch(INIT_SQL)?;
    Ok(conn)
}

/// 초기 시드 제품(TOP 10 정책 검증 적용) 적재 및 업서트
pub fn upsert_seed_products(
    conn: &mut Connection,
    products: &[SeedProduct],
    reference_date: NaiveDate,
) -> anyhow::Result<(usize, usize)> {
    let tx = conn.transaction()?;
    let mut upserted = 0usize;
    let mut filtered_out = 0usize;

    for p in products {
        if !passes_release_policy(&p.brand, p.release_date.as_deref(), reference_date) {
            filtered_out += 1;
            continue;
        }

        let codecs_json = serde_json::to_string(&p.codecs)?;
        tx.execute(
            "INSERT INTO products (
                id, brand, name, form_factor, release_date, release_policy,
                price_usd, price_krw_est, anc_support, battery_hours, codecs,
                water_resistance, beginner_summary, source_url, updated_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, datetime('now'))
             ON CONFLICT(id) DO UPDATE SET
                brand = excluded.brand,
                name = excluded.name,
                form_factor = excluded.form_factor,
                release_date = excluded.release_date,
                release_policy = excluded.release_policy,
                price_usd = excluded.price_usd,
                price_krw_est = excluded.price_krw_est,
                anc_support = excluded.anc_support,
                battery_hours = excluded.battery_hours,
                codecs = excluded.codecs,
                water_resistance = excluded.water_resistance,
                beginner_summary = excluded.beginner_summary,
                source_url = excluded.source_url,
                updated_at = datetime('now')",
            params![
                p.id,
                p.brand,
                p.name,
                p.form_factor.as_str(),
                p.release_date,
                p.release_policy.as_str(),
                p.price_usd,
                p.price_krw_est,
                if p.anc_support { 1 } else { 0 },
                p.battery_hours,
                codecs_json,
                p.water_resistance,
                p.beginner_summary,
                p.source_url,
            ],
        )?;

        tx.execute(
            "INSERT INTO sound_profiles (
                product_id, bass, mid, treble, soundstage, isolation_anc, comfort, signature_label
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
             ON CONFLICT(product_id) DO UPDATE SET
                bass = excluded.bass,
                mid = excluded.mid,
                treble = excluded.treble,
                soundstage = excluded.soundstage,
                isolation_anc = excluded.isolation_anc,
                comfort = excluded.comfort,
                signature_label = excluded.signature_label",
            params![
                p.id,
                p.sound_profile.bass,
                p.sound_profile.mid,
                p.sound_profile.treble,
                p.sound_profile.soundstage,
                p.sound_profile.isolation_anc,
                p.sound_profile.comfort,
                p.sound_profile.signature_label,
            ],
        )?;

        // 기존 청크 교체 (중복 누적 방지)
        tx.execute(
            "DELETE FROM review_chunks WHERE product_id = ?1",
            params![p.id],
        )?;

        // 요약 청크 + 리뷰 청크 저장
        let summary_text = format!(
            "{} {} ({}, 약 {}만 원, ANC: {}): {}",
            p.brand,
            p.name,
            p.form_factor.label_ko(),
            (p.price_krw_est + 5_000) / 10_000,
            if p.anc_support { "지원" } else { "미지원" },
            p.beginner_summary
        );
        tx.execute(
            "INSERT INTO review_chunks (product_id, chunk_type, content, source_url)
             VALUES (?1, 'summary', ?2, ?3)",
            params![p.id, summary_text, p.source_url],
        )?;

        for chunk in &p.review_chunks {
            tx.execute(
                "INSERT INTO review_chunks (product_id, chunk_type, content, source_url)
                 VALUES (?1, ?2, ?3, ?4)",
                params![p.id, chunk.chunk_type, chunk.content, p.source_url],
            )?;
        }

        upserted += 1;
    }

    tx.commit()?;
    Ok((upserted, filtered_out))
}

pub fn upsert_glossary(conn: &mut Connection, terms: &[GlossaryTerm]) -> anyhow::Result<usize> {
    let tx = conn.transaction()?;
    for t in terms {
        tx.execute(
            "INSERT INTO glossary_terms (slug, term_ko, term_en, short_analogy, detail_markdown)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(slug) DO UPDATE SET
                term_ko = excluded.term_ko,
                term_en = excluded.term_en,
                short_analogy = excluded.short_analogy,
                detail_markdown = excluded.detail_markdown",
            params![t.slug, t.term_ko, t.term_en, t.short_analogy, t.detail_markdown],
        )?;
    }
    tx.commit()?;
    Ok(terms.len())
}

pub fn load_seed_files_if_empty(
    conn: &mut Connection,
    products_path: &str,
    glossary_path: &str,
    reference_date: NaiveDate,
) -> anyhow::Result<(usize, usize)> {
    let count: i64 = conn.query_row("SELECT COUNT(*) FROM products", [], |r| r.get(0))?;
    let mut loaded_products = 0usize;
    if count == 0 {
        let raw = std::fs::read_to_string(products_path)
            .with_context(|| format!("시드 제품 파일 읽기 실패: {}", products_path))?;
        let seeds: Vec<SeedProduct> = serde_json::from_str(&raw)?;
        let (upserted, _) = upsert_seed_products(conn, &seeds, reference_date)?;
        loaded_products = upserted;
    }

    let g_count: i64 = conn.query_row("SELECT COUNT(*) FROM glossary_terms", [], |r| r.get(0))?;
    let mut loaded_glossary = 0usize;
    if g_count == 0 {
        let raw = std::fs::read_to_string(glossary_path)
            .with_context(|| format!("용어 사전 파일 읽기 실패: {}", glossary_path))?;
        let terms: Vec<GlossaryTerm> = serde_json::from_str(&raw)?;
        loaded_glossary = upsert_glossary(conn, &terms)?;
    }

    Ok((loaded_products, loaded_glossary))
}

fn map_product_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Product> {
    let ff_str: String = row.get(3)?;
    let policy_str: String = row.get(5)?;
    let anc_int: i64 = row.get(8)?;
    let codecs_json: String = row.get(10)?;
    let codecs: Vec<String> = serde_json::from_str(&codecs_json).unwrap_or_default();

    Ok(Product {
        id: row.get(0)?,
        brand: row.get(1)?,
        name: row.get(2)?,
        form_factor: FormFactor::from_str_opt(&ff_str).unwrap_or(FormFactor::TwsInEar),
        release_date: row.get(4)?,
        release_policy: ReleasePolicy::from_db(&policy_str),
        price_usd: row.get(6)?,
        price_krw_est: row.get(7)?,
        anc_support: anc_int != 0,
        battery_hours: row.get(9)?,
        codecs,
        water_resistance: row.get(11)?,
        beginner_summary: row.get(12)?,
        source_url: row.get(13)?,
        last_verified_at: row.get(14)?,
        sound_profile: SoundProfile {
            bass: row.get::<_, i64>(15)? as u8,
            mid: row.get::<_, i64>(16)? as u8,
            treble: row.get::<_, i64>(17)? as u8,
            soundstage: row.get::<_, i64>(18)? as u8,
            isolation_anc: row.get::<_, i64>(19)? as u8,
            comfort: row.get::<_, i64>(20)? as u8,
            signature_label: row.get(21)?,
        },
    })
}

pub fn list_products(
    conn: &Connection,
    filter: &ProductFilter,
    reference_date: NaiveDate,
) -> anyhow::Result<Vec<Product>> {
    let mut stmt = conn.prepare(
        "SELECT p.id, p.brand, p.name, p.form_factor, p.release_date, p.release_policy,
                p.price_usd, p.price_krw_est, p.anc_support, p.battery_hours, p.codecs,
                p.water_resistance, p.beginner_summary, p.source_url, p.last_verified_at,
                s.bass, s.mid, s.treble, s.soundstage, s.isolation_anc, s.comfort, s.signature_label
         FROM products p
         INNER JOIN sound_profiles s ON p.id = s.product_id",
    )?;

    let rows = stmt.query_map([], map_product_row)?;
    let mut items = Vec::new();

    for r in rows {
        let p = r?;
        if !passes_release_policy(&p.brand, p.release_date.as_deref(), reference_date) {
            continue;
        }
        if let Some(b) = filter.brand.as_deref() {
            if !b.is_empty() && b != "all" && !p.brand.eq_ignore_ascii_case(b) {
                continue;
            }
        }
        if let Some(ff) = filter.form_factor.as_deref() {
            if !ff.is_empty() && ff != "all" && p.form_factor.as_str() != ff {
                continue;
            }
        }
        if let Some(pol) = filter.policy.as_deref() {
            if !pol.is_empty() && pol != "all" && p.release_policy.as_str() != pol {
                continue;
            }
        }
        if filter.anc_only == Some(true) && !p.anc_support {
            continue;
        }
        if let Some(max_m) = filter.max_budget_manwon {
            if max_m > 0 && p.price_krw_est > max_m * 10_000 {
                continue;
            }
        }
        items.push(p);
    }

    match filter.sort.as_deref() {
        Some("price_asc") => items.sort_by_key(|p| p.price_krw_est),
        Some("price_desc") => items.sort_by(|a, b| b.price_krw_est.cmp(&a.price_krw_est)),
        _ => items.sort_by(|a, b| b.release_date.cmp(&a.release_date).then(a.brand.cmp(&b.brand))),
    }

    Ok(items)
}

pub fn get_products_by_ids(conn: &Connection, ids: &[String]) -> anyhow::Result<Vec<Product>> {
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    let mut results = Vec::new();
    let mut stmt = conn.prepare(
        "SELECT p.id, p.brand, p.name, p.form_factor, p.release_date, p.release_policy,
                p.price_usd, p.price_krw_est, p.anc_support, p.battery_hours, p.codecs,
                p.water_resistance, p.beginner_summary, p.source_url, p.last_verified_at,
                s.bass, s.mid, s.treble, s.soundstage, s.isolation_anc, s.comfort, s.signature_label
         FROM products p
         INNER JOIN sound_profiles s ON p.id = s.product_id
         WHERE p.id = ?1",
    )?;
    for id in ids.iter().take(4) {
        let mut rows = stmt.query_map(params![id], map_product_row)?;
        if let Some(Ok(p)) = rows.next() {
            results.push(p);
        }
    }
    Ok(results)
}

pub fn list_glossary_terms(conn: &Connection) -> anyhow::Result<Vec<GlossaryTerm>> {
    let mut stmt = conn.prepare(
        "SELECT slug, term_ko, term_en, short_analogy, detail_markdown
         FROM glossary_terms ORDER BY term_ko ASC",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok(GlossaryTerm {
            slug: r.get(0)?,
            term_ko: r.get(1)?,
            term_en: r.get(2)?,
            short_analogy: r.get(3)?,
            detail_markdown: r.get(4)?,
        })
    })?;
    let mut terms = Vec::new();
    for r in rows {
        terms.push(r?);
    }
    Ok(terms)
}

pub fn mark_product_verified(conn: &Connection, product_id: &str) -> anyhow::Result<()> {
    conn.execute(
        "UPDATE products SET last_verified_at = datetime('now') WHERE id = ?1",
        params![product_id],
    )?;
    Ok(())
}

pub fn insert_collection_log(
    conn: &Connection,
    run_id: &str,
    total_checked: usize,
    live_ok_count: usize,
    fallback_count: usize,
    filtered_out_count: usize,
    status: &str,
    summary_message: &str,
) -> anyhow::Result<()> {
    conn.execute(
        "INSERT INTO collection_logs (
            run_id, total_checked, live_ok_count, fallback_count, filtered_out_count, status, summary_message
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            run_id,
            total_checked as i64,
            live_ok_count as i64,
            fallback_count as i64,
            filtered_out_count as i64,
            status,
            summary_message
        ],
    )?;
    Ok(())
}

pub fn recent_collection_logs(
    conn: &Connection,
    limit: usize,
) -> anyhow::Result<Vec<CollectionLogEntry>> {
    let mut stmt = conn.prepare(
        "SELECT id, run_id, triggered_at, total_checked, live_ok_count,
                fallback_count, filtered_out_count, status, summary_message
         FROM collection_logs
         ORDER BY id DESC
         LIMIT ?1",
    )?;
    let rows = stmt.query_map(params![limit as i64], |r| {
        Ok(CollectionLogEntry {
            id: r.get(0)?,
            run_id: r.get(1)?,
            triggered_at: r.get(2)?,
            total_checked: r.get(3)?,
            live_ok_count: r.get(4)?,
            fallback_count: r.get(5)?,
            filtered_out_count: r.get(6)?,
            status: r.get(7)?,
            summary_message: r.get(8)?,
        })
    })?;
    let mut logs = Vec::new();
    for r in rows {
        logs.push(r?);
    }
    Ok(logs)
}

pub fn list_all_chunks(
    conn: &Connection,
    reference_date: NaiveDate,
) -> anyhow::Result<Vec<ChunkRecord>> {
    let mut stmt = conn.prepare(
        "SELECT c.id, c.product_id, p.name, p.brand, p.release_date, p.release_policy,
                c.content, c.source_url, c.embedding
         FROM review_chunks c
         INNER JOIN products p ON c.product_id = p.id",
    )?;
    let rows = stmt.query_map([], |r| {
        let brand: String = r.get(3)?;
        let release_date: Option<String> = r.get(4)?;
        let policy_str: String = r.get(5)?;
        Ok((
            release_date,
            ChunkRecord {
                id: r.get(0)?,
                product_id: r.get(1)?,
                product_name: r.get(2)?,
                brand,
                release_policy: ReleasePolicy::from_db(&policy_str),
                content: r.get(6)?,
                source_url: r.get(7)?,
                embedding: r.get(8)?,
            },
        ))
    })?;

    let mut out = Vec::new();
    for r in rows {
        let (release_date, rec) = r?;
        if passes_release_policy(&rec.brand, release_date.as_deref(), reference_date) {
            out.push(rec);
        }
    }
    Ok(out)
}

pub fn update_chunk_embedding(
    conn: &Connection,
    chunk_id: i64,
    embedding_bytes: &[u8],
    dim: usize,
    model_name: &str,
) -> anyhow::Result<()> {
    conn.execute(
        "UPDATE review_chunks
         SET embedding = ?1, embedding_dim = ?2, embedding_model = ?3
         WHERE id = ?4",
        params![embedding_bytes, dim as i64, model_name, chunk_id],
    )?;
    Ok(())
}

pub fn search_fts_chunks(
    conn: &Connection,
    sanitized_query: &str,
    limit: usize,
) -> anyhow::Result<Vec<i64>> {
    if sanitized_query.trim().is_empty() {
        return Ok(Vec::new());
    }
    let mut stmt = conn.prepare(
        "SELECT rowid FROM review_chunks_fts
         WHERE review_chunks_fts MATCH ?1
         ORDER BY bm25(review_chunks_fts)
         LIMIT ?2",
    )?;
    let rows = stmt.query_map(params![sanitized_query, limit as i64], |r| r.get::<_, i64>(0))?;
    let mut ids = Vec::new();
    for r in rows {
        ids.push(r?);
    }
    Ok(ids)
}
