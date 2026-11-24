use chrono::{Duration, NaiveDate};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

pub const TOP10_BRANDS: [&str; 10] = [
    "Apple",
    "Samsung",
    "Sony",
    "Bose",
    "Sennheiser",
    "Shure",
    "Westone Audio",
    "Anker",
    "QCY",
    "Shokz",
];

pub const EVERGREEN_BRANDS: [&str; 2] = ["Shure", "Westone Audio"];
pub const TARGET_PRODUCTS_PER_BRAND: usize = 5;
pub const TARGET_TOTAL_PRODUCTS: usize = TOP10_BRANDS.len() * TARGET_PRODUCTS_PER_BRAND;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FormFactor {
    TwsInEar,
    TwsOpenEar,
    BoneConduction,
    OverEar,
    WiredIem,
}

impl FormFactor {
    pub fn as_str(&self) -> &'static str {
        match self {
            FormFactor::TwsInEar => "tws_in_ear",
            FormFactor::TwsOpenEar => "tws_open_ear",
            FormFactor::BoneConduction => "bone_conduction",
            FormFactor::OverEar => "over_ear",
            FormFactor::WiredIem => "wired_iem",
        }
    }

    pub fn label_ko(&self) -> &'static str {
        match self {
            FormFactor::TwsInEar => "커널형 무선(TWS)",
            FormFactor::TwsOpenEar => "오픈형 무선",
            FormFactor::BoneConduction => "골전도 스포츠",
            FormFactor::OverEar => "오버이어 헤드폰",
            FormFactor::WiredIem => "유선 인이어(IEM)",
        }
    }

    pub fn from_str_opt(s: &str) -> Option<Self> {
        match s.trim() {
            "tws_in_ear" => Some(FormFactor::TwsInEar),
            "tws_open_ear" => Some(FormFactor::TwsOpenEar),
            "bone_conduction" => Some(FormFactor::BoneConduction),
            "over_ear" => Some(FormFactor::OverEar),
            "wired_iem" => Some(FormFactor::WiredIem),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReleasePolicy {
    #[serde(rename = "rolling_12_months")]
    Rolling12Months,
    EvergreenPopular,
}

impl ReleasePolicy {
    pub fn as_str(&self) -> &'static str {
        match self {
            ReleasePolicy::Rolling12Months => "rolling_12_months",
            ReleasePolicy::EvergreenPopular => "evergreen_popular",
        }
    }

    pub fn label_ko(&self) -> &'static str {
        match self {
            ReleasePolicy::Rolling12Months => "최근 1년 신제품",
            ReleasePolicy::EvergreenPopular => "인기 스테디셀러",
        }
    }

    pub fn from_db(s: &str) -> Self {
        match s {
            "evergreen_popular" => ReleasePolicy::EvergreenPopular,
            _ => ReleasePolicy::Rolling12Months,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SoundProfile {
    pub bass: u8,
    pub mid: u8,
    pub treble: u8,
    pub soundstage: u8,
    pub isolation_anc: u8,
    pub comfort: u8,
    pub signature_label: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SeedReviewChunk {
    pub chunk_type: String,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SeedProduct {
    pub id: String,
    pub brand: String,
    pub name: String,
    pub form_factor: FormFactor,
    pub release_date: Option<String>,
    pub release_policy: ReleasePolicy,
    pub price_usd: Option<f64>,
    pub price_krw_est: i64,
    pub anc_support: bool,
    pub battery_hours: Option<String>,
    pub codecs: Vec<String>,
    pub water_resistance: String,
    pub beginner_summary: String,
    pub source_url: String,
    pub sound_profile: SoundProfile,
    pub review_chunks: Vec<SeedReviewChunk>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Product {
    pub id: String,
    pub brand: String,
    pub name: String,
    pub form_factor: FormFactor,
    pub release_date: Option<String>,
    pub release_policy: ReleasePolicy,
    pub price_usd: Option<f64>,
    pub price_krw_est: i64,
    pub anc_support: bool,
    pub battery_hours: Option<String>,
    pub codecs: Vec<String>,
    pub water_resistance: String,
    pub beginner_summary: String,
    pub source_url: String,
    pub last_verified_at: Option<String>,
    pub sound_profile: SoundProfile,
}

impl Product {
    pub fn price_manwon(&self) -> i64 {
        (self.price_krw_est + 5_000) / 10_000
    }

    pub fn form_factor_label(&self) -> &'static str {
        self.form_factor.label_ko()
    }

    pub fn policy_label(&self) -> &'static str {
        self.release_policy.label_ko()
    }

    pub fn is_evergreen(&self) -> bool {
        self.release_policy == ReleasePolicy::EvergreenPopular
    }

    pub fn codecs_joined(&self) -> String {
        self.codecs.join(", ")
    }

    pub fn battery_display(&self) -> String {
        self.battery_hours
            .clone()
            .unwrap_or_else(|| "유선 (충전 불필요)".to_string())
    }

    pub fn release_display(&self) -> String {
        if self.is_evergreen() {
            "인기 스테디셀러".to_string()
        } else {
            self.release_date
                .clone()
                .unwrap_or_else(|| "최근 1년 이내".to_string())
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GlossaryTerm {
    pub slug: String,
    pub term_ko: String,
    pub term_en: String,
    pub short_analogy: String,
    pub detail_markdown: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct ProductFilter {
    pub brand: Option<String>,
    pub form_factor: Option<String>,
    pub policy: Option<String>,
    pub anc_only: Option<bool>,
    pub max_budget_manwon: Option<i64>,
    pub sort: Option<String>,
}

/// TOP 10 브랜드 화이트리스트 및 1년 출시일 정책 검증
pub fn is_top10_brand(brand: &str) -> bool {
    TOP10_BRANDS
        .iter()
        .any(|b| b.eq_ignore_ascii_case(brand.trim()))
}

pub fn is_evergreen_brand(brand: &str) -> bool {
    EVERGREEN_BRANDS
        .iter()
        .any(|b| b.eq_ignore_ascii_case(brand.trim()))
}

/// 일반 8개사는 reference_date 기준 365일 이내(포함) 출시 제품만 통과.
/// Shure 및 Westone Audio는 출시일과 무관하게 통과.
pub fn passes_release_policy(
    brand: &str,
    release_date: Option<&str>,
    reference_date: NaiveDate,
) -> bool {
    if !is_top10_brand(brand) {
        return false;
    }
    if is_evergreen_brand(brand) {
        return true;
    }

    let Some(date_str) = release_date else {
        return false;
    };
    let Ok(parsed) = NaiveDate::parse_from_str(date_str.trim(), "%Y-%m-%d") else {
        return false;
    };

    let cutoff = reference_date - Duration::days(365);
    parsed >= cutoff && parsed <= reference_date
}

/// 카탈로그 쿼터 검증: TOP 10 브랜드 각 5개씩 총 50개 제품 및 정책 일치 확인
pub fn validate_catalog_quota(
    products: &[Product],
    reference_date: NaiveDate,
) -> Result<(), String> {
    if products.len() != TARGET_TOTAL_PRODUCTS {
        return Err(format!(
            "총 제품 수 불일치: 기대 {}개, 실제 {}개",
            TARGET_TOTAL_PRODUCTS,
            products.len()
        ));
    }

    let mut counts: HashMap<&str, usize> = HashMap::new();
    for p in products {
        if !passes_release_policy(&p.brand, p.release_date.as_deref(), reference_date) {
            return Err(format!("정책 위반 제품 발견: {} ({})", p.id, p.brand));
        }
        let expected_policy = if is_evergreen_brand(&p.brand) {
            ReleasePolicy::EvergreenPopular
        } else {
            ReleasePolicy::Rolling12Months
        };
        if p.release_policy != expected_policy {
            return Err(format!(
                "정책 라벨 불일치: {} ({:?} != {:?})",
                p.id, p.release_policy, expected_policy
            ));
        }
        let canonical = TOP10_BRANDS
            .iter()
            .copied()
            .find(|b| b.eq_ignore_ascii_case(&p.brand))
            .ok_or_else(|| format!("TOP 10 외 브랜드: {}", p.brand))?;
        *counts.entry(canonical).or_insert(0) += 1;
    }

    for b in TOP10_BRANDS {
        let c = counts.get(b).copied().unwrap_or(0);
        if c != TARGET_PRODUCTS_PER_BRAND {
            return Err(format!(
                "브랜드별 제품 수 불일치 ({}): 기대 {}개, 실제 {}개",
                b, TARGET_PRODUCTS_PER_BRAND, c
            ));
        }
    }

    Ok(())
}
