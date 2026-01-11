//! Copyright (c) Nov 16, 2021 Petr Maj
//! Originally from CodeDJ Parasite
//! Source: https://github.com/PRL-PRG/codedj-parasite/blob/old_sentinels_new_stuff/src/github.rs

#![allow(clippy::all)]

use curl::easy::*;
use std::str;
use std::sync::*;

pub struct Github {
    tokens: Mutex<TokensManager>,
}

impl Github {
    pub fn new(tokens: &str) -> Github {
        Github {
            tokens: Mutex::new(TokensManager::new(tokens)),
        }
    }

    /** Performs a github request of the specified url and returns the result string.  
     */
    pub fn request(&self, url: &str) -> Result<json::JsonValue, std::io::Error> {
        let mut attempts = 0;
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
            {
                let mut ct = conn.transfer();
                ct.write_function(|data| {
                    response.extend_from_slice(data);
                    return Ok(data.len());
                })?;
                ct.header_function(|data| {
                    response_headers.extend_from_slice(data);
                    return true;
                })?;
                ct.perform()?;
            }
            let rhdr = to_string(&response_headers).to_lowercase();
            if rhdr.starts_with("http/1.1 200")
                || rhdr.starts_with("http/1.1 301")
                || rhdr.starts_with("http/2 200")
                || rhdr.starts_with("http/2 301")
            {
                let result = json::parse(&to_string(&response));
                match result {
                    Ok(value) => return Ok(value),
                    Err(_) => {
                        return Err(std::io::Error::new(
                            std::io::ErrorKind::Other,
                            "Cannot parse json result",
                        ));
                    }
                }
            } else if rhdr.starts_with("http/1.1 401")
                || rhdr.starts_with("http/1.1 403")
                || rhdr.starts_with("http/2 401")
                || rhdr.starts_with("http/2 403")
            {
                if rhdr.contains("x-ratelimit-remaining: 0") {
                    // move to next token
                    self.tokens.lock().unwrap().next_token(token.1);
                // check for the secondary rate limit:)
                } else {
                    let result = json::parse(&to_string(&response));
                    match result {
                        Ok(value) => {
                            println!("{:?}", value);
                            if value["message"].is_string() && value["message"].as_str().unwrap() == "You have exceeded a secondary rate limit. Please wait a few minutes before you try again." {
                                println!("Secondary rate limit: sleep 1m");
                                std::thread::sleep(std::time::Duration::from_millis(1000 * 60));
                            }
                        }
                        Err(_) => {}
                    }
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::Other,
                        rhdr.split("\n").next().unwrap(),
                    ));
                }
            } else {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    rhdr.split("\n").next().unwrap(),
                ));
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

struct TokensManager {
    tokens: Vec<String>,
    current: usize,
}

impl TokensManager {
    fn new(filename: &str) -> TokensManager {
        let mut reader = csv::ReaderBuilder::new()
            .has_headers(true)
            .double_quote(false)
            .escape(Some(b'\\'))
            .from_path(filename)
            .unwrap();
        let mut tokens = Vec::<String>::new();
        for x in reader.records() {
            tokens.push(String::from(&x.unwrap()[0]));
        }
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

/** Lossless conversion from possibly non-UTF8 strings to valid UTF8 strings with the non-UTF bytes escaped.

   Because we can, we use the BEL character as escape character because the chances of real text containing it are rather small, yet it is reasonably simple for further processing.
*/

pub fn to_string(bytes: &[u8]) -> String {
    let mut result = String::new();
    let mut x = bytes;
    loop {
        match str::from_utf8(x) {
            // if successful, replace any bel character with double bel, add to the buffer and exit
            Ok(s) => {
                result.push_str(&s.replace("%", "%%"));
                return result;
            }
            Err(e) => {
                let (ok, bad) = x.split_at(e.valid_up_to());
                if !ok.is_empty() {
                    result.push_str(&str::from_utf8(ok).unwrap().replace("%", "%%"));
                }
                // encode the bad character
                result.push_str(&format!("%{:x}", bad[0]));
                // move past the offending character
                x = &bad[1..];
            }
        }
    }
}
