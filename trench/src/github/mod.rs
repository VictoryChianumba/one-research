use serde::Deserialize;

// ── Public types ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub enum NodeType {
  Dir,
  File,
}

#[derive(Debug, Clone)]
pub struct TreeNode {
  pub name: String,
  pub path: String,
  pub node_type: NodeType,
}

// ── Public API ───────────────────────────────────────────────────────────────

/// Returns the default branch name for the given repo.
pub fn get_default_branch(
  owner: &str,
  repo: &str,
  token: &str,
) -> Result<String, String> {
  let url = format!("https://api.github.com/repos/{owner}/{repo}");
  let resp: RepoInfo = get_json(&url, token)?;
  Ok(resp.default_branch)
}

/// Returns the directory listing for `path` (empty string for root).
/// Directories come before files; both groups are sorted alphabetically.
pub fn fetch_tree_dir(
  owner: &str,
  repo: &str,
  branch: &str,
  path: &str,
  token: &str,
) -> Result<Vec<TreeNode>, String> {
  // Apply the same path encoding as fetch_file. Without this, a hostile or
  // compromised tree-listing response with `path = "../actions/secrets"` (or
  // `"foo?ref=evil"`) would smuggle a crafted URL into the next request,
  // sent with the user's `Authorization: Bearer <token>` header
  // (audit Sec HIGH #1).
  if !is_safe_branch(branch) {
    return Err(format!("github: rejected unsafe branch name: {branch:?}"));
  }
  let path_seg = if path.is_empty() {
    String::new()
  } else {
    format!("/{}", encode_url_path(path)?)
  };
  let url = format!(
    "https://api.github.com/repos/{owner}/{repo}/contents{path_seg}?ref={branch}"
  );

  let items: Vec<ContentItem> = get_json(&url, token)?;

  let mut nodes: Vec<TreeNode> = items
    .into_iter()
    .map(|item| TreeNode {
      name: item.name,
      path: item.path,
      node_type: if item.item_type == "dir" {
        NodeType::Dir
      } else {
        NodeType::File
      },
    })
    .collect();

  nodes.sort_by(|a, b| {
    let a_dir = matches!(a.node_type, NodeType::Dir);
    let b_dir = matches!(b.node_type, NodeType::Dir);
    b_dir.cmp(&a_dir).then_with(|| a.name.cmp(&b.name))
  });

  Ok(nodes)
}

/// Reject parent-directory traversal in `path`, then percent-encode every
/// byte that isn't safe for use inside a URL path segment. Preserves `/`
/// as a separator. Closes Sec MED #5 (recalibrated from HIGH): the prior
/// implementation interpolated `path` directly into the URL, allowing a
/// hostile GitHub tree-listing response with `path = "..?ref=token"` or
/// `"foo#frag"` to smuggle additional URL components into the request,
/// where our bearer token is attached.
/// True iff `branch` matches `[A-Za-z0-9._/-]+` and is non-empty. Branch
/// names from GitHub responses (`default_branch`, etc.) flow into the `?ref=`
/// query param of subsequent requests; without this gate, a tampered response
/// with `branch = "main&token=..."` could inject parameters into our calls.
fn is_safe_branch(branch: &str) -> bool {
  !branch.is_empty()
    && branch
      .chars()
      .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '/' | '-'))
}

fn encode_url_path(path: &str) -> Result<String, String> {
  if path.contains("..") {
    return Err(format!(
      "github: path contains parent-directory segment: {path:?}"
    ));
  }
  let mut out = String::with_capacity(path.len());
  for &b in path.as_bytes() {
    let safe = b.is_ascii_alphanumeric()
      || matches!(b, b'-' | b'_' | b'.' | b'~' | b'/');
    if safe {
      out.push(b as char);
    } else {
      out.push_str(&format!("%{b:02X}"));
    }
  }
  Ok(out)
}

