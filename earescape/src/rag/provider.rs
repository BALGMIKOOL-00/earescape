use async_trait::async_trait;
use serde_json::json;
use std::time::Duration;

#[async_trait]
pub trait EmbeddingProvider: Send + Sync {
    fn model_name(&self) -> &str;
    async fn embed(&self, text: &str) -> anyhow::Result<Vec<f32>>;
}

#[async_trait]
pub trait LlmProvider: Send + Sync {
    fn provider_name(&self) -> &str;
    async fn generate(&self, system_prompt: &str, user_prompt: &str) -> anyhow::Result<String>;
}

/// API 키 없이도 100% 동작하는 결정론적 한국어 2-gram 해싱 임베딩 (128차원 L2 정규화)
pub struct FallbackMockEmbeddingProvider;

impl FallbackMockEmbeddingProvider {
    pub fn embed_sync(text: &str) -> Vec<f32> {
        const DIM: usize = 128;
        let mut vec = vec![0.0_f32; DIM];
        let normalized: String = text
            .to_lowercase()
            .chars()
            .filter(|c| c.is_alphanumeric() || ('가'..='힣').contains(c))
            .collect();
        let chars: Vec<char> = normalized.chars().collect();

        if chars.len() < 2 {
            if let Some(&c) = chars.first() {
                vec[(c as usize) % DIM] += 1.0;
            }
        } else {
            for w in chars.windows(2) {
                let mut h: u64 = 14695981039346656037;
                for &ch in w {
                    h ^= ch as u64;
                    h = h.wrapping_mul(1099511628211);
                }
                let idx = (h as usize) % DIM;
                vec[idx] += 1.0;
            }
        }

        let norm = vec.iter().map(|x| x * x).sum::<f32>().sqrt();
        if norm > 0.0 {
            for x in &mut vec {
                *x /= norm;
            }
        }
        vec
    }
}

#[async_trait]
impl EmbeddingProvider for FallbackMockEmbeddingProvider {
    fn model_name(&self) -> &str {
        "local-ngram-128d"
    }

    async fn embed(&self, text: &str) -> anyhow::Result<Vec<f32>> {
        Ok(Self::embed_sync(text))
    }
}

/// API 키 미설정 또는 외부 네트워크 차단 시 동작하는 한국어 에디토리얼 폴백 LLM
pub struct FallbackMockLlmProvider;

#[async_trait]
impl LlmProvider for FallbackMockLlmProvider {
    fn provider_name(&self) -> &str {
        "오프라인 로컬 RAG 엔진 (Fallback)"
    }

    async fn generate(&self, _system_prompt: &str, user_prompt: &str) -> anyhow::Result<String> {
        let mut lines = Vec::new();
        lines.push("TOP 10 브랜드 데이터베이스에서 질문과 가장 밀접한 근거를 찾아 정리해 드립니다.".to_string());
        lines.push(String::new());

        let mut found_context = false;
        for line in user_prompt.lines() {
            if line.starts_with("- [") {
                found_context = true;
                lines.push(line.to_string());
            }
        }

        if !found_context {
            lines.push("현재 기준(일반 8개사 최근 1년 신제품 + 슈어·웨스톤 스테디셀러) 내에서 직접 일치하는 항목이 적어, 카탈로그나 취향 진단을 통해 조건을 좁혀 보시는 것을 추천합니다.".to_string());
        } else {
            lines.push(String::new());
            lines.push("💡 입문자 팁: 오른쪽 인용 카드에서 [비교함에 담기]를 누르면 최대 4개 제품의 저음·중음·고음·ANC 차이를 레이더 차트로 한눈에 비교할 수 있어요.".to_string());
        }

        Ok(lines.join("\n"))
    }
}

/// Gemini API 임베딩 프로바이더
pub struct GeminiEmbeddingProvider {
    http: reqwest::Client,
    api_key: String,
    model: String,
}

impl GeminiEmbeddingProvider {
    pub fn new(api_key: String, model: String) -> anyhow::Result<Self> {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(15))
            .build()?;
        Ok(Self {
            http,
            api_key,
            model,
        })
    }
}

#[async_trait]
impl EmbeddingProvider for GeminiEmbeddingProvider {
    fn model_name(&self) -> &str {
        &self.model
    }

    async fn embed(&self, text: &str) -> anyhow::Result<Vec<f32>> {
        let url = format!(
            "https://generativelanguage.googleapis.com/v1beta/models/{}:embedContent?key={}",
            self.model, self.api_key
        );
        let body = json!({
            "content": {
                "parts": [{ "text": text }]
            }
        });
        let resp = self.http.post(&url).json(&body).send().await?;
        if !resp.status().is_success() {
            // 외부 호출 실패 시 로컬 임베딩 폴백
            return Ok(FallbackMockEmbeddingProvider::embed_sync(text));
        }
        let val: serde_json::Value = resp.json().await?;
        let arr = val["embedding"]["values"]
            .as_array()
            .ok_or_else(|| anyhow::anyhow!("Gemini 임베딩 응답 파싱 실패"))?;
        let vec: Vec<f32> = arr
            .iter()
            .filter_map(|v| v.as_f64().map(|f| f as f32))
            .collect();
        if vec.is_empty() {
            Ok(FallbackMockEmbeddingProvider::embed_sync(text))
        } else {
            Ok(vec)
        }
    }
}

/// Gemini API 텍스트 생성 프로바이더
pub struct GeminiLlmProvider {
    http: reqwest::Client,
    api_key: String,
    model: String,
}

impl GeminiLlmProvider {
    pub fn new(api_key: String, model: String) -> anyhow::Result<Self> {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(25))
            .build()?;
        Ok(Self {
            http,
            api_key,
            model,
        })
    }
}

#[async_trait]
impl LlmProvider for GeminiLlmProvider {
    fn provider_name(&self) -> &str {
        "Gemini API (Live RAG)"
    }

    async fn generate(&self, system_prompt: &str, user_prompt: &str) -> anyhow::Result<String> {
        let url = format!(
            "https://generativelanguage.googleapis.com/v1beta/models/{}:generateContent?key={}",
            self.model, self.api_key
        );
        let body = json!({
            "system_instruction": {
                "parts": [{ "text": system_prompt }]
            },
            "contents": [{
                "role": "user",
                "parts": [{ "text": user_prompt }]
            }],
            "generationConfig": {
                "temperature": 0.3
            }
        });
        let resp = self.http.post(&url).json(&body).send().await?;
        if !resp.status().is_success() {
            return FallbackMockLlmProvider.generate(system_prompt, user_prompt).await;
        }
        let val: serde_json::Value = resp.json().await?;
        let text = val["candidates"][0]["content"]["parts"][0]["text"]
            .as_str()
            .map(|s| s.to_string());
        match text {
            Some(t) if !t.trim().is_empty() => Ok(t),
            _ => FallbackMockLlmProvider.generate(system_prompt, user_prompt).await,
        }
    }
}
