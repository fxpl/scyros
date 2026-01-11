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
// limitations under the License.

//! Collects random IDs of public projects on GitHub, along with their names and whether each project is a fork.
//! IDs are selected at random with replacement (so the same ID can appear multiple times).
//! The maximum ID is set to 871212690 by default (as of 2025/01/24).
//! The IDs are stored in a CSV file at a location provided as an argument.
//! If the program is interrupted, it can be restarted and will resume from the last ID sampled.
//! IDs are sampled in sequential chunks of 100.
//!
//! Output CSV file format:
//!  * id: ids of the projects.
//!  * name: full names (i.e., username/projectname) of the projects.
//!  * fork: whether projects are fork (1) or not (0).
//!  * requests: number of requests made to the Github API (roughly row number / 100).

use clap::ArgAction;
use clap::{Arg, Command};
use indicatif::ProgressBar;
use json::JsonValue;
use polars::prelude::DataFrame;
use polars::prelude::DataType;
use polars::prelude::Field;
use polars::prelude::Schema;
use std::fmt::Write as _;
use std::io::Write;
use std::iter::FromIterator as _;
use std::path::Path;

use crate::utils::csv::*;
use crate::utils::error::*;
use crate::utils::fs::*;
use crate::utils::github::*;
use crate::utils::github_api::Github;
use crate::utils::json::*;
use crate::utils::logger::Logger;
use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};

/// Command line arguments parsing.
pub fn cli() -> Command {
    Command::new("ids")
        .about("Collects random ids of public projects on GitHub, along with their name and whether the project is a fork.")
        .long_about(
            "Collects random IDs of public projects on GitHub, along with their names and whether each project is a fork.\n\
            IDs are selected at random with replacement (so the same ID can appear multiple times).\n\
            The maximum ID is set to 1128315983 by default (as of 2026/01/05).\n\
            The IDs are stored in a CSV file at a location provided as an argument.\n\
            If the program is interrupted, it can be restarted and will resume from the last ID sampled.\n\
            IDs are sampled in sequential chunks of 100."
        )
        .disable_version_flag(true)
        .arg(
            Arg::new("output")
                .short('o')
                .long("output")
                .value_name("OUTPUT_FILE.csv")
                .help("Path to the output CSV file storing the ids and the names of the projects. If the file does not exist, it will be created.\n\
                      If the file already exists, new data will be appended to the end of the file.")
                .required(true)
        )
        .arg(
            Arg::new("tokens")
                .short('t')
                .long("tokens")
                .value_name("TOKENS_FILE.csv")
                .help("Path to the file containing the GitHub tokens to use for querrying GitHub REST API.\n\
                       It must be a valid CSV file, with a column named 'token' where every entry is a valid GitHub token.")
                .required(true)
        )
        .arg(
            Arg::new("seed")
                .short('s')
                .long("seed")
                .value_name("SEED")
                .help("Seed used to generate random ids.")
                .default_value("11372246557183969657")
                .value_parser(clap::value_parser!(u64)),
        )
        .arg(
            Arg::new("min")
                .long("min")
                .value_name("MIN_ID")
                .help("Minimum id to sample.")
                .default_value("0")
                .value_parser(clap::value_parser!(u32))
        )
        .arg(
            Arg::new("max")
            .long("max")
            .value_name("MAX_ID")
            .help("Maximum id to sample. Default to 2026/01/05.")
            .default_value("1128315983")
            .value_parser(clap::value_parser!(u32))
        )
        .arg(
            Arg::new("number")
                .short('n')
                .long("number")
                .value_name("NUMBER_OF_IDS")
                .help("Number of ids to sample. \
                If the number is not a multiple of 100, it is rounded to the least multiple of 100 greater than the value.\n\
                If not specified the program will run indefinitely.")
                .value_parser(clap::value_parser!(usize))
        )
        .arg(
            Arg::new("mode")
            .long("mode")
            .value_name("MODE")
            .help("Sampling mode. 'linear' to sample ids in sequential order, 'random' to sample ids at random.")
            .default_value("random")
            .value_parser(["linear", "random"]),
        )
        .arg(
            Arg::new("force")
                .short('f')
                .long("force")
                .help("Override the output file if it already exists.")
                .default_value("false")
                .action(ArgAction::SetTrue),
        )
}

