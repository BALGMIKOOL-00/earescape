PRAGMA foreign_keys = ON;

-- 1. TOP 10 브랜드 음향 기기 카탈로그
CREATE TABLE IF NOT EXISTS products (
    id                TEXT    PRIMARY KEY,
    brand             TEXT    NOT NULL,
    name              TEXT    NOT NULL,
    form_factor       TEXT    NOT NULL CHECK (form_factor IN ('tws_in_ear', 'tws_open_ear', 'bone_conduction', 'over_ear', 'wired_iem')),
    release_date      TEXT,
    release_policy    TEXT    NOT NULL CHECK (release_policy IN ('rolling_12_months', 'evergreen_popular')),
    price_usd         REAL,
    price_krw_est     INTEGER NOT NULL CHECK (price_krw_est >= 0),
    anc_support       INTEGER NOT NULL DEFAULT 0 CHECK (anc_support IN (0, 1)),
    battery_hours     TEXT,
    codecs            TEXT    NOT NULL DEFAULT '[]',
    water_resistance  TEXT    NOT NULL DEFAULT 'None',
    beginner_summary  TEXT    NOT NULL,
    source_url        TEXT    NOT NULL,
    last_verified_at  TEXT,
    updated_at        TEXT    NOT NULL DEFAULT (datetime('now'))
);

CREATE INDEX IF NOT EXISTS idx_products_filter
    ON products (brand, form_factor, anc_support, price_krw_est);

-- 2. 6축 사운드 및 사용성 프로필 (1~10점 척도)
CREATE TABLE IF NOT EXISTS sound_profiles (
    product_id      TEXT    PRIMARY KEY REFERENCES products(id) ON DELETE CASCADE,
    bass            INTEGER NOT NULL CHECK (bass BETWEEN 1 AND 10),
    mid             INTEGER NOT NULL CHECK (mid BETWEEN 1 AND 10),
    treble          INTEGER NOT NULL CHECK (treble BETWEEN 1 AND 10),
    soundstage      INTEGER NOT NULL CHECK (soundstage BETWEEN 1 AND 10),
    isolation_anc   INTEGER NOT NULL CHECK (isolation_anc BETWEEN 1 AND 10),
    comfort         INTEGER NOT NULL CHECK (comfort BETWEEN 1 AND 10),
    signature_label TEXT    NOT NULL
);

-- 3. RAG 검색용 지식 청크 + 벡터 임베딩 (f32 little-endian BLOB)
CREATE TABLE IF NOT EXISTS review_chunks (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    product_id      TEXT    NOT NULL REFERENCES products(id) ON DELETE CASCADE,
    chunk_type      TEXT    NOT NULL,
    content         TEXT    NOT NULL,
    source_url      TEXT    NOT NULL,
    embedding       BLOB,
    embedding_dim   INTEGER NOT NULL DEFAULT 0,
    embedding_model TEXT    NOT NULL DEFAULT 'local-ngram-v1',
    created_at      TEXT    NOT NULL DEFAULT (datetime('now'))
);

CREATE INDEX IF NOT EXISTS idx_review_chunks_product
    ON review_chunks (product_id);

-- 4. FTS5 전문 검색 가상 테이블 (External Content Table)
CREATE VIRTUAL TABLE IF NOT EXISTS review_chunks_fts USING fts5(
    content,
    content='review_chunks',
    content_rowid='id',
    tokenize='unicode61'
);

CREATE TRIGGER IF NOT EXISTS trg_review_chunks_ai AFTER INSERT ON review_chunks BEGIN
    INSERT INTO review_chunks_fts(rowid, content)
    VALUES (new.id, new.content);
END;

CREATE TRIGGER IF NOT EXISTS trg_review_chunks_ad AFTER DELETE ON review_chunks BEGIN
    INSERT INTO review_chunks_fts(review_chunks_fts, rowid, content)
    VALUES ('delete', old.id, old.content);
END;

CREATE TRIGGER IF NOT EXISTS trg_review_chunks_au AFTER UPDATE ON review_chunks BEGIN
    INSERT INTO review_chunks_fts(review_chunks_fts, rowid, content)
    VALUES ('delete', old.id, old.content);
    INSERT INTO review_chunks_fts(rowid, content)
    VALUES (new.id, new.content);
END;

-- 5. 입문자용 음향 용어 사전
CREATE TABLE IF NOT EXISTS glossary_terms (
    slug            TEXT PRIMARY KEY,
    term_ko         TEXT NOT NULL,
    term_en         TEXT NOT NULL,
    short_analogy   TEXT NOT NULL,
    detail_markdown TEXT NOT NULL
);

-- 6. 온디맨드 [데이터 수집] 실행 로그
CREATE TABLE IF NOT EXISTS collection_logs (
    id                 INTEGER PRIMARY KEY AUTOINCREMENT,
    run_id             TEXT    NOT NULL,
    triggered_at       TEXT    NOT NULL DEFAULT (datetime('now')),
    total_checked      INTEGER NOT NULL DEFAULT 0,
    live_ok_count      INTEGER NOT NULL DEFAULT 0,
    fallback_count     INTEGER NOT NULL DEFAULT 0,
    filtered_out_count INTEGER NOT NULL DEFAULT 0,
    status             TEXT    NOT NULL CHECK (status IN ('success', 'partial', 'fallback')),
    summary_message    TEXT    NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_collection_logs_recent
    ON collection_logs (id DESC);
