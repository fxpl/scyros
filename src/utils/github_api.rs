//! Copyright (c) Nov 16, 2021 Petr Maj
//! Originally from CodeDJ Parasite
//! Source: <https://github.com/PRL-PRG/codedj-parasite/blob/old_sentinels_new_stuff/src/github.rs>

#![allow(clippy::all)]

use curl::easy::*;
use std::sync::*;
use tracing::warn;

/// Start of the message of an error that GitHub returned as an HTTP status, such as `http/2 404`.
pub const HTTP_ERROR_PREFIX: &str = "http/";

/// Start of the message of any other error of a request, such as a network error.
pub const OTHER_ERROR_PREFIX: &str = "error:";

pub struct Github {
    tokens: Mutex<TokensManager>,
    /// Idle handles, reused by the next requests so that their connections to the API stay open.
    /// Concurrent requests each take their own handle.
    connections: Mutex<Vec<Easy>>,
}

impl Github {
    pub fn new(tokens: Vec<String>) -> Github {
        Github {
            tokens: Mutex::new(TokensManager::new(tokens)),
            connections: Mutex::new(Vec::new()),
        }
    }

    /// Performs a github request of the specified url and returns the result string.
    ///
    /// Rate limits are waited out. Server errors (5xx) and transport errors are retried a few times.
    /// The message of an error starts with [`HTTP_ERROR_PREFIX`] when GitHub answered with an error status,
    /// followed by the status line of the final response (such as `http/2 404`), and with [`OTHER_ERROR_PREFIX`] otherwise.
    pub fn request(&self, url: &str) -> Result<json::JsonValue, std::io::Error> {
        self.request_with_retries(url).map_err(|e| {
            if e.to_string().starts_with(HTTP_ERROR_PREFIX) {
                e
            } else {
                std::io::Error::new(e.kind(), format!("{OTHER_ERROR_PREFIX} {e}"))
            }
        })
    }

    fn request_with_retries(&self, url: &str) -> Result<json::JsonValue, std::io::Error> {
        const MAX_TRANSIENT_FAILURES: u32 = 3;
        let mut attempts = 0;
        let mut transient_failures: u32 = 0;
        // Earliest time at which one of the tokens that reached their rate limit is reset.
        let mut earliest_reset: Option<u64> = None;
        let max_attempts = self.tokens.lock().unwrap().len();
        loop {
            let token = self.tokens.lock().unwrap().get_token();
            let (status, response_headers, response) = match self.get(url, &token.0) {
                Ok(performed) => performed,
                Err(e) => {
                    transient_failures += 1;
                    if transient_failures >= MAX_TRANSIENT_FAILURES {
                        return Err(e.into());
                    }
                    warn!("Request to {url} failed ({e}), retrying");
                    wait_before_retry(transient_failures);
                    continue;
                }
            };

            let all_headers = String::from_utf8_lossy(&response_headers).to_lowercase();
            let rhdr = final_response_headers(&all_headers);
            let status_line = rhdr.lines().next().unwrap_or_default().trim();
            let status_error = || std::io::Error::new(std::io::ErrorKind::Other, status_line);

            match status {
                200 => {
                    let result = json::parse(&String::from_utf8_lossy(&response));
                    match result {
                        Ok(value) => return Ok(value),
                        Err(_) => {
                            return Err(std::io::Error::new(
                                std::io::ErrorKind::Other,
                                "Cannot parse json result",
                            ));
                        }
                    }
                }
                401 | 403 | 429 if rhdr.contains("x-ratelimit-remaining: 0") => {
                    // move to next token
                    self.tokens.lock().unwrap().next_token(token.1);
                    if let Some(reset) = rate_limit_reset(rhdr) {
                        earliest_reset = Some(earliest_reset.map_or(reset, |r| r.min(reset)));
                    }
                }
                403 | 429 if status == 429 || is_secondary_rate_limit(rhdr, &response) => {
                    let wait_seconds: u64 = retry_after_seconds(rhdr).unwrap_or(60);
                    warn!("Secondary rate limit: waiting {wait_seconds} s");
                    std::thread::sleep(std::time::Duration::from_secs(wait_seconds));
                    continue;
                }
                500..=599 => {
                    transient_failures += 1;
                    if transient_failures >= MAX_TRANSIENT_FAILURES {
                        return Err(status_error());
                    }
                    warn!("Request to {url} failed ({status_line}), retrying");
                    wait_before_retry(transient_failures);
                    continue;
                }
                _ => return Err(status_error()),
            }
            attempts += 1;
            // All the tokens reached their rate limit: wait until the first of them is reset.
            if attempts == max_attempts {
                wait_for_reset(earliest_reset);
                attempts = 0;
                earliest_reset = None;
            }
        }
    }

    /// Sends a GET request to the specified url with the specified token.
    /// The connection is kept open for the next request.
    ///
    /// # Returns
    /// The status code, the headers and the body of the response, or an error if the request could not be performed.
    fn get(&self, url: &str, token: &str) -> Result<(u32, Vec<u8>, Vec<u8>), curl::Error> {
        let mut conn: Easy = self
            .connections
            .lock()
            .unwrap()
            .pop()
            .unwrap_or_else(Easy::new);
        let result = Self::get_with(&mut conn, url, token);
        self.connections.lock().unwrap().push(conn);
        result
    }

