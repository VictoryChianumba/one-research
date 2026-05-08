use std::io::Read;
use std::sync::OnceLock;
use std::time::Duration;

pub const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);
pub const MAX_BODY_BYTES: u64 = 10 * 1024 * 1024; // 10 MB

/// Process-wide shared `reqwest::blocking::Client`. Memoized so DNS, the
/// connection pool, and TLS state are reused across every ingestion source +
/// discovery + chat request, instead of building a fresh client (and a fresh
/// connection pool) per call as the prior implementation did across 15 sites.
///
/// Three hardening defaults are baked in here so they can't drift across
/// per-source request builders:
///
/// - `redirect::Policy::limited(2)` — caps cross-host redirect chains.
///   Prior default was 10, which is the SSRF amplification surface the code
///   review's Sec MED #15 flagged: a network-controlled URL can pivot up to
///   10 hops away from the originally-allowlisted host.
/// - `user_agent` set to `trench/<crate_version>` — was inconsistent across
///   sources (some set `trench/0.1`, some `trench/1.0`, most none); now
///   uniform.
/// - `timeout(REQUEST_TIMEOUT)` — unchanged, was already correct.
///
/// `reqwest::blocking::Client` is `Send + Sync + Clone` (all internal state
/// is `Arc`-wrapped), so a `OnceLock` works directly without `Mutex`. We
/// return `&'static Client` rather than cloning so callers don't pay a
/// refcount bump on every request — `Client::get` takes `&self`.
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
  use super::client;

  /// Two consecutive `client()` calls must return pointer-equal references —
  /// proves the `OnceLock` is doing its job and we have one shared client
  /// per process, not 15 per refresh cycle.
  #[test]
  fn client_is_memoized() {
    let a = client();
    let b = client();
    assert!(
      std::ptr::eq(a, b),
      "client() should return the same memoized instance on every call"
    );
  }
}
