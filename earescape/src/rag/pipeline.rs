use crate::config::AppConfig;
use crate::db::repository::{list_all_chunks, search_fts_chunks, update_chunk_embedding};
use crate::domain::product::{Product, ReleasePolicy};
use crate::rag::provider::{
    EmbeddingProvider, FallbackMockEmbeddingProvider, FallbackMockLlmProvider,
    GeminiEmbeddingProvider, GeminiLlmProvider, LlmProvider,
};
use chrono::NaiveDate;
use rusqlite::Connection;
use serde::Serialize;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone, Serialize)]
pub struct HybridSearchHit {
    pub chunk_id: i64,
    pub product_id: String,
    pub product_name: String,
    pub brand: String,
    pub policy_label: String,
    pub is_evergreen: bool,
    pub content: String,
    pub source_url: String,
    pub rrf_score: f32,
}

#[derive(Debug, Clone, Serialize)]
pub struct RagAnswer {
    pub answer_text: String,
    pub provider_used: String,
    pub citations: Vec<HybridSearchHit>,
}

pub struct RagPipeline {
    db: Arc<Mutex<Connection>>,
    embedder: Arc<dyn EmbeddingProvider>,
    llm: Arc<dyn LlmProvider>,
    reference_date: NaiveDate,
}

impl RagPipeline {
    pub fn from_config(db: Arc<Mutex<Connection>>, config: &AppConfig) -> anyhow::Result<Self> {
        let (embedder, llm): (Arc<dyn EmbeddingProvider>, Arc<dyn LlmProvider>) =
            if config.llm_provider.eq_ignore_ascii_case("gemini") {
                if let Some(ref api_key) = config.gemini_api_key {
                    (
                        Arc::new(GeminiEmbeddingProvider::new(
                            api_key.clone(),
                            config.gemini_embed_model.clone(),
                        )?),
                        Arc::new(GeminiLlmProvider::new(
                            api_key.clone(),
                            config.gemini_chat_model.clone(),
                        )?),
                    )
                } else {
                    (
                        Arc::new(FallbackMockEmbeddingProvider),
                        Arc::new(FallbackMockLlmProvider),
                    )
                }
            } else {
                (
                    Arc::new(FallbackMockEmbeddingProvider),
                    Arc::new(FallbackMockLlmProvider),
                )
            };

        Ok(Self {
            db,
            embedder,
            llm,
            reference_date: config.reference_date,
        })
    }

    pub fn new_offline(db: Arc<Mutex<Connection>>, reference_date: NaiveDate) -> Self {
        Self {
            db,
            embedder: Arc::new(FallbackMockEmbeddingProvider),
            llm: Arc::new(FallbackMockLlmProvider),
            reference_date,
        }
    }

    pub fn provider_label(&self) -> String {
        self.llm.provider_name().to_string()
    }

    /// 임베딩이 비어 있는 청크에 대해 벡터 생성 후 SQLite BLOB 저장
    pub async fn ensure_embeddings(&self) -> anyhow::Result<usize> {
        let chunks = {
            let conn = self.db.lock().map_err(|_| anyhow::anyhow!("DB 락 획득 실패"))?;
            list_all_chunks(&conn, self.reference_date)?
        };

        let mut updated = 0usize;
        for ch in chunks {
            if ch.embedding.is_some() {
                continue;
            }
            let text = format!("{} {} {}", ch.brand, ch.product_name, ch.content);
            let vec = self.embedder.embed(&text).await?;
            let bytes = encode_embedding(&vec);
            {
                let conn = self.db.lock().map_err(|_| anyhow::anyhow!("DB 락 획득 실패"))?;
                update_chunk_embedding(&conn, ch.id, &bytes, vec.len(), self.embedder.model_name())?;
            }
            updated += 1;
        }
        Ok(updated)
    }