    /// Sends a GET request to the specified url with the specified token, using the specified handle.
    fn get_with(
        conn: &mut Easy,
        url: &str,
        token: &str,
    ) -> Result<(u32, Vec<u8>, Vec<u8>), curl::Error> {
        conn.url(url)?;
        conn.follow_location(true)?;
        let mut headers = List::new();
        headers.append("User-Agent: dcd")?;
        headers.append(&format!("Authorization: token {token}"))?;
        conn.http_headers(headers)?;

        let mut response = Vec::new();
        let mut response_headers = Vec::new();
        {
            let mut ct = conn.transfer();
            ct.write_function(|data| {
                response.extend_from_slice(data);
                Ok(data.len())
            })?;
            ct.header_function(|data| {
                response_headers.extend_from_slice(data);
                true
            })?;
            ct.perform()?;
        }
        Ok((conn.response_code()?, response_headers, response))
    }
}

/// Returns the headers of the final response. When redirects are followed, the headers of every response are concatenated.
fn final_response_headers(all_headers: &str) -> &str {
    match all_headers.rfind("\nhttp/") {
        Some(i) => &all_headers[i + 1..],
        None => all_headers,
    }
}

/// Waits before retrying a request that failed for a transient reason, longer after each failure.
fn wait_before_retry(failures: u32) {
    std::thread::sleep(std::time::Duration::from_secs(2u64.pow(failures)));
}

/// Returns whether a response reports a secondary rate limit, i.e. GitHub asks to slow down although the token has requests left.
/// GitHub signals it with 403 or 429 responses containing a `retry-after` header or a message about the secondary rate limit.
fn is_secondary_rate_limit(headers: &str, body: &[u8]) -> bool {
    retry_after_seconds(headers).is_some()
        || String::from_utf8_lossy(body)
            .to_lowercase()
            .contains("secondary rate limit")
}

/// Returns the number of seconds in the `retry-after` header of a response, if any.
fn retry_after_seconds(headers: &str) -> Option<u64> {
    numeric_header(headers, "retry-after:")
}

/// Returns the time at which the rate limit of a token is reset, in seconds since the Unix epoch,
/// from the `x-ratelimit-reset` header of a response, if any.
fn rate_limit_reset(headers: &str) -> Option<u64> {
    numeric_header(headers, "x-ratelimit-reset:")
}

/// Returns the numeric value of the header starting with `prefix` in lowercase headers, if any.
fn numeric_header(headers: &str, prefix: &str) -> Option<u64> {
    headers
        .lines()
        .find_map(|line| line.strip_prefix(prefix))
        .and_then(|value| value.trim().parse().ok())
}

/// Waits until a rate limit is reset at the given time, in seconds since the Unix epoch, or 10 minutes if the time is unknown.
fn wait_for_reset(reset: Option<u64>) {
    let wait_seconds: u64 = match reset {
        Some(reset) => {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_secs());
            // One more second in case the clocks differ slightly, and at least 10 seconds so that a
            // local clock ahead of GitHub's does not make every token fail again right away.
            (reset.saturating_sub(now) + 1).clamp(10, 60 * 60)
        }
        None => 60 * 10,
    };
    warn!("All tokens reached their rate limit: waiting {wait_seconds} s until it resets");
    std::thread::sleep(std::time::Duration::from_secs(wait_seconds));
}

struct TokensManager {
    tokens: Vec<String>,
    current: usize,
}

impl TokensManager {
    fn new(tokens: Vec<String>) -> TokensManager {
        TokensManager { tokens, current: 0 }
    }

    fn len(&self) -> usize {
        self.tokens.len()
    }

    /** Returns a possibly valid token that should be used for the request and its id.
     */
    fn get_token(&mut self) -> (String, usize) {
        (self.tokens[self.current].clone(), self.current)
    }

    fn next_token(&mut self, id: usize) {
        if self.current == id {
            self.current += 1;
            if self.current == self.tokens.len() {
                self.current = 0;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const REDIRECTED: &str = "http/2 301 \r\nlocation: https://api.github.com/repositories/1\r\n\r\nhttp/2 404 \r\nx-ratelimit-remaining: 4999\r\n\r\n";

    #[test]
    fn final_response_headers_skips_redirects() {
        assert!(final_response_headers(REDIRECTED).starts_with("http/2 404"));
        assert_eq!(
            final_response_headers("http/1.1 200 ok\r\n\r\n"),
            "http/1.1 200 ok\r\n\r\n"
        );
    }

    #[test]
    fn secondary_rate_limit_detection() {
        let limited = "http/2 403 \r\nretry-after: 30\r\n\r\n";
        assert_eq!(retry_after_seconds(limited), Some(30));
        assert!(is_secondary_rate_limit(limited, b""));
        assert!(is_secondary_rate_limit(
            "http/2 403 \r\n\r\n",
            br#"{"message": "You have exceeded a secondary rate limit and have been temporarily blocked."}"#
        ));
        assert!(!is_secondary_rate_limit(
            "http/2 403 \r\n\r\n",
            br#"{"message": "Repository access blocked"}"#
        ));
        assert_eq!(retry_after_seconds(REDIRECTED), None);
    }

    #[test]
    fn rate_limit_reset_detection() {
        let limited =
            "http/2 403 \r\nx-ratelimit-remaining: 0\r\nx-ratelimit-reset: 1791336000\r\n\r\n";
        assert_eq!(rate_limit_reset(limited), Some(1791336000));
        assert_eq!(rate_limit_reset(REDIRECTED), None);
    }
}