/// Main function
///
/// # Arguments
///
/// * `output_path` - Path to the output CSV file.
/// * `tokens` - Path to the file containing GitHub tokens.
/// * `seed` - Random seed used to generate the random ids.
/// * `min_id` - Minimum id to sample.
/// * `max_id` - Maximum id to sample.
/// * `n` - Number of ids to sample. If not defined, the program runs indefinitely.
/// * `mode` - Sampling mode. 'linear' to sample ids in sequential order, 'random' to sample ids at random.
/// * `force` - If true, overwrite the output file, append otherwise.
/// * `logger` - Logger printing to standard output.
///
pub fn run(
    output_path: &str,
    tokens: &str,
    seed: u64,
    min_id: u32,
    max_id: u32,
    n: Option<usize>,
    mode: &str,
    force: bool,
    logger: &mut Logger,
) -> Result<(), Error> {
    // Check if the token file is valid.
    logger.log_tokens(tokens)?;

    // Load the previous results if the file exists.
    let (mut last_id, mut requests): (u32, usize) = if force {
        logger.log("Overwriting previous results")?;
        (min_id, 0)
    } else if Path::new(output_path).exists() {
        let input_df: DataFrame = logger.log_completion("Loading previous results", || {
            open_csv(
                output_path,
                Some(Schema::from_iter(vec![
                    Field::new("id".into(), DataType::UInt32),
                    Field::new("name".into(), DataType::String),
                    Field::new("fork".into(), DataType::UInt32),
                    Field::new("request_number".into(), DataType::UInt32),
                ])),
                Some(ProjectInfo::header().to_vec()),
            )
        })?;
        let last_id: u32 = ok_or_else(
            map_err(
                map_err(input_df.column("id"), "Could not get 'id' column")?.u32(),
                "Could not convert 'id' column to u32",
            )?
            .last(),
            "Could not get last id",
        )?;

        let last_request_number: u32 = ok_or_else(
            map_err(
                map_err(
                    input_df.column("request_number"),
                    "Could not get 'request_number' column",
                )?
                .u32(),
                "Could not convert 'request_number' column to u32",
            )?
            .last(),
            "Could not get last request number",
        )?;

        logger.log(&format!("  {} ids already sampled.", input_df.height()))?;
        (last_id, last_request_number as usize + 1)
    } else {
        logger.log("No previous data found")?;
        (min_id, 0)
    };

    match n {
        Some(n) => logger.log(&format!("Sampling {} ids...", n))?,
        None => logger.log("Sampling ids...")?,
    }
    logger.log(&format!("Range: [{}, {}]", min_id, max_id))?;

    // Append or overwrite the data to the file depending on the force flag.
    let mut output_file = CSVFile::new(
        output_path,
        if force {
            FileMode::Overwrite
        } else {
            FileMode::Append
        },
    )?;

    // Write the header if the file is empty.
    output_file.write_header(ProjectInfo::header())?;

    // Initialize Github client.
    let gh = Github::new(tokens);

    // Create a progress bar if the number of ids to sample is known or a spinner if not.
    let progress_bar: ProgressBar = match n {
        Some(n) => ProgressBar::new(n as u64),
        None => ProgressBar::new_spinner(),
    };

    if n.is_some() {
        progress_bar.set_style(
            indicatif::ProgressStyle::default_bar()
                .template("{elapsed} {wide_bar} {percent}%")
                .unwrap(),
        )
    }

    // If the program was interrupted, the rng will be in the same state as before.
    // In order to avoid collecting the same ids again, we compute the number of requests
    // that were made before the interruption and generate that many random numbers that
    // will be discarded.

    let mut rng: StdRng = SeedableRng::seed_from_u64(seed);
    if mode == "random" {
        logger.log_seed(seed)?;
        for _ in 0..requests {
            rng.gen_range(min_id..max_id);
        }
    }

    // Number of reamaining IDs to collect.
    // Collects as long as this number is positive
    let mut remaining: Option<usize> = n;

    while remaining
        .map(|x| x > 0)
        .unwrap_or(mode == "random" || last_id < max_id)
    {
        // Generate a random id.
        let first_id: u32 = if mode == "random" {
            rng.gen_range(min_id..max_id)
        } else {
            last_id
        };

        const MAX_RETRIES: usize = 3;

        // Sends the request to the Github API.
        let request: JsonValue = {
            let mut attempts = 0;

            let mut request: Result<JsonValue, Error> =
                Error::new("Did not send any request yet: ID {}").to_res();
            while request.is_err() && attempts < MAX_RETRIES {
                request = map_err(
                    gh.request(&format!(
                        "https://api.github.com/repositories?since={}",
                        first_id
                    )),
                    &format!(
                        "Could not send the request to the Github API: ID {}",
                        first_id
                    ),
                );
                attempts += 1;
            }
            map_err(request, "Maximum number of retries reached")
        }?;
        match request {
            json::JsonValue::Array(repos) => {
                // String builder containing the content of the response
                let mut builder: String = String::new();

                // Number of repositories in the response.
                let response_size = repos.len();

                // Skipped null responses
                let mut skipped: usize = 0;

                // If the response is an array, process each repository.
                for repo in repos.iter() {
                    if repo.is_null() {
                        skipped += 1;
                    } else {
                        let project_info: ProjectInfo = ProjectInfo::parse_json(repo, ())?;
                        last_id = project_info.id as u32;
                        // Write the row in the CSV file.
                        map_err(
                            writeln!(&mut builder, "{}", project_info.to_csv(requests)),
                            &format!("Could not write {} to string builder", repo),
                        )?;
                    }
                }

                // Advance the progress bar.
                match remaining {
                    Some(_) => progress_bar.inc((response_size - skipped) as u64),
                    None => progress_bar.tick(),
                }

                // Substract ids sampled
                remaining = remaining.map(|x| x.saturating_sub(response_size - skipped));

                // Write the response to the file.
                map_err(
                    write!(&mut output_file, "{}", builder),
                    &format!("Could not write to file {}", output_path),
                )?;
            }
            // Handle "Not Found" error or unknown response format.
            _ => {
                if !request.has_key("message")
                    || ok_or_else(
                        request["message"].as_str(),
                        &format!("Could not parse message as string in {}", request),
                    )? != "Not Found"
                {
                    Error::new(&format!("Unknown response format: {} ", request)).to_res()?;
                }
            }
        }

        requests += 1;
    }

    Ok(())
}