    /// FTS5 BM25 + 순수 Rust 코사인 유사도 + 한영 브랜드/모델명 부스팅 -> RRF (k=60) 하이브리드 검색
    pub async fn hybrid_search(&self, query: &str, top_k: usize) -> anyhow::Result<Vec<HybridSearchHit>> {
        let query_vec = self.embedder.embed(query).await?;
        let sanitized = sanitize_fts5_query(query);

        let (chunks, fts_ids) = {
            let conn = self.db.lock().map_err(|_| anyhow::anyhow!("DB 락 획득 실패"))?;
            let chunks = list_all_chunks(&conn, self.reference_date)?;
            let fts_ids = search_fts_chunks(&conn, &sanitized, 20).unwrap_or_default();
            (chunks, fts_ids)
        };

        // 벡터 유사도 순위 계산
        let mut vec_scored: Vec<(i64, f32)> = chunks
            .iter()
            .map(|ch| {
                let emb = match &ch.embedding {
                    Some(b) if !b.is_empty() => decode_embedding(b),
                    _ => FallbackMockEmbeddingProvider::embed_sync(&format!(
                        "{} {} {}",
                        ch.brand, ch.product_name, ch.content
                    )),
                };
                let sim = if emb.len() == query_vec.len() {
                    cosine_similarity(&query_vec, &emb)
                } else {
                    let local_q = FallbackMockEmbeddingProvider::embed_sync(query);
                    let local_d = FallbackMockEmbeddingProvider::embed_sync(&format!(
                        "{} {} {}",
                        ch.brand, ch.product_name, ch.content
                    ));
                    cosine_similarity(&local_q, &local_d)
                };
                (ch.id, sim)
            })
            .collect();

        vec_scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

        const RRF_K: f32 = 60.0;
        let mut rrf_scores: HashMap<i64, f32> = HashMap::new();

        for (rank, (id, _)) in vec_scored.iter().take(20).enumerate() {
            *rrf_scores.entry(*id).or_insert(0.0) += 1.0 / (RRF_K + (rank as f32) + 1.0);
        }
        for (rank, id) in fts_ids.iter().enumerate() {
            *rrf_scores.entry(*id).or_insert(0.0) += 1.0 / (RRF_K + (rank as f32) + 1.0);
        }

        // 한·영 브랜드 별칭 및 모델명 명시 매칭 부스팅 (50종 이상 규모에서 정확도 보장)
        let matched_brands = detect_brand_aliases(query);
        let query_lower = query.to_lowercase();
        let query_tokens: Vec<String> = query_lower
            .split_whitespace()
            .map(|t| {
                t.chars()
                    .filter(|c| c.is_alphanumeric())
                    .collect::<String>()
            })
            .filter(|t| t.len() >= 2 && t != "pro" && t != "air" && t != "max")
            .collect();

        for ch in &chunks {
            let mut boost = 0.0_f32;
            if matched_brands
                .iter()
                .any(|b| ch.brand.eq_ignore_ascii_case(b))
            {
                boost += 0.06;
            }
            let name_lower = ch.product_name.to_lowercase();
            let name_compact: String = name_lower
                .chars()
                .filter(|c| c.is_alphanumeric())
                .collect();
            for tok in &query_tokens {
                if name_lower.contains(tok) || name_compact.contains(tok) {
                    boost += 0.08;
                }
            }
            if boost > 0.0 {
                *rrf_scores.entry(ch.id).or_insert(0.0) += boost;
            }
        }

        let chunk_map: HashMap<i64, _> = chunks.into_iter().map(|c| (c.id, c)).collect();
        let mut ranked: Vec<(i64, f32)> = rrf_scores.into_iter().collect();
        ranked.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

        // 제품당 중복 노출 방지하며 top_k 선정
        let mut hits = Vec::new();
        let mut seen_products = std::collections::HashSet::new();

        for (id, score) in ranked {
            if let Some(ch) = chunk_map.get(&id) {
                if seen_products.insert(ch.product_id.clone()) {
                    hits.push(HybridSearchHit {
                        chunk_id: ch.id,
                        product_id: ch.product_id.clone(),
                        product_name: ch.product_name.clone(),
                        brand: ch.brand.clone(),
                        policy_label: ch.release_policy.label_ko().to_string(),
                        is_evergreen: ch.release_policy == ReleasePolicy::EvergreenPopular,
                        content: ch.content.clone(),
                        source_url: ch.source_url.clone(),
                        rrf_score: (score * 1000.0).round() / 1000.0,
                    });
                    if hits.len() >= top_k {
                        break;
                    }
                }
            }
        }

        Ok(hits)
    }

