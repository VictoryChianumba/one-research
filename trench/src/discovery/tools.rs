use crate::config::Config;
use crate::models::FeedItem;
use serde_json::{Value, json};

pub struct ToolDef {
  pub name: &'static str,
  pub description: &'static str,
  pub schema: Value,
}

pub struct ToolResult {
  pub items: Vec<FeedItem>,
  pub text: String,
}

/// Returns all tool definitions available given the current config.
/// search_web is only included when a Perplexity API key is configured.
pub fn all_tool_defs(config: &Config) -> Vec<ToolDef> {
  let mut tools = vec![
    ToolDef {
      name: "search_arxiv",
      description: "Search arXiv for recent AI/ML papers. Use 2-3 targeted queries for best coverage.",
      schema: json!({
        "type": "object",
        "properties": {
          "query": {
            "type": "string",
            "description": "Search query (title words, author name, or topic)"
          },
          "max_results": {
            "type": "integer",
            "description": "Results to return (1-50, default 20)",
            "minimum": 1,
            "maximum": 50
          }
        },
        "required": ["query"]
      }),
    },
    ToolDef {
      name: "fetch_arxiv_paper",
      description: "Fetch a specific arXiv paper by ID (e.g. '2312.01234'). Use when you know the exact paper.",
      schema: json!({
        "type": "object",
        "properties": {
          "arxiv_id": {
            "type": "string",
            "description": "arXiv ID, optionally with version suffix"
          }
        },
        "required": ["arxiv_id"]
      }),
    },
  ];

  if config
    .perplexity_api_key
    .as_deref()
    .map(|k| !k.trim().is_empty())
    .unwrap_or(false)
  {
    tools.push(ToolDef {
      name: "search_web",
      description: "Search the web for recent AI/ML news, blog posts, and discussions. Returns synthesized context.",
      schema: json!({
        "type": "object",
        "properties": {
          "query": {
            "type": "string",
            "description": "Search query"
          }
        },
        "required": ["query"]
      }),
    });
  }

  tools
}

pub fn execute(name: &str, input: &Value, config: &Config) -> ToolResult {
  match name {
    "search_arxiv" => exec_search_arxiv(input),
    "fetch_arxiv_paper" => exec_fetch_arxiv_paper(input),
    "search_web" => exec_search_web(input, config),
    _ => ToolResult { items: vec![], text: format!("Unknown tool: {name}") },
  }
}

fn exec_search_arxiv(input: &Value) -> ToolResult {
  let query = match input["query"].as_str() {
    Some(q) => q,
    None => return err_result("Missing query"),
  };
  let max = input["max_results"].as_u64().unwrap_or(20).min(50) as usize;

  match crate::ingestion::arxiv::search_query(query, max) {
    Ok(items) if items.is_empty() => ToolResult {
      items: vec![],
      text: format!("No arXiv papers found for '{query}'"),
    },
    Ok(items) => {
      let text = items_summary(&items, "arXiv");
      ToolResult { items, text }
    }
    Err(e) => err_result(&format!("arXiv search failed: {e}")),
  }
}

fn exec_fetch_arxiv_paper(input: &Value) -> ToolResult {
  let id = match input["arxiv_id"].as_str() {
    Some(id) => id,
    None => return err_result("Missing arxiv_id"),
  };

  match crate::ingestion::arxiv::fetch_by_ids(&[id.to_string()]) {
    Ok(items) if items.is_empty() => ToolResult {
      items: vec![],
      text: format!("No paper found with arXiv ID '{id}'"),
    },
    Ok(items) => {
      let text = items_summary(&items, "arXiv");
      ToolResult { items, text }
    }
    Err(e) => err_result(&format!("arXiv fetch failed: {e}")),
  }
}

fn exec_search_web(input: &Value, config: &Config) -> ToolResult {
  let query = match input["query"].as_str() {
    Some(q) => q,
    None => return err_result("Missing query"),
  };
  let api_key =
    match config.perplexity_api_key.as_deref().filter(|k| !k.trim().is_empty())
    {
      Some(k) => k.to_string(),
      None => return err_result("Perplexity API key not configured"),
    };

  match perplexity_search(query, &api_key) {
    Ok(text) => ToolResult { items: vec![], text },
    Err(e) => err_result(&format!("Web search failed: {e}")),
  }
}

fn perplexity_search(query: &str, api_key: &str) -> Result<String, String> {
  let body = json!({
    "model": "sonar",
    "messages": [{"role": "user", "content": query}],
    "search_recency_filter": "month"
  });

  let resp = crate::http::client()
    .post("https://api.perplexity.ai/chat/completions")
    .header("Authorization", format!("Bearer {api_key}"))
    .header("Content-Type", "application/json")
    .json(&body)
    .send()
    .map_err(|e| e.to_string())?;

  if !resp.status().is_success() {
    let status = resp.status().as_u16();
    return Err(format!("Perplexity API error {status}"));
  }

  // Route through read_body so the workspace 10 MB body cap applies — bare
  // resp.json() would let a hostile/compromised endpoint return arbitrarily
  // large JSON and exhaust memory on this thread.
  let body = crate::http::read_body(resp)?;
  let json: Value = serde_json::from_str(&body).map_err(|e| e.to_string())?;
  Ok(
    json["choices"][0]["message"]["content"]
      .as_str()
      .unwrap_or("No response from web search")
      .to_string(),
  )
}

fn items_summary(items: &[FeedItem], source: &str) -> String {
  let n = items.len();
  let preview: String = items
    .iter()
    .take(5)
    .map(|i| format!("- {}", i.title))
    .collect::<Vec<_>>()
    .join("\n");
  let note =
    if n > 5 { format!(" (showing first 5 of {n})") } else { String::new() };
  format!("Found {n} papers from {source}{note}:\n{preview}")
}

fn err_result(msg: &str) -> ToolResult {
  ToolResult { items: vec![], text: msg.to_string() }
}
