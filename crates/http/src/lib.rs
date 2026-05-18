use std::io::Read;
use std::sync::OnceLock;
use std::time::Duration;

pub const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);
pub const MAX_BODY_BYTES: u64 = 10 * 1024 * 1024; // 10 MB

/// Retry policy for `with_retry`. `backoffs_ms` is the sleep before each
/// *retry* attempt (so `vec![3_000, 6_000]` means: first request, sleep
/// 3000ms, retry, sleep 6000ms, retry — three attempts total). `retriable`
/// classifies HTTP status codes; non-retriable codes return immediately.
///
/// See ADR-004 §D3. Constants here are load-bearing: the byte-equivalent
/// network behavior test in `with_retry::tests::arxiv_defaults_match_history`
/// asserts they match the deleted `fetch_arxiv_with_retry` helper's
/// hardcoded values.
pub struct RetryPolicy {
  pub backoffs_ms: Vec<u64>,
  pub retriable: fn(u16) -> bool,
}

impl RetryPolicy {
  /// arXiv export-API envelope — 3s then 6s on 429 or 503. Matches the
  /// constants that lived inside `huggingface::fetch_arxiv_with_retry`
  /// before C10 (commit `5491470`, deleted in C10 PR 2).
  pub fn arxiv() -> Self {
    Self {
      backoffs_ms: vec![3_000, 6_000],
      retriable: |code| code == 429 || code == 503,
    }
  }

  /// No retries — single attempt; any non-success status returns as-is.
  /// Used by sources whose upstreams haven't shown a 429/503 problem.
  pub fn none() -> Self {
    Self { backoffs_ms: vec![], retriable: |_| false }
  }
}

/// Execute `make` against `client`, retrying per `policy` on retriable
/// status codes. Sleeps `policy.backoffs_ms[attempt-1]` before each retry
/// using `std::thread::sleep` (blocking — consistent with the rest of the
/// blocking ingestion pipeline).
///
/// `make` is a closure rather than a `RequestBuilder` because
/// `RequestBuilder::send` consumes self — each attempt has to construct a
/// fresh builder. The closure receives `&Client` so it can chain `.get(url)`
/// without capturing the client itself.
///
/// Returns the first successful `Response`, or the last error string if
/// every attempt failed or a non-retriable status was returned.
pub fn with_retry<F>(
  client: &reqwest::blocking::Client,
  policy: &RetryPolicy,
  make: F,
) -> Result<reqwest::blocking::Response, String>
where
  F: Fn(&reqwest::blocking::Client) -> reqwest::blocking::RequestBuilder,
{
  let mut last_err = String::new();
  for attempt in 0..=policy.backoffs_ms.len() {
    if attempt > 0 {
      let wait = policy.backoffs_ms[attempt - 1];
      log::info!("http::with_retry: sleeping {wait}ms before retry {attempt}");
      std::thread::sleep(Duration::from_millis(wait));
    }
    let resp = match make(client).send() {
      Ok(r) => r,
      Err(e) => return Err(format!("HTTP request failed: {e}")),
    };
    let code = resp.status().as_u16();
    if resp.status().is_success() {
      return Ok(resp);
    }
    last_err = format!("HTTP {code}");
    if !(policy.retriable)(code) {
      return Err(last_err);
    }
  }
  Err(last_err)
}

/// Process-wide shared `reqwest::blocking::Client`. Memoized so DNS, pool,
/// and TLS state are reused across all callers. Hardened defaults: 15s
/// timeout, redirect cap of 2 (caps SSRF pivot range), and a uniform
/// `trench/<version>` user-agent. Returns `&'static` so callers don't pay
/// the refcount bump on every request.
pub fn client() -> &'static reqwest::blocking::Client {
  static CLIENT: OnceLock<reqwest::blocking::Client> = OnceLock::new();
  CLIENT.get_or_init(|| {
    reqwest::blocking::Client::builder()
      .timeout(REQUEST_TIMEOUT)
      .redirect(reqwest::redirect::Policy::limited(2))
      .user_agent(concat!("trench/", env!("CARGO_PKG_VERSION")))
      .build()
      .expect("failed to build HTTP client")
  })
}

/// Read a response body up to `MAX_BODY_BYTES`. Returns an error if the body
/// exceeds the limit or cannot be decoded as UTF-8.
pub fn read_body(resp: reqwest::blocking::Response) -> Result<String, String> {
  let mut limited = resp.take(MAX_BODY_BYTES + 1);
  let mut buf = Vec::new();
  limited.read_to_end(&mut buf).map_err(|e| format!("body read error: {e}"))?;
  if buf.len() as u64 > MAX_BODY_BYTES {
    return Err(format!(
      "response body exceeds {} MB limit",
      MAX_BODY_BYTES / 1024 / 1024
    ));
  }
  String::from_utf8(buf).map_err(|e| format!("body encoding error: {e}"))
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn client_is_memoized() {
    let a = client();
    let b = client();
    assert!(
      std::ptr::eq(a, b),
      "client() should return the same memoized instance on every call"
    );
  }

  // ── RetryPolicy / with_retry tests ───────────────────────────────────

  #[test]
  fn arxiv_defaults_match_history() {
    // Load-bearing: these are the exact constants `fetch_arxiv_with_retry`
    // shipped with at commit `5491470`. If anyone tunes the envelope they
    // must also update the deleted-helper history note in ADR-004.
    let p = RetryPolicy::arxiv();
    assert_eq!(p.backoffs_ms, vec![3_000, 6_000]);
    assert!((p.retriable)(429));
    assert!((p.retriable)(503));
    assert!(!(p.retriable)(500));
    assert!(!(p.retriable)(200));
    assert!(!(p.retriable)(404));
  }

  #[test]
  fn none_policy_has_no_backoffs_and_no_retriable_codes() {
    let p = RetryPolicy::none();
    assert!(p.backoffs_ms.is_empty());
    // Every conceivable status is non-retriable under the none policy.
    for code in [200, 301, 400, 404, 429, 500, 503] {
      assert!(!(p.retriable)(code), "code {code} should not be retriable");
    }
  }

  #[test]
  fn retry_policy_count_implies_attempt_count() {
    // Invariant: total attempts = backoffs.len() + 1 (the initial try).
    // The loop iterates `0..=backoffs.len()`, so this is structural.
    let p = RetryPolicy::arxiv();
    assert_eq!(p.backoffs_ms.len() + 1, 3, "arxiv policy = 3 attempts total");
    let n = RetryPolicy::none();
    assert_eq!(n.backoffs_ms.len() + 1, 1, "none policy = 1 attempt total");
  }
}