/// Information about a GitHub project.
struct ProjectInfo {
    /// Project ID.
    id: i32,
    /// Project full name <user/repository>.
    name: String,
    /// Whether the project is a fork (1) or not (0).
    fork: u32,
}

impl ToCSV for ProjectInfo {
    /// request number
    type Key = usize;

    fn header() -> &'static [&'static str] {
        &["id", "name", "fork", "request_number"]
    }

    fn to_csv(&self, request_number: Self::Key) -> String {
        format!("{},{},{},{}", self.id, self.name, self.fork, request_number)
    }
}
impl Default for ProjectInfo {
    fn default() -> Self {
        Self {
            id: -1,
            name: String::new(),
            fork: 0,
        }
    }
}

impl FromGitHub for ProjectInfo {
    type Complement = ();

    fn parse_json(json: &json::JsonValue, _complement: ()) -> Result<Self, Error>
    where
        Self: Sized,
    {
        let id: u32 = get_field::<u32>(json, "id")?;
        let name: String = get_field::<String>(json, "full_name")?;
        let fork: u32 = get_field::<bool>(json, "fork")? as u32;

        Ok(Self {
            id: id as i32,
            name,
            fork,
        })
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;
    use crate::utils::logger::Logger;

    const TEST_DATA: &str = "tests/data/phases/ids";
    const TOKENS: &str = "ghtokens.csv";
    const SEED: u64 = 113722657;

    #[test]
    fn test_random_ids() {
        let id_half = format!("{}/id_random_1.csv", TEST_DATA);
        let id_full = format!("{}/id_random_2.csv", TEST_DATA);
        let id_force = format!("{}/id_random_3.csv", TEST_DATA);

        assert!(delete_file(&id_half, true).is_ok());
        assert!(delete_file(&id_full, true).is_ok());
        assert!(delete_file(&id_force, true).is_ok());

        run(
            &id_half,
            TOKENS,
            SEED,
            0,
            871212690,
            Some(280),
            "random",
            false,
            &mut Logger::new(),
        )
        .unwrap();

        assert!(run(
            &id_half,
            TOKENS,
            SEED,
            0,
            871212690,
            Some(280),
            "random",
            false,
            &mut Logger::new()
        )
        .is_ok());

        run(
            &id_full,
            TOKENS,
            SEED,
            0,
            871212690,
            Some(500),
            "random",
            false,
            &mut Logger::new(),
        )
        .unwrap();

        run(
            &id_force,
            TOKENS,
            SEED,
            0,
            871212690,
            Some(1000),
            "random",
            true,
            &mut Logger::new(),
        )
        .unwrap();

        run(
            &id_half,
            TOKENS,
            SEED,
            0,
            871212690,
            Some(500),
            "random",
            true,
            &mut Logger::new(),
        )
        .unwrap();

        assert!(fs::read_to_string(&id_half).unwrap() == fs::read_to_string(&id_full).unwrap());
        assert!(fs::read_to_string(&id_half).unwrap() != fs::read_to_string(&id_force).unwrap());

        assert!(delete_file(&id_half, false).is_ok());
        assert!(delete_file(&id_full, false).is_ok());
        assert!(delete_file(&id_force, false).is_ok());
    }

    #[test]
    fn test_linear_ids() {
        let id_half = format!("{}/id_linear_1.csv", TEST_DATA);
        let id_full = format!("{}/id_linear_2.csv", TEST_DATA);
        let id_force = format!("{}/id_linear_3.csv", TEST_DATA);

        assert!(delete_file(&id_half, true).is_ok());
        assert!(delete_file(&id_full, true).is_ok());
        assert!(delete_file(&id_force, true).is_ok());

        run(
            &id_half,
            TOKENS,
            SEED,
            0,
            871212690,
            Some(280),
            "linear",
            false,
            &mut Logger::new(),
        )
        .unwrap();

        assert!(run(
            &id_half,
            TOKENS,
            SEED,
            0,
            871212690,
            Some(280),
            "linear",
            false,
            &mut Logger::new()
        )
        .is_ok());

        run(
            &id_full,
            TOKENS,
            SEED,
            0,
            871212690,
            Some(500),
            "linear",
            false,
            &mut Logger::new(),
        )
        .unwrap();

        run(
            &id_force,
            TOKENS,
            SEED,
            0,
            871212690,
            Some(1000),
            "linear",
            true,
            &mut Logger::new(),
        )
        .unwrap();

        run(
            &id_half,
            TOKENS,
            SEED,
            0,
            871212690,
            Some(500),
            "linear",
            true,
            &mut Logger::new(),
        )
        .unwrap();

        assert!(fs::read_to_string(&id_half).unwrap() == fs::read_to_string(&id_full).unwrap());
        assert!(fs::read_to_string(&id_half).unwrap() != fs::read_to_string(&id_force).unwrap());

        assert!(delete_file(&id_half, false).is_ok());
        assert!(delete_file(&id_full, false).is_ok());
        assert!(delete_file(&id_force, false).is_ok());
    }
}
