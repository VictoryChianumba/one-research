use std::io::Read;
use std::sync::mpsc::Sender;

use serde::Deserialize;
use serde_json::{Value, json};

use crate::config::Config;
use crate::discovery::intent::QueryIntent;
use crate::discovery::{DiscoveryMessage, tools};

const MAX_ITERATIONS: usize = 8;
const API_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(60);
const MAX_BODY_BYTES: u64 = 4 * 1024 * 1024;
const MODEL: &str = "claude-sonnet-4-6";

const SYSTEM_FIND: &str = "\
You are a research discovery agent for an AI/ML paper reader called Trench. \
Find the most relevant research papers for the user's query using the available tools.

Guidelines:
- For specific papers or authors: use fetch_arxiv_paper or search_arxiv with precise terms.
- For topic searches: call search_arxiv 2-3 times with different query angles.
- For recent context or news: use search_web if available.
- Aim for 5-25 relevant papers total. Stop when you have good coverage.
- After finding papers, write a concise 2-3 sentence summary of what you found.";

const SYSTEM_LIT_REVIEW: &str = "\
You are a research discovery agent for an AI/ML paper reader called Trench. \
Find papers and produce a structured literature review.

Guidelines:
- Call search_arxiv 3-4 times with different angles to get broad coverage.
- After finding papers, write a structured review using EXACTLY this format:

## Consensus
(2-3 sentences: what the papers broadly agree on)

## Active Debates
(2-3 sentences: areas of genuine disagreement or competing approaches)

## Open Questions
(2-3 sentences: unsolved problems the papers identify)";

const SYSTEM_SOTA: &str = "\
You are a research discovery agent for an AI/ML paper reader called Trench. \
Find papers reporting state-of-the-art results and summarise the competitive landscape.

Guidelines:
- Search for recent benchmarks, evaluations, and comparison papers.
- After finding papers, write a SOTA summary using EXACTLY this format:

## State of the Art: [topic]
(brief 1-sentence context)

For each top model/method found, one line: **Name** — key metric(s) — brief note
Example: **GPT-4o** — MMLU 87.2, HumanEval 90.2 — strong on reasoning and code

List at most 8 entries, strongest first.";

const SYSTEM_READING_LIST: &str = "\
You are a research discovery agent for an AI/ML paper reader called Trench. \
Find papers and organise them as a structured learning path.

Guidelines:
- Find foundational papers first, then intermediate, then advanced.
- After finding papers, write a numbered reading list using EXACTLY this format:

## Learning Path: [topic]

1. **Title** (Author, Year) — one sentence: why read this first
2. **Title** (Author, Year) — one sentence: what it builds on

Order from foundations to advanced. Include 5-12 papers.";

const SYSTEM_CODE: &str = "\
You are a research discovery agent for an AI/ML paper reader called Trench. \
Find papers with available implementations and summarise the code landscape.

Guidelines:
- Search arXiv for papers that explicitly mention code, GitHub, or open-source releases.
- After finding papers, write a code-focused summary using EXACTLY this format:

## Implementations: [topic]

For each paper with code: **Title** — GitHub: [link or 'not found'] — [framework, brief note]
Group by: Official implementations first, then third-party.";

const SYSTEM_COMPARE: &str = "\
You are a research discovery agent for an AI/ML paper reader called Trench. \
Find papers representing different approaches and produce a structured comparison.

Guidelines:
- Search for papers on each side of the comparison.
- After finding papers, write a comparison using EXACTLY this format:

## Comparison: [topic]

### [Approach A]
(2-3 sentences: key characteristics, strengths, weaknesses)

### [Approach B]
(2-3 sentences: key characteristics, strengths, weaknesses)

### Verdict
(1-2 sentences: when to use which, or which currently leads)";

const SYSTEM_DIGEST: &str = "\
You are a research discovery agent for an AI/ML paper reader called Trench. \
Find the most significant AI/ML developments from the past week.

