use anyhow::{Result, anyhow};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::provider::{ChatProvider, ProviderResponse};
use crate::{ChatMessage, Role};

const MAX_RESPONSE_BYTES: u64 = 4 * 1024 * 1024;
const DEFAULT_MODEL: &str = "claude-sonnet-4-6";

fn read_body(resp: reqwest::blocking::Response) -> anyhow::Result<String> {
  use std::io::Read;
  let mut buf = Vec::new();
  resp
    .take(MAX_RESPONSE_BYTES + 1)
    .read_to_end(&mut buf)
    .map_err(|e| anyhow::anyhow!("body read error: {e}"))?;
  if buf.len() as u64 > MAX_RESPONSE_BYTES {
    anyhow::bail!("response body exceeds 4 MB limit");
  }
  String::from_utf8(buf).map_err(|e| anyhow::anyhow!("body encoding: {e}"))
}

pub struct ClaudeProvider {
  pub api_key: String,
  pub model: String,
  client: reqwest::blocking::Client,
}

impl ClaudeProvider {
  pub fn new(api_key: impl Into<String>) -> Self {
    Self {
      api_key: api_key.into(),
      model: DEFAULT_MODEL.to_string(),
      // Share the workspace-hardened client (redirect cap, UA, timeout).
      // Client::clone is an Arc bump, not a connection-pool rebuild.
      client: one_research_http::client().clone(),
    }
  }

  pub fn with_model(
    api_key: impl Into<String>,
    model: impl Into<String>,
  ) -> Self {
    Self {
      api_key: api_key.into(),
      model: model.into(),
      client: one_research_http::client().clone(),
    }
  }
}

#[derive(Deserialize)]
struct ContentBlock {
  text: String,
}

#[derive(Deserialize)]
struct Usage {
  input_tokens: u64,
  output_tokens: u64,
}

#[derive(Deserialize)]
struct ClaudeResponse {
  content: Vec<ContentBlock>,
  #[serde(default)]
  usage: Option<Usage>,
}

impl ChatProvider for ClaudeProvider {
  fn name(&self) -> &str {
    "claude"
  }
  fn model(&self) -> &str {
    &self.model
  }
  fn context_window(&self) -> u64 {
    200_000
  }

  fn send(&self, messages: &[ChatMessage]) -> Result<ProviderResponse> {
    let system_text: String = messages
      .iter()
      .filter(|m| matches!(m.role, Role::System))
      .map(|m| m.content.as_str())
      .collect::<Vec<_>>()
      .join("\n\n");

    let api_messages: Vec<Value> = messages
      .iter()
      .filter(|m| !matches!(m.role, Role::System))
      .map(|m| {
        let role = match m.role {
          Role::User => "user",
          Role::Assistant => "assistant",
          Role::System => {
            panic!("System role should be filtered out before this map")
          }
        };
        json!({ "role": role, "content": m.content })
      })
      .collect();

    let mut body = json!({
        "model": self.model,
        "max_tokens": 4096,
        "messages": api_messages,
    });

    if !system_text.is_empty() {
      body["system"] = json!(system_text);
    }

    let resp = self
      .client
      .post("https://api.anthropic.com/v1/messages")
      .header("x-api-key", &self.api_key)
      .header("anthropic-version", "2023-06-01")
      .header("content-type", "application/json")
      .json(&body)
      .send()?;

    let status = resp.status();
    let text = read_body(resp)?;

    if !status.is_success() {
      let friendly = friendly_error(status.as_u16(), &text);
      return Err(anyhow!("{friendly}"));
    }

    let parsed: ClaudeResponse = serde_json::from_str(&text).map_err(|e| {
      anyhow!("Failed to parse Claude response: {e}\nBody: {text}")
    })?;

    let content = parsed
      .content
      .into_iter()
      .next()
      .map(|b| b.text)
      .ok_or_else(|| anyhow!("Claude response had no content blocks"))?;

    let (input_tokens, output_tokens) =
      parsed.usage.map(|u| (u.input_tokens, u.output_tokens)).unwrap_or((0, 0));

    Ok(ProviderResponse { content, input_tokens, output_tokens })
  }
}

fn friendly_error(status: u16, body: &str) -> String {
  if let Ok(v) = serde_json::from_str::<Value>(body) {
    if let Some(err_type) = v["error"]["type"].as_str() {
      return match err_type {
        "authentication_error" => {
          "invalid API key — check settings".to_string()
        }
        "rate_limit_error" => {
          "rate limit exceeded — try again shortly".to_string()
        }
        t if t.contains("quota") => {
          "quota exceeded — check billing".to_string()
        }
        _ => {
          let msg = v["error"]["message"].as_str().unwrap_or("unknown error");
          // Char-aware truncation guards the byte boundary; sanitize then
          // strips any terminal-escape bytes the upstream API embedded in
          // the error message before it reaches log lines or the chat
          // surface.
          let short = crate::sanitize::truncate_chars(msg, 80);
          let short = crate::sanitize::sanitize_terminal_text(&short);
          format!("API error — {short}")
        }
      };
    }
  }
  format!("API error {status}")
}