/// Returns the raw UTF-8 text of a file.
/// Returns an error for binary files or files that exceed the GitHub 1 MB limit.
pub fn fetch_file(
  owner: &str,
  repo: &str,
  path: &str,
  token: &str,
) -> Result<String, String> {
  let encoded_path = encode_url_path(path)?;
  let url = format!(
    "https://api.github.com/repos/{owner}/{repo}/contents/{encoded_path}"
  );
  let resp: FileContent = get_json(&url, token)?;

  if resp.encoding.as_deref() != Some("base64") {
    return Err(format!(
      "Unexpected encoding: {}",
      resp.encoding.unwrap_or_default()
    ));
  }

  let clean = resp.content.replace('\n', "").replace('\r', "");
  let bytes = base64_decode(&clean)?;
  String::from_utf8(bytes)
    .map_err(|_| "Binary file — cannot display".to_string())
}

// ── HTTP helpers ─────────────────────────────────────────────────────────────

fn get_json<T: for<'de> Deserialize<'de>>(
  url: &str,
  token: &str,
) -> Result<T, String> {
  let mut req = crate::http::client()
    .get(url)
    .header("User-Agent", "trench/0.1")
    .header("Accept", "application/vnd.github+json")
    .header("X-GitHub-Api-Version", "2022-11-28");

  if !token.is_empty() {
    req = req.header("Authorization", format!("Bearer {token}"));
  }

  let resp = req.send().map_err(|e| format!("HTTP error: {e}"))?;

  if !resp.status().is_success() {
    return Err(format!("GitHub API: HTTP {}", resp.status()));
  }

  let body = crate::http::read_body(resp)?;
  serde_json::from_str(&body).map_err(|e| format!("JSON parse error: {e}"))
}

// ── Serde types ──────────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct RepoInfo {
  default_branch: String,
}

#[derive(Deserialize)]
struct ContentItem {
  name: String,
  path: String,
  #[serde(rename = "type")]
  item_type: String,
}

#[derive(Deserialize)]
struct FileContent {
  content: String,
  encoding: Option<String>,
}

fn base64_decode(s: &str) -> Result<Vec<u8>, String> {
  use base64::Engine;
  base64::engine::general_purpose::STANDARD
    .decode(s)
    .map_err(|e| format!("base64 decode error: {e}"))
}

#[cfg(test)]
mod tests {
  use super::encode_url_path;

  #[test]
  fn encodes_plain_path_unchanged() {
    assert_eq!(encode_url_path("src/main.rs").unwrap(), "src/main.rs");
    assert_eq!(encode_url_path("README.md").unwrap(), "README.md");
    assert_eq!(
      encode_url_path("path/to/file_name-1.txt").unwrap(),
      "path/to/file_name-1.txt"
    );
  }

  #[test]
  fn encodes_spaces_as_percent_20() {
    assert_eq!(
      encode_url_path("docs/my file.md").unwrap(),
      "docs/my%20file.md"
    );
  }

  #[test]
  fn encodes_question_mark_to_block_query_smuggling() {
    // The actionable attack — a path containing `?` smuggles new URL
    // query parameters that take precedence over the existing query.
    let encoded = encode_url_path("foo?ref=secret").unwrap();
    assert!(!encoded.contains('?'), "encoded path: {encoded}");
    assert!(encoded.contains("%3F"), "encoded path: {encoded}");
  }

  #[test]
  fn encodes_hash_to_block_fragment_smuggling() {
    let encoded = encode_url_path("foo#frag").unwrap();
    assert!(!encoded.contains('#'), "encoded path: {encoded}");
    assert!(encoded.contains("%23"), "encoded path: {encoded}");
  }

  #[test]
  fn rejects_parent_traversal() {
    assert!(encode_url_path("../etc/passwd").is_err());
    assert!(encode_url_path("foo/../bar").is_err());
    assert!(encode_url_path("foo..bar").is_err()); // two dots anywhere
  }

  #[test]
  fn allows_single_dot_segments() {
    // Single `.` is harmless (current dir reference).
    assert_eq!(encode_url_path("./foo").unwrap(), "./foo");
    assert_eq!(encode_url_path("foo.bar").unwrap(), "foo.bar");
  }

  #[test]
  fn encodes_non_ascii_via_utf8_bytes() {
    let encoded = encode_url_path("café.md").unwrap();
    // é is two bytes in UTF-8 (0xC3 0xA9).
    assert!(encoded.contains("%C3%A9"), "encoded: {encoded}");
  }
}
