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

use anyhow::{bail, Error, Result};
use std::fmt::Display;
use std::io::{self, Write};
use std::sync::Arc;
use std::sync::OnceLock;
use std::time::Duration;
use tracing::{error, info, warn, Level};

use crate::utils::{csv::CSVFile, fs::FileMode, github::is_valid_token_file};

use super::fs::write_csv;
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use polars::frame::DataFrame;

#[derive(Debug)]
pub enum TaskStatus {
    InProgress,
    Success,
    Failure,
}

impl Display for TaskStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TaskStatus::InProgress => write!(f, "IN PROGRESS"),
            TaskStatus::Success => write!(f, "SUCCESS"),
            TaskStatus::Failure => write!(f, "FAILED"),
        }
    }
}

pub struct TaskLogger {
    /// The progress bar used to log the progress of the task.
    pb: ProgressBar,
    /// The message to log for the task.
    msg: String,
}

impl TaskLogger {
    /// Creates a new task logger and starts logging the progress of the task.
    ///
    /// # Arguments
    /// * `logger` - A reference to the logger to use for logging the progress of the task.
    /// * `msg` - The message to log for the task.
    ///
    /// # Returns
    /// The task logger, or an error if the logger could not be created.
    pub fn new(logger: &Logger, msg: impl Into<String>) -> Result<TaskLogger> {
        let msg: String = msg.into();

        let pb: ProgressBar = logger.progress.add(ProgressBar::new_spinner());
        let style: ProgressStyle = ProgressStyle::with_template("{spinner} {msg}")?;
        pb.set_style(style);
        pb.enable_steady_tick(Duration::from_millis(100));
        pb.set_message(format!("{msg} - {}", TaskStatus::InProgress));

        Ok(TaskLogger { pb, msg })
    }

    /// Logs the success of a task
    pub fn success(&self) {
        self.pb.finish_and_clear();
        info!("{} - {}", self.msg, TaskStatus::Success);
    }

    /// Logs the failure of a task
    pub fn failure(&self) {
        self.pb.finish_and_clear();
        error!("{} - {}", self.msg, TaskStatus::Failure);
    }

    /// Updates the message of the task logger.
    pub fn set_message(&self, msg: impl Into<String>) {
        self.pb.set_message(msg.into());
    }
}

#[derive(Clone)]
struct MultiProgressWriter {
    progress: Arc<MultiProgress>,
}

struct MultiProgressLineWriter {
    progress: Arc<MultiProgress>,
    buf: Vec<u8>,
}

impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for MultiProgressWriter {
    type Writer = MultiProgressLineWriter;

    fn make_writer(&'a self) -> Self::Writer {
        MultiProgressLineWriter {
            progress: Arc::clone(&self.progress),
            buf: Vec::new(),
        }
    }

    fn make_writer_for(&'a self, meta: &tracing::Metadata<'_>) -> Self::Writer {
        let _ = meta;
        self.make_writer()
    }
}

impl std::io::Write for MultiProgressLineWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.buf.extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        if !self.buf.is_empty() {
            let s = String::from_utf8_lossy(&self.buf);
            for line in s.lines() {
                let _ = self.progress.println(line);
            }
            self.buf.clear();
        }
        Ok(())
    }
}

impl Drop for MultiProgressLineWriter {
    fn drop(&mut self) {
        let _ = self.flush();
    }
}

pub struct Logger {
    /// The multiprogess bar used to log the progress of the tasks.
    progress: Arc<MultiProgress>,
}

impl Logger {
    /// Creates a new logger and sets it as the global default logger.
    ///
    /// * debug - If true, the logger will log debug messages as well. Otherwise, only info, warning and error messages will be logged.
    ///
    /// # Returns
    /// The logger, or an error if the logger could not be created.
    pub fn new(debug: bool) -> Result<Self> {
        let logger = Self {
            progress: Arc::new(MultiProgress::new()),
        };

        let writer = MultiProgressWriter {
            progress: Arc::clone(&logger.progress),
        };

        let max_level = if debug { Level::DEBUG } else { Level::INFO };
        let subscriber = tracing_subscriber::fmt()
            .with_writer(writer)
            .with_target(false)
            .without_time()
            .with_level(true)
            .with_max_level(max_level)
            .finish();

        tracing::subscriber::set_global_default(subscriber)?;

        Ok(logger)
    }

    /// Runs a task and logs its progress, success, or failure.
    ///
    /// # Arguments
    /// * `msg` - The message to log for the task.
    /// * `f` - The task to run
    ///
    /// # Returns
    /// The result of the task
    pub fn run_task<T>(
        &self,
        msg: impl Into<String>,
        f: impl FnOnce() -> anyhow::Result<T>,
    ) -> anyhow::Result<T> {
        let task = TaskLogger::new(self, msg)?;
        let result = f();

        match &result {
            Ok(_) => task.success(),
            Err(_) => task.failure(),
        }

        result
    }

    /// Logs the tokens file being loaded.
    ///
    /// # Arguments
    /// * `tokens_file` - The path to the tokens file.
    ///
    /// # Returns
    ///
    /// A result containing a vector of strings representing the tokens, or an error if the file is invalid.
    pub fn log_tokens(&self, tokens_file: &str) -> Result<Vec<String>> {
        self.run_task("Loading tokens", || {
            is_valid_token_file(tokens_file)
                .and_then(|_| CSVFile::new(tokens_file, FileMode::Read)?.column(0))
        })
    }
}

static TEST_LOGGER: OnceLock<Logger> = OnceLock::new();

/// Returns a reference to a logger that should be used for testing purposes.
pub fn test_logger() -> &'static Logger {
    TEST_LOGGER.get_or_init(|| Logger::new(true).unwrap())
}
/// Logs if the program will create an output file or overwrite an existing one.
/// In the latter case, it will also check if the user explicitly asked for it.
///
/// # Arguments
/// * `output_path` - The path to the output file.
/// * `no_output` - If true, no output file will be generated.
/// * `force` - Flag the user must set to override an existing file.
pub fn log_output_file(output_path: &str, no_output: bool, force: bool) -> Result<(), Error> {
    if no_output {
        info!("No output file will be generated.");
        Ok(())
    } else {
        match crate::utils::fs::check_path(output_path) {
            Ok(_) => {
                if force {
                    warn!("Overriding existing file: {}", output_path);
                    Ok(())
                } else {
                    bail!("File {output_path} already exists. Use --force to override it.")
                }
            }
            Err(_) => {
                info!("Creating new file: {}", output_path);
                Ok(())
            }
        }
    }
}

/// Logs the writing of a DataFrame to a CSV file, unless no_output is true.
///
/// # Arguments
/// * `logger` - A mutable reference to the logger.
/// * `output_path` - The path to the output file.
/// * `data` - The DataFrame to write to the output file.
/// * `no_output` - If true, no output file will be generated and the writing will not be logged.
///
/// # Returns
/// An error if the writing of the output file fails, or if the logging of the writing in the terminal fails.
pub fn log_write_output(
    logger: &Logger,
    output_path: &str,
    data: &mut DataFrame,
    no_output: bool,
) -> Result<()> {
    if !no_output {
        logger.run_task(format!("Writing to {output_path}"), || {
            write_csv(output_path, data)
        })
    } else {
        Ok(())
    }
}

/// Logs the seed used for random number generation.
///
/// # Arguments
/// * `seed` - The random seed to log.
pub fn log_seed(seed: u64) {
    info!("Your random seed is {}, don't forget it!", seed)
}
