use anyhow::{Result, anyhow};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::provider::{ChatProvider, ProviderResponse};
use crate::{ChatMessage, Role};

const MAX_RESPONSE_BYTES: u64 = 4 * 1024 * 1024;

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

pub struct OpenAiProvider {
  pub api_key: String,
  pub model: String,
  client: reqwest::blocking::Client,
}

impl OpenAiProvider {
  pub fn new(api_key: impl Into<String>) -> Self {
    Self {
      api_key: api_key.into(),
      model: "gpt-4o".to_string(),
      // Share the workspace-hardened client (redirect cap, UA, timeout).
      // Client::clone is an Arc bump, not a connection-pool rebuild.
      client: trench_http::client().clone(),
    }
  }

  pub fn with_model(
    api_key: impl Into<String>,
    model: impl Into<String>,
  ) -> Self {
    Self {
      api_key: api_key.into(),
      model: model.into(),
      client: trench_http::client().clone(),
    }
  }
}

#[derive(Deserialize)]
struct Message {
  content: String,
}

#[derive(Deserialize)]
struct Choice {
  message: Message,
}

#[derive(Deserialize, Default)]
struct Usage {
  #[serde(default)]
  prompt_tokens: u64,
  #[serde(default)]
  completion_tokens: u64,
}

#[derive(Deserialize)]
struct OpenAiResponse {
  choices: Vec<Choice>,
  #[serde(default)]
  usage: Option<Usage>,
}

impl ChatProvider for OpenAiProvider {
  fn name(&self) -> &str {
    "openai"
  }
  fn model(&self) -> &str {
    &self.model
  }
  fn context_window(&self) -> u64 {
    128_000
  }

  fn send(&self, messages: &[ChatMessage]) -> Result<ProviderResponse> {
    let api_messages: Vec<Value> = messages
      .iter()
      .map(|m| {
        let role = match m.role {
          Role::System => "system",
          Role::User => "user",
          Role::Assistant => "assistant",
        };
        json!({ "role": role, "content": m.content })
      })
      .collect();

    let body = json!({
        "model": self.model,
        "messages": api_messages,
    });

    let resp = self
      .client
      .post("https://api.openai.com/v1/chat/completions")
      .header("Authorization", format!("Bearer {}", self.api_key))
      .header("content-type", "application/json")
      .json(&body)
      .send()?;

    let status = resp.status();
    let text = read_body(resp)?;

    if !status.is_success() {
      let friendly = friendly_error(status.as_u16(), &text);
      return Err(anyhow!("{friendly}"));
    }

    let parsed: OpenAiResponse = serde_json::from_str(&text).map_err(|e| {
      anyhow!("Failed to parse OpenAI response: {e}\nBody: {text}")
    })?;

    let content = parsed
      .choices
      .into_iter()
      .next()
      .map(|c| c.message.content)
      .ok_or_else(|| anyhow!("OpenAI response had no choices"))?;

    let (input_tokens, output_tokens) = parsed
      .usage
      .map(|u| (u.prompt_tokens, u.completion_tokens))
      .unwrap_or((0, 0));

    Ok(ProviderResponse { content, input_tokens, output_tokens })
  }
}

fn friendly_error(status: u16, body: &str) -> String {
  if let Ok(v) = serde_json::from_str::<Value>(body) {
    if let Some(err_type) = v["error"]["type"].as_str() {
      return match err_type {
        "invalid_api_key" | "invalid_request_error" if status == 401 => {
          "invalid API key — check settings".to_string()
        }
        t if t.contains("rate_limit") => {
          "rate limit exceeded — try again shortly".to_string()
        }
        t if t.contains("quota") || t.contains("billing") => {
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