Guidelines:
- Search for very recent papers across cs.LG, cs.AI, cs.CL.
- Use search_web if available to find announcements and news.
- After finding papers, write a digest using EXACTLY this format:

## This Week in AI/ML

### Highlights
(3-5 bullet points: the most significant papers or announcements)

### Papers to Watch
(3-5 bullet points: notable papers worth reading this week)";

const SYSTEM_AUTHOR: &str = "\
You are a research discovery agent for an AI/ML paper reader called Trench. \
Find all recent papers by the specified author or researcher.

Guidelines:
- Use search_arxiv with the author's name as the primary query.
- Try variations of the name if initial results are sparse.
- After finding papers, write a summary using EXACTLY this format:

## Papers by [Author Name]

(1-sentence context: their research focus or affiliation if known)

List each paper: **Title** (Year) — one sentence summary
Order by most recent first.";

const SYSTEM_TRENDING: &str = "\
You are a research discovery agent for an AI/ML paper reader called Trench. \
Find papers on the given topic that are currently getting significant attention.

Guidelines:
- Use search_arxiv with recency-focused queries to find papers gaining attention.
- Prioritise papers from the last 30 days.
- After finding papers, write a trending summary using EXACTLY this format:

## Trending: [topic]

(1-sentence context: why this topic is getting attention right now)

For each paper: **Title** — why it matters — [GitHub stars or engagement signal if known]
List at most 10, most significant first.";

fn system_for_intent(intent: QueryIntent) -> &'static str {
  match intent {
    QueryIntent::FindPapers => SYSTEM_FIND,
    QueryIntent::LiteratureReview => SYSTEM_LIT_REVIEW,
    QueryIntent::SotaLookup => SYSTEM_SOTA,
    QueryIntent::ReadingList => SYSTEM_READING_LIST,
    QueryIntent::CodeSearch => SYSTEM_CODE,
    QueryIntent::Compare => SYSTEM_COMPARE,
    QueryIntent::Digest => SYSTEM_DIGEST,
    QueryIntent::AuthorSearch => SYSTEM_AUTHOR,
    QueryIntent::Trending => SYSTEM_TRENDING,
  }
}

pub fn run(
  topic: &str,
  config: &Config,
  tx: &Sender<DiscoveryMessage>,
  prior_history: Option<Vec<Value>>,
  intent: QueryIntent,
) {
  if let Err(e) = run_inner(topic, config, tx, prior_history, intent) {
    let _ = tx.send(DiscoveryMessage::Error(e));
  }
}

fn run_inner(
  topic: &str,
  config: &Config,
  tx: &Sender<DiscoveryMessage>,
  prior_history: Option<Vec<Value>>,
  intent: QueryIntent,
) -> Result<(), String> {
  let api_key = config
    .claude_api_key
    .as_deref()
    .filter(|k| !k.trim().is_empty())
    .ok_or_else(|| "No Claude API key configured".to_string())?;

  let tool_defs = tools::all_tool_defs(config);
  let tools_json: Vec<Value> = tool_defs
    .iter()
    .map(|t| json!({ "name": t.name, "description": t.description, "input_schema": t.schema }))
    .collect();

  let client = reqwest::blocking::Client::builder()
    .timeout(API_TIMEOUT)
    .build()
    .map_err(|e| e.to_string())?;

  let mut messages: Vec<Value> = match prior_history {
    Some(mut h) => {
      h.push(json!({ "role": "user", "content": topic }));
      h
    }
    None => vec![json!({ "role": "user", "content": topic })],
  };

  let _ = tx.send(DiscoveryMessage::StatusUpdate(format!(
    "Starting discovery for '{topic}'…"
  )));

  let system = system_for_intent(intent);

  for step in 0..MAX_ITERATIONS {
    let response =
      call_claude(&client, api_key, system, &messages, &tools_json)?;

    // Collect tool_use blocks before moving content into messages.
    let tool_uses: Vec<Value> = response
      .content
      .iter()
      .filter(|b| b["type"] == "tool_use")
      .cloned()
      .collect();

    // Always append the assistant turn.
    messages.push(json!({ "role": "assistant", "content": response.content }));

    if tool_uses.is_empty() {
      // No tools called — agent is done.
      emit_snapshot(&messages, intent, tx);
      let _ = tx.send(DiscoveryMessage::Complete);
      return Ok(());
    }

    // Execute tools and collect results for the next user message.
    let mut tool_results = Vec::new();
    for tool_use in &tool_uses {
      let id = tool_use["id"].as_str().unwrap_or("").to_string();
      let name = tool_use["name"].as_str().unwrap_or("").to_string();
      let input = &tool_use["input"];

      let _ = tx.send(DiscoveryMessage::StatusUpdate(format!(
        "Calling {name}… (step {}/{})",
        step + 1,
        MAX_ITERATIONS
      )));

      let result = tools::execute(&name, input, config);

      if !result.items.is_empty() {
        let _ = tx.send(DiscoveryMessage::Items(result.items));
      }

      tool_results.push(json!({
        "type": "tool_result",
        "tool_use_id": id,
        "content": result.text
      }));
    }

    messages.push(json!({ "role": "user", "content": tool_results }));
  }

  emit_snapshot(&messages, intent, tx);
  let _ = tx.send(DiscoveryMessage::Complete);
  Ok(())
}

