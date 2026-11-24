use crate::domain::product::{FormFactor, Product};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AncPreference {
    Required,
    Preferred,
    Any,
}

#[derive(Debug, Clone, Deserialize)]
pub struct QuizInput {
    pub bass: u8,              // 1..=5
    pub mid: u8,               // 1..=5
    pub treble: u8,            // 1..=5
    pub anc: AncPreference,    // required | preferred | any
    pub form_factor: String,   // any | tws_in_ear | tws_open_ear | bone_conduction | over_ear | wired_iem
    pub budget_manwon: i64,    // 만원 단위 (예: 35 -> 350,000원)
    pub use_case: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ScoredProduct {
    pub rank: usize,
    pub product: Product,
    pub match_percent: u8,
    pub euclidean_distance: f32,
    pub match_reasons: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct QuizRecommendationResult {
    pub total_candidates: usize,
    pub filtered_candidates: usize,
    pub relaxed_fallback: bool,
    pub fallback_notice: String,
    pub top3: Vec<ScoredProduct>,
}

/// 1) 하드 필터 적용: 예산(budget_manwon * 10_000), 착용 방식(any 제외), ANC 필수 여부
/// 2) 1~5점 입력을 10점 척도(*2)로 정규화 후 SoundProfile(bass, mid, treble)과 유클리드 거리 계산
/// 3) 결과가 0건일 경우 조건을 단계적으로 완화(예산 +25% 또는 착용방식 완화)해 빈 화면 방지
pub fn evaluate_quiz(products: &[Product], input: &QuizInput) -> QuizRecommendationResult {
    let total_candidates = products.len();
    let max_budget_krw = (input.budget_manwon.max(5)) * 10_000;
    let target_ff = if input.form_factor == "any" || input.form_factor.trim().is_empty() {
        None
    } else {
        FormFactor::from_str_opt(&input.form_factor)
    };

    let mut filtered: Vec<&Product> = products
        .iter()
        .filter(|p| p.price_krw_est <= max_budget_krw)
        .filter(|p| match target_ff {
            Some(ff) => p.form_factor == ff,
            None => true,
        })
        .filter(|p| match input.anc {
            AncPreference::Required => p.anc_support,
            AncPreference::Preferred | AncPreference::Any => true,
        })
        .collect();

    let mut relaxed_fallback = false;
    let mut fallback_notice = String::new();

    // 조건이 너무 엄격해 결과가 없을 때 단계적 완화
    if filtered.is_empty() {
        relaxed_fallback = true;
        let expanded_budget = (max_budget_krw as f64 * 1.25) as i64;
        filtered = products
            .iter()
            .filter(|p| p.price_krw_est <= expanded_budget)
            .filter(|p| match input.anc {
                AncPreference::Required => p.anc_support,
                AncPreference::Preferred | AncPreference::Any => true,
            })
            .collect();

        if !filtered.is_empty() {
            fallback_notice = format!(
                "선택하신 조건에 정확히 일치하는 제품이 없어 예산을 약 {}만 원까지 넓히고 착용 방식을 완화해 가장 가까운 대안을 찾았습니다.",
                expanded_budget / 10_000
            );
        } else {
            filtered = products.iter().collect();
            fallback_notice =
                "선택하신 조건에 정확히 일치하는 제품이 없어 전체 TOP 10 라인업 중 소리 성향이 가장 가까운 제품을 추천합니다."
                    .to_string();
        }
    }

    let filtered_candidates = filtered.len();

    let target_b = (input.bass.clamp(1, 5) * 2) as f32;
    let target_m = (input.mid.clamp(1, 5) * 2) as f32;
    let target_t = (input.treble.clamp(1, 5) * 2) as f32;
    // 최대 가능 거리: 각 축 최대 차 9 -> sqrt(9^2 * 3) ≈ 15.588
    let max_dist = (81.0_f32 * 3.0).sqrt();

    let mut scored: Vec<ScoredProduct> = filtered
        .into_iter()
        .map(|p| {
            let db = target_b - p.sound_profile.bass as f32;
            let dm = target_m - p.sound_profile.mid as f32;
            let dt = target_t - p.sound_profile.treble as f32;
            let dist = (db * db + dm * dm + dt * dt).sqrt();

            let mut score = ((1.0 - (dist / max_dist)) * 100.0).clamp(0.0, 100.0);

            // ANC 선호 시 보너스
            if input.anc == AncPreference::Preferred && p.anc_support {
                score = (score + 5.0).min(100.0);
            }

            // 사용 환경 보너스
            if let Some(uc) = input.use_case.as_deref() {
                match uc {
                    "commute" if p.sound_profile.isolation_anc >= 8 => {
                        score = (score + 4.0).min(100.0);
                    }
                    "workout"
                        if matches!(
                            p.form_factor,
                            FormFactor::TwsOpenEar | FormFactor::BoneConduction
                        ) || p.water_resistance.contains("55")
                            || p.water_resistance.contains("57")
                            || p.water_resistance.contains("67")
                            || p.water_resistance.contains("X5")
                            || p.water_resistance.contains("X6") =>
                    {
                        score = (score + 5.0).min(100.0);
                    }
                    "studio" if p.form_factor == FormFactor::WiredIem || p.sound_profile.mid >= 9 =>
                    {
                        score = (score + 4.0).min(100.0);
                    }
                    _ => {}
                }
            }

            let mut reasons = Vec::new();
            reasons.push(format!(
                "사운드 성향: {} (저음 {} · 중음 {} · 고음 {})",
                p.sound_profile.signature_label,
                p.sound_profile.bass,
                p.sound_profile.mid,
                p.sound_profile.treble
            ));
            if p.anc_support {
                reasons.push(format!(
                    "ANC 지원 (차음력 {}/10)",
                    p.sound_profile.isolation_anc
                ));
            } else if p.form_factor == FormFactor::WiredIem {
                reasons.push(format!(
                    "강력한 패시브 차음 ({}/10) · 유선 원음",
                    p.sound_profile.isolation_anc
                ));
            } else {
                reasons.push(format!(
                    "귀 압박 없는 개방형 착용감 ({}/10)",
                    p.sound_profile.comfort
                ));
            }
            if p.price_krw_est <= max_budget_krw {
                reasons.push(format!("예산 이내 (약 {}만 원대)", p.price_manwon()));
            } else {
                reasons.push(format!("예산 근접 대안 (약 {}만 원대)", p.price_manwon()));
            }

            ScoredProduct {
                rank: 0,
                product: p.clone(),
                match_percent: score.round() as u8,
                euclidean_distance: (dist * 100.0).round() / 100.0,
                match_reasons: reasons,
            }
        })
        .collect();

    scored.sort_by(|a, b| {
        b.match_percent
            .cmp(&a.match_percent)
            .then_with(|| {
                b.product
                    .sound_profile
                    .isolation_anc
                    .cmp(&a.product.sound_profile.isolation_anc)
            })
            .then_with(|| a.product.price_krw_est.cmp(&b.product.price_krw_est))
    });

    let top3: Vec<ScoredProduct> = scored
        .into_iter()
        .take(3)
        .enumerate()
        .map(|(i, mut item)| {
            item.rank = i + 1;
            item
        })
        .collect();

    QuizRecommendationResult {
        total_candidates,
        filtered_candidates,
        relaxed_fallback,
        fallback_notice,
        top3,
    }
}