    pub async fn answer(&self, query: &str, top_k: usize) -> anyhow::Result<RagAnswer> {
        let hits = self.hybrid_search(query, top_k).await?;

        let system_prompt = "\
당신은 음향 입문자를 돕는 '막귀탈출(EarEscape)' 전문 오디오 에디터입니다.
규칙:
1) 반드시 아래 제공된 TOP 10 브랜드 근거 컨텍스트 내 제품과 스펙만 사용해 답변하세요.
2) 컨텍스트에 없는 수치나 출시일을 절대 지어내지 마세요.
3) 전문 음향 용어(BA 드라이버, ANC, LDAC 등)는 괄호 안에 입문자용 쉬운 비유를 덧붙이세요.
4) 추천 제품마다 정책 구분(최근 1년 신제품 / 인기 스테디셀러)과 핵심 추천 이유를 명확히 안내하세요.";

        let mut context_lines = String::new();
        for hit in &hits {
            context_lines.push_str(&format!(
                "- [{} {} · {}]: {} (출처: {})\n",
                hit.brand, hit.product_name, hit.policy_label, hit.content, hit.source_url
            ));
        }

        let user_prompt = format!(
            "사용자 질문: {}\n\n[검색된 TOP 10 근거 데이터]\n{}",
            query, context_lines
        );

        let answer_text = self.llm.generate(system_prompt, &user_prompt).await?;

        Ok(RagAnswer {
            answer_text,
            provider_used: self.llm.provider_name().to_string(),
            citations: hits,
        })
    }

    pub async fn summarize_comparison(&self, products: &[Product]) -> anyhow::Result<String> {
        if products.is_empty() {
            return Ok("비교할 제품을 2개 이상 선택해 주세요.".to_string());
        }

        let system_prompt = "\
당신은 '막귀탈출' 비교 스튜디오 에디터입니다.
선택된 이어폰·헤드폰들의 저음/중음/고음, ANC, 착용 방식, 가격 차이를 바탕으로 입문자가 즉시 결정할 수 있도록 정확히 3줄로 요약하세요.";

        let mut ctx = String::new();
        for p in products {
            ctx.push_str(&format!(
                "- [{} {} ({})]: 약 {}만 원, ANC {}, 저음 {}/중음 {}/고음 {}, 착용감 {}, 한줄평: {}\n",
                p.brand,
                p.name,
                p.form_factor_label(),
                p.price_manwon(),
                if p.anc_support { "지원" } else { "미지원" },
                p.sound_profile.bass,
                p.sound_profile.mid,
                p.sound_profile.treble,
                p.sound_profile.comfort,
                p.beginner_summary
            ));
        }

        self.llm.generate(system_prompt, &ctx).await
    }
}

/// 한국어·영문 브랜드명 별칭 감지
fn detect_brand_aliases(query: &str) -> Vec<&'static str> {
    let q = query.to_lowercase();
    let pairs: [(&str, &[&str]); 10] = [
        ("Apple", &["apple", "애플", "airpods", "에어팟", "beats", "비츠"]),
        ("Samsung", &["samsung", "삼성", "갤럭시", "버즈", "buds", "jbl"]),
        ("Sony", &["sony", "소니", "wf-1000", "wh-1000", "linkbuds", "링크버즈"]),
        ("Bose", &["bose", "보스", "quietcomfort", "콰이어트"]),
        ("Sennheiser", &["sennheiser", "젠하이저", "momentum", "모멘텀", "accentum"]),
        ("Shure", &["shure", "슈어", "se215", "se425", "se535", "se846", "aonic"]),
        ("Westone Audio", &["westone", "웨스톤", "웨스트스톤", "mach"]),
        ("Anker", &["anker", "앤커", "soundcore", "사운드코어", "liberty", "aerofit"]),
        ("QCY", &["qcy", "melobuds", "멜로버즈", "crossky"]),
        ("Shokz", &["shokz", "샥즈", "openfit", "openrun", "opendots", "골전도"]),
    ];

    let mut matched = Vec::new();
    for (canonical, aliases) in pairs {
        if aliases.iter().any(|a| q.contains(a)) {
            matched.push(canonical);
        }
    }
    matched
}

pub fn encode_embedding(vec: &[f32]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(vec.len() * 4);
    for &f in vec {
        bytes.extend_from_slice(&f.to_le_bytes());
    }
    bytes
}

pub fn decode_embedding(bytes: &[u8]) -> Vec<f32> {
    bytes
        .chunks_exact(4)
        .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
        .collect()
}

pub fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }
    let mut dot = 0.0_f32;
    let mut na = 0.0_f32;
    let mut nb = 0.0_f32;
    for (&x, &y) in a.iter().zip(b.iter()) {
        dot += x * y;
        na += x * x;
        nb += y * y;
    }
    let denom = na.sqrt() * nb.sqrt();
    if denom == 0.0 {
        0.0
    } else {
        dot / denom
    }
}

/// FTS5 MATCH 구문 오류 방지 + 한국어 조사 대응 전방 일치(\"토큰\"* OR ...)
pub fn sanitize_fts5_query(raw: &str) -> String {
    let tokens: Vec<String> = raw
        .split_whitespace()
        .map(|tok| {
            tok.chars()
                .filter(|c| c.is_alphanumeric() || ('가'..='힣').contains(c))
                .collect::<String>()
        })
        .filter(|t| !t.is_empty())
        .map(|t| format!("\"{}\"*", t))
        .collect();
    tokens.join(" OR ")
}
