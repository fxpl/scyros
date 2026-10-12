// Copyright 2026 Andrea Gilot
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

//! Explains an error code, such as E404.

use anyhow::Result;
use clap::{Arg, Command};

use crate::utils::error::{ErrorCode, ERRORS_DOC};

/// Command line arguments parsing.
pub fn cli() -> Command {
    Command::new("explain")
        .about("Explain an error code, such as E404.")
        .long_about(ERRORS_DOC)
        .disable_version_flag(true)
        .arg(
            Arg::new("code")
                .value_name("CODE")
                .help("The error code to explain, such as E404.")
                .required(true),
        )
}

/// Prints the explanation of an error code.
///
/// # Arguments
///
/// * `code` - The error code, such as `E404`.
pub fn run(code: &str) -> Result<()> {
    println!("{}", code.parse::<ErrorCode>()?.explanation());
    Ok(())
}