fn emit_snapshot(
  messages: &[Value],
  intent: QueryIntent,
  tx: &Sender<DiscoveryMessage>,
) {
  let initial_query = messages[0]["content"].as_str().unwrap_or("").to_string();
  let mut snapshot = crate::discovery::SessionHistory {
    messages: messages.to_vec(),
    initial_query,
    query_intent: intent,
  };
  snapshot.truncate_to_limit();
  let _ = tx.send(DiscoveryMessage::SessionSnapshot(snapshot));
}

#[derive(Deserialize)]
struct ClaudeResponse {
  content: Vec<Value>,
}

fn call_claude(
  client: &reqwest::blocking::Client,
  api_key: &str,
  system: &str,
  messages: &[Value],
  tools: &[Value],
) -> Result<ClaudeResponse, String> {
  let body = json!({
    "model": MODEL,
    "max_tokens": 4096,
    "system": system,
    "tools": tools,
    "messages": messages
  });

  let resp = client
    .post("https://api.anthropic.com/v1/messages")
    .header("x-api-key", api_key)
    .header("anthropic-version", "2023-06-01")
    .header("content-type", "application/json")
    .json(&body)
    .send()
    .map_err(|e| format!("HTTP error: {e}"))?;

  let status = resp.status();
  let text = read_body(resp)?;

  if !status.is_success() {
    return Err(friendly_error(status.as_u16(), &text));
  }

  serde_json::from_str(&text)
    .map_err(|e| format!("Failed to parse Claude response: {e}"))
}

fn read_body(resp: reqwest::blocking::Response) -> Result<String, String> {
  let mut buf = Vec::new();
  resp
    .take(MAX_BODY_BYTES + 1)
    .read_to_end(&mut buf)
    .map_err(|e| e.to_string())?;
  if buf.len() as u64 > MAX_BODY_BYTES {
    return Err("response exceeds 4 MB limit".to_string());
  }
  String::from_utf8(buf).map_err(|e| e.to_string())
}

fn friendly_error(status: u16, body: &str) -> String {
  if let Ok(v) = serde_json::from_str::<Value>(body) {
    if let Some(msg) = v["error"]["message"].as_str() {
      // Char-aware truncation guards the byte boundary; sanitize then
      // strips any terminal-escape bytes the upstream API embedded in the
      // error message — a hostile/compromised endpoint could otherwise
      // route OSC-52 / CSI sequences straight into discovery_status and
      // hijack the user's terminal (audit Sec HIGH #3).
      let short = crate::sanitize::truncate_chars(msg, 100);
      let short = crate::sanitize::sanitize_terminal_text(&short);
      return format!("Claude API — {short}");
    }
  }
  format!("Claude API error {status}")
}
