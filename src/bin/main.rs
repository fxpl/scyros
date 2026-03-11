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

use anyhow::{anyhow, Context, Result};
use clap::{Arg, ArgAction, Command};
use scyros::phases::{
    download, duplicate_files, duplicate_ids, extract_benchmarks, filter_languages,
    filter_metadata, forks, ids, languages, metadata, parse, pull_request,
};
use scyros::utils::logger::Logger;
use tracing::{error, info};

fn cli() -> Command {
    Command::new("scyros")
        .about("")
        .author("Andrea Gilot <andrea.gilot@it.uu.se>")
        .subcommand(ids::cli())
        .subcommand(duplicate_ids::cli())
        .subcommand(forks::cli())
        .subcommand(metadata::cli())
        .subcommand(pull_request::cli())
        .subcommand(filter_metadata::cli())
        .subcommand(languages::cli())
        .subcommand(filter_languages::cli())
        .subcommand(download::cli())
        .subcommand(duplicate_files::cli())
        .subcommand(parse::cli())
        .subcommand(extract_benchmarks::cli())
        .arg(
            Arg::new("debug")
                .long("debug")
                .help("Print stack trace on error.")
                .action(ArgAction::SetTrue),
        )
        .disable_version_flag(true)
}

fn main() {
    let cli_args = cli().get_matches();

    // Calls to unwrap are safe because the arguments are required.
    let res: Result<()> =
        Logger::new().and_then(|logger|
        match cli_args.subcommand_name() {
            Some (subcommand) => {
                cli_args.subcommand_matches(subcommand).with_context(||
                format!("The subcommand {} is not available. Run the program with the --help flag to see the list of subcommands", subcommand)).and_then
                (
                    |cli_subargs| {
                            if subcommand == ids::cli().get_name() {
                                ids::run(
                                    cli_subargs.get_one::<String>("output").unwrap(),
                                    cli_subargs.get_one::<String>("tokens").unwrap(),
                                    *cli_subargs.get_one::<u64>("seed").unwrap(),
                                    *cli_subargs.get_one::<u32>("min").unwrap(),
                                    *cli_subargs.get_one::<u32>("max").unwrap(),
                                    cli_subargs.get_one::<usize>("number").copied(),
                                    cli_subargs.get_one::<String>("mode").unwrap(),
                                    cli_subargs.get_flag("force"),
                                    &logger
                                )
                            } else if subcommand == duplicate_ids::cli().get_name() {
                                duplicate_ids::run(
                                    cli_subargs.get_one::<String>("input").unwrap(),
                                    cli_subargs.get_one::<String>("output").map(|x| x.as_str()),
                                    cli_subargs.get_one::<String>("column").unwrap(),
                                    cli_subargs.get_flag("force"),
                                    cli_subargs.get_flag("no-output"),
                                    &logger
                                )
                            } else if subcommand == forks::cli().get_name() {
                                forks::run(
                                    cli_subargs.get_one::<String>("input").unwrap(),
                                    cli_subargs.get_one::<String>("output").map(|x| x.as_str()),
                                    cli_subargs.get_one::<String>("column").unwrap(),
                                    cli_subargs.get_flag("force"),
                                    cli_subargs.get_flag("no-output"),
                                    &logger
                                )
                            } else if subcommand == metadata::cli().get_name() {
                                metadata::run(
                                    cli_subargs.get_one::<String>("input").unwrap(),
                                    cli_subargs.get_one::<String>("output"),
                                    cli_subargs.get_one::<String>("tokens").unwrap(),
                                    cli_subargs.get_one::<String>("cache"),
                                    *cli_subargs.get_one::<u64>("seed").unwrap(),
                                    cli_subargs.get_flag("force"),
                                    cli_subargs.get_one::<String>("ids").unwrap(),
                                    cli_subargs.get_one::<String>("names").unwrap(),
                                    cli_subargs.get_one::<usize>("sub").copied(),
                                    &logger,
                                )
                            } else if subcommand == filter_metadata::cli().get_name() {
                                filter_metadata::run(
                                    cli_subargs.get_one::<String>("input").unwrap(),
                                    cli_subargs.get_one::<String>("output").map(|x| x.as_str()),
                                    cli_subargs.get_one::<u64>("loc").unwrap().to_owned(),
                                    cli_subargs.get_one::<u32>("age").unwrap().to_owned(),
                                    cli_subargs.get_flag("disabled"),
                                    cli_subargs.get_flag("non-code"),
                                    cli_subargs.get_flag("force"),
                                    cli_subargs.get_flag("no-output"),
                                    &logger,
                                )
                            } else if subcommand == languages::cli().get_name() {
                                languages::run(
                                    cli_subargs.get_one::<String>("input").unwrap(),
                                    cli_subargs.get_one::<String>("output").map(|x| x.as_str()),
                                    cli_subargs.get_one::<String>("tokens").unwrap(),
                                    cli_subargs.get_one::<String>("cache"),
                                    *cli_subargs.get_one::<u64>("seed").unwrap(),
                                    cli_subargs.get_flag("force"),
                                    cli_subargs.get_one::<String>("ids").unwrap(),
                                    cli_subargs.get_one::<String>("names").unwrap(),
                                    cli_subargs.get_one::<usize>("sub").copied(),
                                    &logger,
                                )
                            } else if subcommand == filter_languages::cli().get_name() {
                                filter_languages::run(
                                    cli_subargs.get_one::<String>("input").unwrap(),
                                    cli_subargs.get_one::<String>("output").map(|x| x.as_str()),
                                    cli_subargs.get_one::<String>("languages").unwrap(),
                                    cli_subargs.get_flag("force"),
                                    cli_subargs.get_flag("no-output"),
                                    &logger,
                                )
                            } else if subcommand == download::cli().get_name() {
                                download::run(
                                    cli_subargs.get_one::<String>("input").unwrap(),
                                    cli_subargs.get_one::<String>("projects").map(|x| x.as_str()),
                                    cli_subargs.get_one::<String>("files").map(|x| x.as_str()),
                                    cli_subargs.get_one::<String>("dest").unwrap(),
                                    cli_subargs.get_one::<String>("tokens").unwrap(),
                                    &cli_subargs
                                        .get_many::<String>("keywords")
                                        .unwrap()
                                        .map(|s| s.as_str())
                                        .collect::<Vec<&str>>(),
                                    cli_subargs.get_flag("skip"),
                                    cli_subargs.get_flag("count"),
                                    cli_subargs.get_flag("force"),
                                    *cli_subargs.get_one::<u64>("seed").unwrap(),
                                    &logger,
                                    *cli_subargs.get_one::<usize>("threads").unwrap(),
                                )
                            } else if subcommand == duplicate_files::cli().get_name() {
                                duplicate_files::run(
                                    cli_subargs.get_one::<String>("input").unwrap(),
                                    cli_subargs.get_one::<String>("output").map(|x| x.as_str()),
                                    cli_subargs.get_one::<String>("map").map(|x| x.as_str()),
                                    cli_subargs.get_flag("force"),
                                    cli_subargs.get_one::<String>("similarity").unwrap(),
                                    *cli_subargs.get_one::<usize>("threads").unwrap(),
                                    cli_subargs.get_one::<String>("header").unwrap(),
                                    &logger,
                                )
                            } else if subcommand == parse::cli().get_name() {
                                parse::run(
                                    cli_subargs.get_one::<String>("input").unwrap(),
                                    cli_subargs.get_one::<String>("output").map(|x| x.as_str()),
                                    cli_subargs.get_one::<String>("logs").map(|x| x.as_str()),
                                    &cli_subargs
                                        .get_many::<String>("keywords")
                                        .unwrap()
                                        .map(|s| s.as_str())
                                        .collect::<Vec<&str>>(),
                                        cli_subargs
                                        .get_many::<String>("lang")
                                        .map(|v|
                                        v.map(|s| s.as_str())
                                        .collect::<Vec<&str>>()),
                                    cli_subargs.get_one::<String>("failures").unwrap(),
                                    *cli_subargs.get_one::<usize>("threads").unwrap(),
                                    *cli_subargs.get_one::<u64>("seed").unwrap(),
                                    cli_subargs.get_flag("force"),
                                    &logger,
                                )
                            }
                            else if subcommand == extract_benchmarks::cli().get_name() {
                                extract_benchmarks::run(
                                    cli_subargs.get_one::<String>("input").unwrap(),
                                    cli_subargs.get_one::<String>("output").map(|x| x.as_str()),
                                    cli_subargs.get_one::<String>("dest").unwrap(),
                                    cli_subargs.get_one::<String>("tokens").unwrap(),
                                    *cli_subargs.get_one::<u64>("seed").unwrap(),
                                    cli_subargs.get_flag("force"),
                                    *cli_subargs.get_one::<usize>("threads").unwrap(),
                                    *cli_subargs.get_one::<u64>("timeout").unwrap(),
                                    &logger,
                                )
                            }
                            else if subcommand == pull_request::cli().get_name() {
                                pull_request::run(
                                    cli_subargs.get_one::<String>("input").unwrap(),
                                    cli_subargs.get_one::<String>("output"),
                                    cli_subargs.get_one::<String>("tokens").unwrap(),
                                    *cli_subargs.get_one::<u64>("seed").unwrap(),
                                    cli_subargs.get_flag("force"),
                                    cli_subargs.get_one::<String>("ids").unwrap(),
                                    cli_subargs.get_one::<String>("names").unwrap(),
                                    cli_subargs.get_one::<String>("dest").unwrap(),
                                    cli_subargs.get_one::<usize>("sub").copied(),
                                    &logger,
                                )
                            }
                            else {
                                Err(anyhow!("The subcommand {} is not available. Run the program with the --help flag to see the list of subcommands", subcommand))
                            }
                    }
                )
        }
        None => Err(anyhow!("You need to specify a subcommand. Run the program with the --help flag to see the list of subcommands"))
    });

    match res {
        Ok(_) => info!("Operation completed successfully."),
        Err(e) => {
            if cli_args.get_flag("debug") {
                error!("{:?}", e);
            } else {
                error!("{}", e);
            }
        }
    }
}
