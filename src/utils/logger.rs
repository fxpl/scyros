// Copyright 2025 Andrea Gilot
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
// limitations under the License.s

use std::fmt::Display;

use crate::utils::{csv::CSVFile, fs::FileMode, github::is_valid_token_file};

use super::{error::*, fs::write_csv};
use console::Style;
use console::Term;
use polars::frame::DataFrame;

#[derive(Debug)]
pub enum TaskStatus {
    InProgress,
    Success,
    Failure,
}

impl Display for TaskStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}",
            match self {
                TaskStatus::InProgress => "...",
                TaskStatus::Success => " - SUCCESS",
                TaskStatus::Failure => " - FAILED",
            }
        )
    }
}

pub struct Logger {
    term: Term,
    last_line: Option<String>,
}

impl Default for Logger {
    fn default() -> Self {
        Logger::new()
    }
}

impl Logger {
    pub fn new() -> Logger {
        Logger {
            term: Term::stdout(),
            last_line: None,
        }
    }

    pub fn change_status(&self, status: TaskStatus) -> Result<(), Error> {
        map_err(
            self.term
                .clear_line()
                .and_then(|_| self.term.move_cursor_up(1)),
            "Could not log in the terminal",
        )?;
        let last_line: &str = ok_or_else(
            self.last_line.as_ref(),
            "Could not change the status of a non previously logged task",
        )?;
        map_err(
            self.term.write_line(&format!("{}{}", last_line, status)),
            "Could not log in the terminal",
        )
    }

    pub fn replace_last_line(&mut self, msg: &str) -> Result<(), Error> {
        map_err(
            self.term
                .clear_line()
                .and_then(|_| self.term.move_cursor_up(1))
                .and_then(|_| self.term.write_line(msg)),
            "Could not log in the terminal",
        )
    }

    pub fn log(&mut self, msg: &str) -> Result<(), Error> {
        self.last_line = Some(msg.to_string());
        map_err(self.term.write_line(msg), "Could not log in the terminal")
    }

    pub fn log_warning(&mut self, msg: &str) -> Result<(), Error> {
        let yellow = Style::new().yellow();
        self.log(&yellow.apply_to(format!("WARNING: {}", msg)).to_string())
    }

    /// Logs the seed used for random number generation.
    ///
    /// # Arguments
    /// * `seed` - The random seed to log.
    ///
    /// # Returns
    ///
    /// A result indicating success or failure of the operation.
    pub fn log_seed(&mut self, seed: u64) -> Result<(), Error> {
        self.log(&format!("Your random seed is {}, don't forget it!", seed))
    }

    /// Logs the tokens file being loaded.
    ///
    /// # Arguments
    /// * `tokens_file` - The path to the tokens file.
    ///
    /// # Returns
    ///
    /// A result containing a vector of strings representing the tokens, or an error if the file is invalid.
    pub fn log_tokens(&mut self, tokens_file: &str) -> Result<Vec<String>, Error> {
        self.log_completion("Loading tokens", || {
            is_valid_token_file(tokens_file)
                .and_then(|_| CSVFile::new(tokens_file, FileMode::Read)?.column(0))
        })
    }

    pub fn start_task(&mut self, msg: &str) -> Result<(), Error> {
        self.log(msg)?;
        self.change_status(TaskStatus::InProgress)
    }

    pub fn log_completion<T, F>(&mut self, msg: &str, task: F) -> Result<T, Error>
    where
        F: FnOnce() -> Result<T, Error>,
    {
        self.start_task(msg)?;
        let res: Result<T, Error> = task();
        if res.is_ok() {
            self.change_status(TaskStatus::Success)
        } else {
            self.change_status(TaskStatus::Failure)
        }?;
        res
    }
}

/// Logs if the program will create an output file or overwrite an existing one.
/// In the latter case, it will also check if the user explicitly asked for it.
///
/// # Arguments
/// * `logger` - A mutable reference to the logger.
/// * `output_path` - The path to the output file.
/// * `no_output` - If true, no output file will be generated.
/// * `force` - Flag the user must set to override an existing file.
pub fn log_output_file(
    logger: &mut Logger,
    output_path: &str,
    no_output: bool,
    force: bool,
) -> Result<(), Error> {
    if no_output {
        logger.log("No output file will be generated.")
    } else {
        match crate::utils::fs::check_path(output_path) {
            Ok(_) => {
                if force {
                    logger.log(&format!("Overriding existing file: {}", output_path))
                } else {
                    Error::new(&format!(
                        "File {} already exists. Use --force to override it.",
                        output_path
                    ))
                    .to_res()
                }
            }
            Err(_) => logger.log(&format!("Creating new file: {}", output_path)),
        }
    }
}

pub fn log_write_output(
    logger: &mut Logger,
    output_path: &str,
    data: &mut DataFrame,
    no_output: bool,
) -> Result<(), Error> {
    if !no_output {
        logger.log_completion(&format!("Writing to {}", output_path), || {
            write_csv(output_path, data)
        })
    } else {
        Ok(())
    }
}
