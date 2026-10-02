//! Copyright (c) Nov 16, 2021 Petr Maj
//! Originally from CodeDJ Parasite
//! Source: https://github.com/PRL-PRG/codedj-parasite/blob/old_sentinels_new_stuff/src/github.rs

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
}

impl Github {
    pub fn new(tokens: Vec<String>) -> Github {
        Github {
            tokens: Mutex::new(TokensManager::new(tokens)),
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
        let max_attempts = self.tokens.lock().unwrap().len();
        loop {
            let mut response = Vec::new();
            let mut response_headers = Vec::new();
            let mut conn = Easy::new();
            conn.url(url)?;
            conn.follow_location(true)?;
            let mut headers = List::new();
            headers.append("User-Agent: dcd").unwrap();
            let token = self.tokens.lock().unwrap().get_token();
            headers
                .append(&format!("Authorization: token {}", token.0))
                .unwrap();
            conn.http_headers(headers)?;
            let performed = {
                let mut ct = conn.transfer();
                ct.write_function(|data| {
                    response.extend_from_slice(data);
                    return Ok(data.len());
                })?;
                ct.header_function(|data| {
                    response_headers.extend_from_slice(data);
                    return true;
                })?;
                ct.perform()
            };
            if let Err(e) = performed {
                transient_failures += 1;
                if transient_failures >= MAX_TRANSIENT_FAILURES {
                    return Err(e.into());
                }
                warn!("Request to {url} failed ({e}), retrying");
                wait_before_retry(transient_failures);
                continue;
            }

            let all_headers = String::from_utf8_lossy(&response_headers).to_lowercase();
            let rhdr = final_response_headers(&all_headers);
            let status_line = rhdr.lines().next().unwrap_or_default().trim();
            let status_error = || std::io::Error::new(std::io::ErrorKind::Other, status_line);

            let status: u32 = conn.response_code()?;
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
            // if we have too many attempts, it likely means that the tokens are all used up, wait 10 minutes is primitive and should work alright...
            if attempts == max_attempts {
                std::thread::sleep(std::time::Duration::from_millis(1000 * 60 * 10));
                attempts = 0;
            }
        }
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
    headers
        .lines()
        .find_map(|line| line.strip_prefix("retry-after:"))
        .and_then(|value| value.trim().parse().ok())
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
}
