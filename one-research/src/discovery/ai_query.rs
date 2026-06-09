use crate::config::Config;
use chat::provider::ChatProvider;
use serde::Deserialize;

/// Fallback single-shot plan — used when no Claude key is configured.
/// Asks the AI to return JSON with arXiv categories, IDs, and search terms.
#[derive(Debug, Clone, Default)]
pub struct DiscoveryPlan {
  pub arxiv_categories: Vec<String>,
  pub paper_ids: Vec<String>,
  pub search_terms: Vec<String>,
}

pub fn run_ai_query(
  topic: &str,
  config: &Config,
) -> Result<DiscoveryPlan, String> {
  let prompt = build_prompt(topic);
  let content = if let Some(key) =
    config.openai_api_key.as_ref().filter(|k| !k.trim().is_empty())
  {
    send_prompt(chat::OpenAiProvider::new(key), &prompt)?
  } else {
    return Err("No OpenAI API key configured".to_string());
  };

  parse_plan(&content)
}

fn build_prompt(topic: &str) -> String {
  format!(
    r#"You are a research discovery assistant for an AI/ML feed reader.
Topic: "{topic}"

Return ONLY valid JSON with this shape:
{{
  "topic": "{topic}",
  "arxiv_categories": ["cs.LG"],
  "paper_ids": ["2312.01234"],
  "search_terms": ["sparse autoencoder mechanistic interpretability"]
}}

Rules: at most 8 categories, 20 paper IDs, 5 search terms. No explanation."#
  )
}

fn send_prompt(
  provider: impl ChatProvider,
  prompt: &str,
) -> Result<String, String> {
  let messages = vec![chat::ChatMessage {
    role: chat::Role::User,
    content: prompt.to_string(),
    timestamp: chrono::Utc::now(),
  }];
  provider.send(&messages).map(|r| r.content).map_err(|e| e.to_string())
}

fn parse_plan(content: &str) -> Result<DiscoveryPlan, String> {
  let stripped = strip_json_fence(content);
  let raw: RawPlan = serde_json::from_str(stripped)
    .map_err(|e| format!("Discovery JSON parse failed: {e}"))?;

  let mut plan = DiscoveryPlan {
    arxiv_categories: raw
      .arxiv_categories
      .into_iter()
      .filter_map(|cat| normalize_arxiv_category(&cat))
      .take(8)
      .collect(),
    paper_ids: raw
      .paper_ids
      .into_iter()
      .filter_map(|id| normalize_arxiv_id(&id))
      .take(20)
      .collect(),
    search_terms: raw
      .search_terms
      .into_iter()
      .map(|s| s.trim().to_string())
      .filter(|s| !s.is_empty())
      .take(5)
      .collect(),
  };

  plan.arxiv_categories.sort();
  plan.arxiv_categories.dedup();
  plan.paper_ids.sort();
  plan.paper_ids.dedup();
  plan.search_terms.sort();
  plan.search_terms.dedup();

  Ok(plan)
}

// serde silently ignores unknown fields by default — no need to list them.
#[derive(Deserialize)]
struct RawPlan {
  #[serde(default)]
  arxiv_categories: Vec<String>,
  #[serde(default, alias = "arxiv_ids")]
  paper_ids: Vec<String>,
  #[serde(default)]
  search_terms: Vec<String>,
}

fn strip_json_fence(content: &str) -> &str {
  let trimmed = content.trim();
  if !trimmed.starts_with("```") {
    return trimmed;
  }
  let without_open = trimmed
    .strip_prefix("```json")
    .or_else(|| trimmed.strip_prefix("```"))
    .unwrap_or(trimmed)
    .trim_start();
  without_open.strip_suffix("```").unwrap_or(without_open).trim()
}

fn normalize_arxiv_category(value: &str) -> Option<String> {
  let cat = value.trim();
  if cat.len() > 20 || !cat.contains('.') {
    return None;
  }
  let valid =
    cat.chars().all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-');
  if valid { Some(cat.to_string()) } else { None }
}

pub fn normalize_arxiv_id(value: &str) -> Option<String> {
  let mut id = value.trim();
  for prefix in
    &["https://arxiv.org/abs/", "http://arxiv.org/abs/", "arxiv.org/abs/"]
  {
    if let Some(rest) = id.strip_prefix(prefix) {
      id = rest;
      break;
    }
  }
  id = id.split(['?', '#']).next().unwrap_or(id);
  if let Some(v_pos) = id.rfind('v') {
    let version = &id[v_pos + 1..];
    if !version.is_empty() && version.chars().all(|c| c.is_ascii_digit()) {
      id = &id[..v_pos];
    }
  }
  let valid = id.contains('.')
    && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-');
  if valid { Some(id.to_string()) } else { None }
}

pub fn is_http_url(url: &str) -> bool {
  url.starts_with("http://") || url.starts_with("https://")
}
