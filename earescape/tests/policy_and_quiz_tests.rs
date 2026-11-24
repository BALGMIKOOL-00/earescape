use chrono::NaiveDate;
use ear_escape::domain::product::{
    is_evergreen_brand, is_top10_brand, passes_release_policy, FormFactor, Product, ReleasePolicy,
    SoundProfile, TOP10_BRANDS,
};
use ear_escape::domain::quiz::{evaluate_quiz, AncPreference, QuizInput};

fn ref_date() -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, 11, 23).unwrap()
}

#[test]
fn test_top10_brand_whitelist_exact_ten() {
    assert_eq!(TOP10_BRANDS.len(), 10);
    for b in TOP10_BRANDS {
        assert!(is_top10_brand(b), "{} should be in TOP10", b);
    }
    assert!(!is_top10_brand("JBL"));
    assert!(!is_top10_brand("Beats"));
}

#[test]
fn test_rolling_12_months_boundary_365_vs_366_days() {
    let reference = ref_date();
    // 정확히 365일 전: 2025-11-23 -> 통과
    assert!(passes_release_policy("Sony", Some("2025-11-23"), reference));
    // 366일 전: 2025-11-22 -> 제외
    assert!(!passes_release_policy("Sony", Some("2025-11-22"), reference));
    // 기준일 당일: 2026-11-23 -> 통과
    assert!(passes_release_policy("Apple", Some("2026-11-23"), reference));
    // 출시일 누락 -> 일반 8개사는 제외
    assert!(!passes_release_policy("Bose", None, reference));
}

#[test]
fn test_shure_and_westone_evergreen_exception() {
    let reference = ref_date();
    assert!(is_evergreen_brand("Shure"));
    assert!(is_evergreen_brand("Westone Audio"));
    // 2011년 출시 슈어 SE215도 통과해야 함
    assert!(passes_release_policy("Shure", Some("2011-03-01"), reference));
    // 출시일 없어도 웨스톤 오디오는 통과
    assert!(passes_release_policy("Westone Audio", None, reference));
}

fn sample_products() -> Vec<Product> {
    vec![
        Product {
            id: "sony-wf-1000xm6".to_string(),
            brand: "Sony".to_string(),
            name: "WF-1000XM6".to_string(),
            form_factor: FormFactor::TwsInEar,
            release_date: Some("2026-02-12".to_string()),
            release_policy: ReleasePolicy::Rolling12Months,
            price_usd: Some(329.99),
            price_krw_est: 449_000,
            anc_support: true,
            battery_hours: Some("24시간".to_string()),
            codecs: vec!["LDAC".to_string()],
            water_resistance: "IPX4".to_string(),
            beginner_summary: "강력한 ANC와 풍성한 저음".to_string(),
            source_url: "https://example.com/sony".to_string(),
            last_verified_at: None,
            sound_profile: SoundProfile {
                bass: 9,
                mid: 8,
                treble: 8,
                soundstage: 8,
                isolation_anc: 10,
                comfort: 8,
                signature_label: "올라운더".to_string(),
            },
        },
        Product {
            id: "westone-pro-x10".to_string(),
            brand: "Westone Audio".to_string(),
            name: "Pro X10".to_string(),
            form_factor: FormFactor::WiredIem,
            release_date: Some("2021-05-18".to_string()),
            release_policy: ReleasePolicy::EvergreenPopular,
            price_usd: Some(249.99),
            price_krw_est: 349_000,
            anc_support: false,
            battery_hours: None,
            codecs: vec!["Wired".to_string()],
            water_resistance: "미지원".to_string(),
            beginner_summary: "보컬 중심 싱글 BA".to_string(),
            source_url: "https://example.com/westone".to_string(),
            last_verified_at: None,
            sound_profile: SoundProfile {
                bass: 6,
                mid: 10,
                treble: 8,
                soundstage: 7,
                isolation_anc: 9,
                comfort: 10,
                signature_label: "보컬 모니터".to_string(),
            },
        },
        Product {
            id: "qcy-h6-ultra".to_string(),
            brand: "QCY".to_string(),
            name: "QCY H6 Ultra".to_string(),
            form_factor: FormFactor::OverEar,
            release_date: Some("2026-08-15".to_string()),
            release_policy: ReleasePolicy::Rolling12Months,
            price_usd: Some(89.99),
            price_krw_est: 99_000,
            anc_support: true,
            battery_hours: Some("100시간".to_string()),
            codecs: vec!["LDAC".to_string()],
            water_resistance: "미지원".to_string(),
            beginner_summary: "초가성비 ANC 헤드폰".to_string(),
            source_url: "https://example.com/qcy".to_string(),
            last_verified_at: None,
            sound_profile: SoundProfile {
                bass: 8,
                mid: 7,
                treble: 7,
                soundstage: 7,
                isolation_anc: 8,
                comfort: 8,
                signature_label: "가성비 밸런스".to_string(),
            },
        },
    ]
}

#[test]
fn test_quiz_hard_filters_anc_and_budget() {
    let products = sample_products();
    let input = QuizInput {
        bass: 4,
        mid: 4,
        treble: 4,
        anc: AncPreference::Required,
        form_factor: "any".to_string(),
        budget_manwon: 15,
        use_case: Some("commute".to_string()),
    };
    let result = evaluate_quiz(&products, &input);
    assert!(!result.relaxed_fallback);
    assert_eq!(result.top3.len(), 1);
    assert_eq!(result.top3[0].product.id, "qcy-h6-ultra");
}

#[test]
fn test_quiz_sound_profile_matching_prefers_vocal_iem() {
    let products = sample_products();
    let input = QuizInput {
        bass: 3,
        mid: 5,
        treble: 4,
        anc: AncPreference::Any,
        form_factor: "wired_iem".to_string(),
        budget_manwon: 40,
        use_case: Some("studio".to_string()),
    };
    let result = evaluate_quiz(&products, &input);
    assert_eq!(result.top3[0].product.id, "westone-pro-x10");
    assert!(result.top3[0].match_percent >= 90);
}
