use std::io::Read;
use std::sync::OnceLock;
use std::time::Duration;

pub const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);
pub const MAX_BODY_BYTES: u64 = 10 * 1024 * 1024; // 10 MB

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
  use super::client;

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
