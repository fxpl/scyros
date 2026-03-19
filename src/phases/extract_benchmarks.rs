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

#[doc = include_str!("../docs/extract_benchmarks.md")]
use crate::utils::csv::CSVFile;
use crate::utils::dataframes;
use crate::utils::fs::*;
use crate::utils::logger::Logger;
use anyhow::{anyhow, bail, Context, Result};
use clang::{Clang, Entity, EntityKind, Index, Usr};
use clap::{Arg, ArgAction, Command};
use indicatif::ProgressBar;
use petgraph::algo::toposort;
use petgraph::graph::{DiGraph, NodeIndex};
use polars::frame::DataFrame;
use polars::prelude::BooleanType;
use polars::prelude::ChunkedArray;
use polars::prelude::{AnyValue, DataType, Field, Schema};
use rand::rngs::StdRng;
use rand::seq::SliceRandom as _;
use rand::SeedableRng;
use regex::Regex;
use std::fmt::{Display, Formatter};
use std::io::Write as _;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;
use std::{
    collections::{HashMap, HashSet, VecDeque},
    fs::read,
    iter::FromIterator as _,
    path::PathBuf,
};
use tracing::{info, warn};

/// Command line arguments parsing.
pub fn cli() -> Command {
    Command::new("extract_benchmarks")
        .about("(Experimental) Extract self-contained C files containing all the dependencies of specified functions.")
        .long_about(include_str!("../docs/extract_benchmarks.md"))
        .author("Andrea Gilot <andrea.gilot@it.uu.se>")
        .disable_version_flag(true)
        .arg(
            Arg::new("input")
                .short('i')
                .long("input")
                .value_name("INPUT_FILE.csv")
                .help("Path to the input csv file containing the functions.")
                .required(true),
        )
        .arg(
            Arg::new("output")
                .short('o')
                .long("output")
                .value_name("OUTPUT_FILE.csv")
                .help("Path to the output csv file storing the functions with their dependencies.")
                .required(false),
        )
        .arg(
            Arg::new("tokens")
                .short('t')
                .long("tokens")
                .value_name("TOKENS_FILE.csv")
                .help("Path to the file containing the GitHub tokens to use. It must be a valid CSV file with one column named 'token' and where every line is a \
                       valid GitHub token (e.g ghp_Ab0C1D2eFg3hIjk4LM56oPqRsTuvWX7yZa8B).")
                .required(true)
        )

        .arg(
            Arg::new("dest")
                .short('d')
                .long("dest")
                .aliases(["target", "destination"])
                .value_name("DESTINATION")
                .help("Directory where the projects and the benchmark files will be stored.")
                .required(true),
        )
        .arg(
            Arg::new("force")
                .long("force")
                .help("Overwrite the output file if it already exists.")
                .action(ArgAction::SetTrue)
        )
        .arg(
            Arg::new("seed")
                .short('s')
                .long("seed")
                .value_name("SEED")
                .help("Seed used to randomly shuffle the input data.")
                .default_value("8966752472649624")
                .value_parser(clap::value_parser!(u64)),
        )
        .arg(
            Arg::new("threads")
                .short('n')
                .help("Number of threads to use when downloading projects.")
                .requires("skip")
                .default_value("1")
                .value_parser(clap::value_parser!(usize)),
        )
        .arg(
            Arg::new("timeout")
                .long("timeout")
                .help("Timeout (in seconds) for parsing a function.")
                .default_value("30")
                .value_parser(clap::value_parser!(u64)),
        )
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct EntityKey {
    usr: Option<Usr>,
    name: Option<String>,
}

impl EntityKey {
    fn from_entity(e: &Entity) -> Self {
        Self {
            usr: e.get_usr(),
            name: e.get_name(),
        }
    }

    fn is_empty(&self) -> bool {
        self.usr.is_none() && self.name.is_none()
    }

    fn is_stdlib(&self) -> bool {
        self.usr.clone().is_some_and(|usr| {
            usr.0 == "c:@F@printf"
                || usr.0 == "c:@F@scanf"
                || usr.0 == "c:@F@malloc"
                || usr.0 == "c:@F@free"
                || usr.0 == "c:@F@calloc"
                || usr.0 == "c:@F@realloc"
                || usr.0 == "c:@F@memcpy"
                || usr.0 == "c:@F@memset"
                || usr.0 == "c:@F@fprintf"
                || usr.0 == "c:@F@pow"
        })
    }
}

impl Display for EntityKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match (&self.usr, &self.name) {
            (None, None) => write!(f, "<unknown>"),
            (Some(usr), Some(name)) => write!(f, "({name}: {usr:?})"),
            (Some(usr), None) => write!(f, "(<unknown>: {usr:?})"),
            (None, Some(name)) => write!(f, "({name}: <unknown>)"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct EntityData {
    children: Vec<EntityData>,
    key: EntityKey,
    kind: EntityKind,
    start: usize,
    end: usize,
    reference: Option<(EntityKey, EntityKind)>,
    file: Option<PathBuf>,
}

impl EntityData {
    fn from_entity(e: &Entity) -> Result<Self> {
        let mut children: Vec<EntityData> = Vec::new();
        for child in e.get_children().iter() {
            children.push(EntityData::from_entity(child)?);
        }

        let range = e
            .get_range()
            .with_context(|| "Could not get entity range")?;
        let start = range.get_start().get_spelling_location();
        let end = range.get_end().get_spelling_location();
        let file = start.file.map(|f| f.get_path());

        let reference = e
            .get_reference()
            .map(|r| (EntityKey::from_entity(&r), r.get_kind()));

        Ok(Self {
            children,
            key: EntityKey::from_entity(e),
            kind: e.get_kind(),
            start: start.offset as usize,
            end: end.offset as usize,
            reference,
            file,
        })
    }

    fn extract_code(&self) -> Result<Vec<u8>> {
        let file: PathBuf = self
            .file
            .clone()
            .with_context(|| "Error while cloning file path")?;
        let mut code = read(&file)?
            .get(self.start..self.end)
            .with_context(|| "Invalid range for entity code extraction")?
            .to_vec();
        if matches!(
            self.kind,
            EntityKind::TypedefDecl
                | EntityKind::StructDecl
                | EntityKind::UnionDecl
                | EntityKind::EnumDecl
        ) && !code.ends_with(b";")
        {
            code.extend_from_slice(b";");
        }

        Ok(code)
    }

    fn all_references(&self) -> HashSet<&(EntityKey, EntityKind)> {
        let mut refs = HashSet::new();
        if let Some(ref_key) = &self.reference {
            refs.insert(ref_key);
        }
        for child in &self.children {
            refs.extend(child.all_references());
        }
        refs
    }

    fn all_references_decl(&self) -> HashSet<&EntityKey> {
        self.all_references()
            .iter()
            .filter_map(|(key, kind)| {
                if matches!(
                    kind,
                    EntityKind::FunctionDecl
                        | EntityKind::TypedefDecl
                        | EntityKind::StructDecl
                        | EntityKind::UnionDecl
                        | EntityKind::EnumDecl
                ) {
                    Some(key)
                } else {
                    None
                }
            })
            .collect()
    }
}

impl Display for EntityData {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        fn go(
            node: &EntityData,
            f: &mut Formatter<'_>,
            prefix: &str,
            is_last: bool,
        ) -> std::fmt::Result {
            let connector = if prefix.is_empty() {
                ""
            } else if is_last {
                "└─ "
            } else {
                "├─ "
            };

            write!(
                f,
                "{}{}{:?} {} [{}:{}..{}]",
                prefix,
                connector,
                node.kind,
                node.key,
                node.file
                    .as_ref()
                    .map(|f| f.display().to_string())
                    .unwrap_or_default(),
                node.start,
                node.end,
            )?;

            if let Some(ref r) = node.reference {
                write!(f, ", ref→{},{:?}", r.0, r.1)?;
            }
            writeln!(f, ")")?;

            let child_prefix_piece = if is_last { "   " } else { "│  " };
            let mut new_prefix = String::with_capacity(prefix.len() + 3);
            new_prefix.push_str(prefix);
            new_prefix.push_str(child_prefix_piece);

            let last = node.children.len().saturating_sub(1);
            for (i, child) in node.children.iter().enumerate() {
                go(child, f, &new_prefix, i == last)?;
            }
            Ok(())
        }

        go(self, f, "", true)
    }
}

struct Workspace {
    clang: Clang,

    root_function_name: String,

    root_file: PathBuf,

    decl: HashMap<EntityKey, EntityData>,

    dependencies: DiGraph<EntityKey, ()>,

    /// Candidate files to parse, sorted by proximity to the root file.
    candidates: VecDeque<PathBuf>,

    node_indices: HashMap<EntityKey, NodeIndex>,

    ignored: HashSet<EntityKey>,

    macros: Vec<Vec<u8>>,

    includes: HashSet<String>,

    cache: bool,

    timeout: u64,

    creation_time: std::time::Instant,
}

impl Workspace {
    fn new(
        clang: Clang,
        project_root: &PathBuf,
        root_file: &PathBuf,
        root_function: &str,
        cache: bool,
        timeout: u64,
    ) -> Result<Self> {
        let candidates = VecDeque::from(files_sorted_by_proximity(project_root, root_file, "c")?);

        Ok(Self {
            clang,
            root_function_name: root_function.to_string(),
            root_file: root_file.clone(),
            decl: HashMap::new(),
            candidates,
            dependencies: DiGraph::new(),
            node_indices: HashMap::new(),
            ignored: HashSet::new(),
            macros: Vec::new(),
            includes: HashSet::new(),
            cache,
            timeout,
            creation_time: std::time::Instant::now(),
        })
    }

    fn check_timeout(&self) -> Result<()> {
        if self.creation_time.elapsed().as_secs() > self.timeout {
            bail!("Timeout reached")
        } else {
            Ok(())
        }
    }

    fn index_file(&mut self, file: &PathBuf, search_key: Option<&EntityKey>) -> Result<()> {
        self.check_timeout()?;
        // Read the file and extract all #include<...> directives
        if let Ok(src) = std::fs::read_to_string(file) {
            for line in src.lines() {
                if let Some(rest) = line.trim_start().strip_prefix("#include") {
                    let rest = rest.trim_start();
                    if rest.starts_with('<') {
                        if let Some(end) = rest.find('>') {
                            let inc = &rest[..=end];
                            self.includes.insert(format!("#include{inc}"));
                        }
                    }
                }
            }
        }

        let index = Index::new(&self.clang, false, false);
        let tu = index
            .parser(file)
            .skip_function_bodies(false)
            .detailed_preprocessing_record(true)
            .parse()
            .with_context(|| format!("Could not parse file {:?}", file.to_str()))?;
        let root = tu.get_entity();

        let mut map = HashMap::<EntityKey, EntityData>::new();
        let includes = HashSet::<String>::new();
        let mut macros = Vec::<Vec<u8>>::new();

        root.visit_children(|e, _parent| {
            if file == &self.root_file && matches!(e.get_kind(), EntityKind::MacroDefinition) {
                if let Ok(entity) = EntityData::from_entity(&e) {
                    if let Ok(code) = entity.extract_code() {
                        macros.push(code);
                    }
                }
                clang::EntityVisitResult::Continue
            } else if matches!(
                e.get_kind(),
                |EntityKind::TypedefDecl| EntityKind::StructDecl
                    | EntityKind::UnionDecl
                    | EntityKind::EnumDecl
            ) || (e.get_kind() == EntityKind::FunctionDecl && e.is_definition())
            {
                let decl = e.get_definition().or(e.get_reference()).unwrap_or(e);
                let key = EntityKey::from_entity(&decl);

                if !key.is_empty()
                    && !map.contains_key(&key)
                    && search_key.is_none_or(|k| k == &key)
                {
                    if let Ok(entity_data) = EntityData::from_entity(&decl) {
                        map.insert(key, entity_data);
                        if search_key.is_some() {
                            return clang::EntityVisitResult::Break;
                        }
                    }
                }
                clang::EntityVisitResult::Continue
            } else {
                clang::EntityVisitResult::Recurse
            }
        });

        self.decl.extend(map);
        self.includes.extend(includes);
        self.macros.extend(macros);

        Ok(())
    }

    fn discover_candidates(&mut self, key: &EntityKey) -> Result<()> {
        self.check_timeout()?;
        if self.cache {
            if !self.decl.contains_key(key) && self.candidates.is_empty() {
                self.ignored.insert(key.clone());
                Ok(())
            } else {
                while !self.decl.contains_key(key) && !self.candidates.is_empty() {
                    match self.candidates.pop_front() {
                        Some(candidate) => self.index_file(&candidate, None)?,
                        None => {
                            self.ignored.insert(key.clone());
                        }
                    }
                }
                Ok(())
            }
        } else if self.decl.contains_key(key) {
            Ok(())
        } else {
            for candidate in self.candidates.clone() {
                self.index_file(&candidate, Some(key))?;
                if self.decl.contains_key(key) {
                    return Ok(());
                }
            }
            self.ignored.insert(key.clone());
            Ok(())
        }
    }

    fn discover_root(&mut self) -> Result<&EntityKey> {
        self.check_timeout()?;
        let root_file = self
            .candidates
            .pop_front()
            .with_context(|| "No root file found")?;
        self.index_file(&root_file, None)
            .with_context(|| format!("Could not index root file {root_file:?}"))?;
        for key in self.decl.keys() {
            if key.name.as_deref() == Some(&self.root_function_name) {
                return Ok(key);
            }
        }
        bail!(
            "Root function {} not found in {} (potentially due to conditional compilation)",
            self.root_function_name,
            root_file.display()
        )
    }

    fn explore_entity(
        &mut self,
        key: &EntityKey,
        explored: &mut HashSet<EntityKey>,
        to_explore: &mut VecDeque<EntityKey>,
    ) -> Result<()> {
        self.check_timeout()?;
        let _ = self.add_node(key);
        explored.insert(key.clone());
        self.discover_candidates(key)?;
        if let Some(entity) = self.decl.get(key).cloned() {
            for k in entity.all_references_decl() {
                let _ = self.add_node(k);
                let _ = self.add_edge(key, k);
                if !explored.contains(k) && !k.is_stdlib() {
                    // println!("Discovered dependency {} of {}", k, key);
                    to_explore.push_back(k.clone());
                }
            }
        }

        Ok(())
    }

    fn add_node(&mut self, key: &EntityKey) -> Result<()> {
        self.check_timeout()?;
        if !self.node_indices.contains_key(key) {
            let idx = self.dependencies.add_node(key.clone());
            self.node_indices.insert(key.clone(), idx);
            Ok(())
        } else {
            bail!("Node already exists")
        }
    }

    fn add_edge(&mut self, from: &EntityKey, to: &EntityKey) -> Result<()> {
        self.check_timeout()?;
        if from == to {
            bail!("Cannot add self-loop edges")
        } else {
            let from_idx = self
                .node_indices
                .get(from)
                .with_context(|| "From node not found")?;
            let to_idx = self
                .node_indices
                .get(to)
                .with_context(|| "To node not found")?;
            self.dependencies.add_edge(*from_idx, *to_idx, ());
            Ok(())
        }
    }

    fn resolve_dependencies(&mut self) -> Result<Vec<EntityKey>> {
        self.check_timeout()?;
        let root_key = self.discover_root()?;
        let mut explored: HashSet<EntityKey> = HashSet::new();
        let mut to_explore: VecDeque<EntityKey> = VecDeque::new();
        to_explore.push_back(root_key.clone());
        while !to_explore.is_empty() {
            let key = to_explore
                .pop_front()
                .ok_or_else(|| anyhow!("No more entities to explore"))?;
            self.explore_entity(&key, &mut explored, &mut to_explore)
                .with_context(|| format!("Error exploring entity {key}"))?;
        }
        // Topological sort of the dependency graph
        let mut sorted_idx = toposort(&self.dependencies, None)
            .map_err(|_| anyhow!("Cycle detected in dependency graph"))?;
        sorted_idx.reverse();

        Ok(sorted_idx
            .into_iter()
            .map(|idx| self.dependencies.node_weight(idx).unwrap().clone())
            .filter(|k| self.decl.contains_key(k))
            .collect::<Vec<_>>())
    }

    fn emit_code(&self, keys: &[EntityKey]) -> Result<Vec<u8>> {
        self.check_timeout()?;
        let mut out_text = Vec::new();

        if !self.ignored.is_empty() {
            out_text.extend_from_slice(b"// Ignored functions:\n// ");
            let ignored: String = self
                .ignored
                .iter()
                .filter(|k| k.name.is_some())
                .map(|k| k.name.as_ref().unwrap().clone())
                .collect::<Vec<_>>()
                .join(", ");
            out_text.extend_from_slice(ignored.as_bytes());
            out_text.extend_from_slice(b"\n\n");
        }

        for key in &self.includes {
            out_text.extend_from_slice(key.as_bytes());
            out_text.extend_from_slice(b"\n");
        }

        for m in &self.macros {
            out_text.extend_from_slice(b"#define ");
            out_text.extend_from_slice(m);
            out_text.extend_from_slice(b"\n");
        }

        for key in keys {
            if let Some(entity) = self.decl.get(key) {
                out_text.extend_from_slice(&entity.extract_code()?);
                out_text.extend_from_slice(b"\n\n");
            }
        }
        Ok(out_text)
    }
}

pub fn run(
    input_file_path: &str,
    output: Option<&str>,
    target: &str,
    tokens_file: &str,
    seed: u64,
    overwrite: bool,
    thread: usize,
    timeout: u64,
    logger: &Logger,
) -> Result<()> {
    // Open the input file and filter out duplicate ids
    let input_df = logger.run_task("Loading input file and filtering duplicates", || {
        open_csv(
            input_file_path,
            Some(Schema::from_iter(vec![
                Field::new("id".into(), DataType::UInt32),
                Field::new("name".into(), DataType::String),
                Field::new("latest_commit".into(), DataType::String),
            ])),
            Some(vec!["id", "name", "latest_commit"]),
        )
    })?;

    let id_col = dataframes::u32(&input_df, "id")?;

    let mut unique_ids: HashSet<u32> = HashSet::new();
    let mut mask: Vec<bool> = Vec::new();

    for id in id_col {
        mask.push(!unique_ids.contains(&id));
        unique_ids.insert(id);
    }

    let mut input_file = input_df
        .filter(&mask.into_iter().collect::<ChunkedArray<BooleanType>>())
        .with_context(|| format!("Could not filter input file {input_file_path}"))?;

    let project_input = format!("{}/{}", target, "tmp_in.csv");

    write_csv(&project_input, &mut input_file)?;

    let projects_output = format!("{}/{}", target, "tmp_out.csv");

    crate::phases::download::run(
        &project_input,
        Some(&projects_output),
        None,
        target,
        Some(tokens_file),
        &["keywords/c_files.json"],
        false,
        false,
        false,
        seed,
        logger,
        thread,
        "sequential",
    )?;

    let projects_df: DataFrame = logger.run_task("Loading downloaded projects", || {
        open_csv(
            &projects_output,
            Some(Schema::from_iter(vec![
                Field::new("id".into(), DataType::UInt32),
                Field::new("path".into(), DataType::String),
            ])),
            Some(vec!["id", "path"]),
        )
    })?;

    let id_to_projects: HashMap<u32, &str> = {
        let ids = dataframes::u32(&projects_df, "id")?;
        let paths = dataframes::str(&projects_df, "path")?;
        ids.into_iter().zip(paths).collect()
    };

    let input_file: DataFrame = logger.run_task("Loading input file for extra", || {
        open_csv(
            input_file_path,
            Some(Schema::from_iter(vec![
                Field::new("id".into(), DataType::UInt32),
                Field::new("path".into(), DataType::String),
                Field::new("function".into(), DataType::String),
            ])),
            Some(vec!["id", "path", "function"]),
        )
    })?;

    let n_fun = input_file.height();

    let mut shuffled_idx = (0..n_fun).collect::<Vec<usize>>();

    // Load the ids from the input file in random order.
    logger.run_task("Loading functions in random order", || {
        let mut rng: StdRng = SeedableRng::seed_from_u64(seed);
        shuffled_idx.shuffle(&mut rng);
        Ok(())
    })?;

    let path_prefix_stripper = Regex::new(r"^.*?[0-9]+-[0-9a-fA-F]{40}/")?;
    let path_suffix_stripper = Regex::new(r"\.functions/\d+$")?;

    let shuffled_rows = shuffled_idx.into_iter().map(|idx| {
        let row = input_file.get_row(idx).unwrap().0;
        match (row[0].clone(), row[1].clone(), row[2].clone()) {
            (AnyValue::UInt32(id), AnyValue::String(path), AnyValue::String(function)) => {
                let path_no_prefix = path_prefix_stripper.replace(path, "").to_string();
                let path_no_suffix = path_suffix_stripper
                    .replace(&path_no_prefix, "")
                    .to_string();
                Ok((idx, id, path_no_suffix, function))
            }
            _ => Err(idx),
        }
    });

    let default_output_path = format!("{input_file_path}.benchmarks.csv");
    let output_path: &str = output.unwrap_or(&default_output_path);
    let mut output_file = CSVFile::new(
        output_path,
        if overwrite {
            FileMode::Overwrite
        } else {
            FileMode::Append
        },
    )?;

    const OUTPUT_FILE_COLS: usize = 4;

    let output_file_headers: [&str; OUTPUT_FILE_COLS] = ["id", "file", "function", "benchmark"];

    output_file.write_header(&output_file_headers)?;

    // Load the previous results.
    let previous_results: HashSet<(String, String)> = if overwrite {
        HashSet::new()
    } else {
        logger.run_task("Resuming progress (parsing)", || {
            if PathBuf::from(&output_path).exists() {
                let output_df: DataFrame = open_csv(
                    output_path,
                    Some(Schema::from_iter(vec![
                        Field::new("file".into(), DataType::String),
                        Field::new("function".into(), DataType::String),
                    ])),
                    Some(vec!["file", "function"]),
                )?;
                let file_col: Vec<&str> = dataframes::str(&output_df, "file")?;
                let function_col: Vec<&str> = dataframes::str(&output_df, "function")?;
                Ok(file_col
                    .into_iter()
                    .zip(function_col)
                    .map(|(f, func)| (f.to_string(), func.to_string()))
                    .collect::<HashSet<(String, String)>>())
            } else {
                Ok(HashSet::new())
            }
        })?
    };

    info!(
        "Resuming from {} previously extracted functions",
        previous_results.len()
    );

    // Create a progress bar
    let progress_bar: ProgressBar = ProgressBar::new(n_fun as u64);

    progress_bar.enable_steady_tick(Duration::from_millis(100));

    progress_bar.set_style(
        indicatif::ProgressStyle::default_bar().template("{elapsed} {wide_bar} {percent}%")?,
    );

    progress_bar.set_length(n_fun as u64);

    for row in shuffled_rows {
        match row {
            Ok((_, id, rel_path, function)) => {
                let proj_path = id_to_projects
                    .get(&id)
                    .with_context(|| format!("Could not get project path for id {id}"))?;
                if *proj_path == "error" {
                    let csv_row = format!("{},{},{},{}", id, rel_path, function, "error");
                    writeln!(&mut output_file, "{csv_row}")?;
                } else {
                    let abs_path = format!("{proj_path}/{rel_path}");
                    let out_path = format!("{target}/benchmarks/{id}-{function}.c");
                    if !previous_results.contains(&(abs_path.clone(), function.to_owned())) {
                        info!(
                            "Extracting benchmark for function {} in file {}",
                            function, abs_path
                        );
                        match extract_root(proj_path, &abs_path, function, &out_path, timeout) {
                            Ok(()) => {
                                let csv_row = format!("{id},{abs_path},{function},{out_path}");
                                writeln!(&mut output_file, "{csv_row}")?;
                            }
                            Err(e) => {
                                let csv_row =
                                    format!("{},{},{},{}", id, abs_path, function, "error");
                                writeln!(&mut output_file, "{csv_row}")?;
                                warn!(
                                    "Could not extract benchmark for function {} in file {}:\n {}",
                                    function, abs_path, e
                                );
                            }
                        }
                    }
                }

                progress_bar.inc(1);
            }
            Err(idx) => {
                bail!("Could not parse row {idx} in the input file")
            }
        }
    }

    Ok(())
}

pub fn run_with_timeout<T>(dur: Duration, f: impl FnOnce() -> T + Send + 'static) -> Result<T>
where
    T: Send + 'static,
{
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let _ = tx.send(f()); // ignore send errors if receiver dropped
    });

    rx.recv_timeout(dur).with_context(|| "Operation timed out")
}

fn extract_root(
    project: &str,
    root_file: &str,
    root_name: &str,
    out_file: &str,
    timeout: u64,
) -> Result<()> {
    let project = check_path(project)?;
    let root_file = check_path(root_file)?;

    let clang = Clang::new().map_err(|_| anyhow!("Could not initialize Clang"))?;
    let mut ws = Workspace::new(clang, &project, &root_file, root_name, true, timeout)?;
    let entities = ws.resolve_dependencies()?;
    let code = ws.emit_code(&entities)?;
    write_file(out_file, &code)?;
    Ok(())
}

#[cfg(test)]
mod tests {

    use anyhow::ensure;
    use clang::TranslationUnit;

    use super::*;

    const TEST_DATA: &str = "tests/data/phases/extract_benchmarks";

    #[test]
    #[ignore]
    fn extract_benchmarks_test() -> Result<()> {
        const STACK_MAIN: &str = "main";
        const SIMPLE_MAIN: &str = "helper";
        const EXT_MAIN: &str = "main";
        const CONST_MAIN: &str = "add";
        const MACRO_MAIN: &str = "main";

        fn extract_code_test() -> Result<()> {
            let path = format!("{TEST_DATA}/simple/simple.c");
            let clang: Clang = Clang::new().map_err(|_| anyhow!("Could not initialize Clang"))?;
            let index: Index = Index::new(&clang, true, true);

            let tu: TranslationUnit = index.parser(&path).parse()?;
            let entity: Entity<'_> = tu.get_entity();
            let data = EntityData::from_entity(&entity)?;

            let code = data.extract_code()?;
            assert_eq!(code, std::fs::read(path)?);
            Ok(())
        }

        fn stack_workspace() -> Result<Workspace> {
            let clang: Clang = Clang::new().map_err(|_| anyhow!("Could not initialize Clang"))?;
            let project_root = PathBuf::from(format!("{TEST_DATA}/stack_project"));
            let root_file = project_root.join("stack.c");
            let root_function = STACK_MAIN;
            Workspace::new(clang, &project_root, &root_file, root_function, true, 5)
        }

        fn simple_workspace() -> Result<Workspace> {
            let clang: Clang = Clang::new().map_err(|_| anyhow!("Could not initialize Clang"))?;
            let project_root = PathBuf::from(format!("{TEST_DATA}/simple"));
            let root_file = project_root.join("simple.c");
            let root_function = "helper";
            Workspace::new(clang, &project_root, &root_file, root_function, true, 5)
        }

        fn ext_workspace() -> Result<Workspace> {
            let clang: Clang = Clang::new().map_err(|_| anyhow!("Could not initialize Clang"))?;
            let project_root = PathBuf::from(format!("{TEST_DATA}/ext"));
            let root_file = project_root.join("ext.c");
            let root_function = EXT_MAIN;
            Workspace::new(clang, &project_root, &root_file, root_function, true, 5)
        }

        fn const_workspace() -> Result<Workspace> {
            let clang: Clang = Clang::new().map_err(|_| anyhow!("Could not initialize Clang"))?;
            let project_root = PathBuf::from(format!("{TEST_DATA}/const"));
            let root_file = project_root.join("add.c");
            Workspace::new(clang, &project_root, &root_file, CONST_MAIN, true, 5)
        }

        fn macro_workspace() -> Result<Workspace> {
            let clang: Clang = Clang::new().map_err(|_| anyhow!("Could not initialize Clang"))?;
            let project_root = PathBuf::from(format!("{TEST_DATA}/macro"));
            let root_file = project_root.join("abs.c");
            Workspace::new(clang, &project_root, &root_file, MACRO_MAIN, true, 5)
        }

        fn workspace_new_test() -> Result<()> {
            let ws = stack_workspace()?;
            assert_eq!(ws.root_function_name, STACK_MAIN);
            assert_eq!(ws.candidates.len(), 1);
            assert_eq!(
                ws.candidates[0],
                PathBuf::from(format!("{TEST_DATA}/stack_project/stack.c"))
            );
            ensure!(ws.decl.is_empty());
            assert_eq!(ws.dependencies.node_count(), 0);
            assert_eq!(ws.dependencies.edge_count(), 0);
            ensure!(ws.node_indices.is_empty());
            Ok(())
        }

        fn workspace_index_file_test() -> Result<()> {
            let file = PathBuf::from(format!("{TEST_DATA}/stack_project/stack.c"));
            let mut ws = stack_workspace()?;
            ws.index_file(&file, None)?;
            assert_eq!(ws.root_function_name, STACK_MAIN);
            assert_eq!(ws.candidates.len(), 1);
            assert_eq!(ws.candidates[0], file);
            assert_eq!(ws.decl.len(), 14);
            assert_eq!(ws.dependencies.node_count(), 0);
            assert_eq!(ws.dependencies.edge_count(), 0);
            ensure!(ws.node_indices.is_empty());
            Ok(())
        }

        fn workspace_discover_candidates_test() -> Result<()> {
            let key = {
                let path = format!("{TEST_DATA}/stack_project/stack.c");
                let clang: Clang =
                    Clang::new().map_err(|_| anyhow!("Could not initialize Clang"))?;
                let index: Index = Index::new(&clang, true, true);

                let tu: TranslationUnit = index.parser(&path).parse()?;
                let entity: Entity<'_> = tu.get_entity().get_children()[0];
                EntityKey::from_entity(&entity)
            };

            let mut ws = stack_workspace()?;
            ws.discover_candidates(&key)?;
            assert_eq!(ws.root_function_name, STACK_MAIN);
            ensure!(ws.candidates.is_empty());
            ensure!(ws.decl.contains_key(&key));
            assert_eq!(ws.dependencies.node_count(), 0);
            assert_eq!(ws.dependencies.edge_count(), 0);
            ensure!(ws.node_indices.is_empty());
            Ok(())
        }

        fn workspace_discover_root_stack_test() -> Result<()> {
            let mut ws = stack_workspace()?;
            ws.discover_root()?;
            assert_eq!(ws.root_function_name, STACK_MAIN);
            ensure!(ws.candidates.is_empty());
            ensure!(ws
                .decl
                .keys()
                .any(|k| k.name.as_deref() == Some(STACK_MAIN)));
            assert_eq!(ws.dependencies.node_count(), 0);
            assert_eq!(ws.dependencies.edge_count(), 0);
            ensure!(ws.node_indices.is_empty());
            Ok(())
        }

        fn workspace_discover_root_ext_test() -> Result<()> {
            let mut ws = ext_workspace()?;
            ws.discover_root()?;
            assert_eq!(ws.root_function_name, EXT_MAIN);
            ensure!(ws.candidates.is_empty());
            ensure!(ws.decl.keys().any(|k| k.name.as_deref() == Some(EXT_MAIN)));
            assert_eq!(ws.decl.len(), 1);
            assert_eq!(ws.dependencies.node_count(), 0);
            assert_eq!(ws.dependencies.edge_count(), 0);
            ensure!(ws.node_indices.is_empty());
            Ok(())
        }

        fn workspace_discover_root_const_test() -> Result<()> {
            let mut ws = const_workspace()?;
            ws.discover_root()?;
            assert_eq!(ws.root_function_name, CONST_MAIN);
            ensure!(ws.candidates.is_empty());
            ensure!(ws
                .decl
                .keys()
                .any(|k| k.name.as_deref() == Some(CONST_MAIN)));
            assert_eq!(ws.decl.len(), 1);
            assert_eq!(ws.dependencies.node_count(), 0);
            assert_eq!(ws.dependencies.edge_count(), 0);
            ensure!(ws.node_indices.is_empty());
            Ok(())
        }

        fn workspace_discover_root_macro_test() -> Result<()> {
            let mut ws = macro_workspace()?;
            ws.discover_root()?;
            assert_eq!(ws.root_function_name, MACRO_MAIN);
            ensure!(ws.candidates.is_empty());
            ensure!(ws
                .decl
                .keys()
                .any(|k| k.name.as_deref() == Some(MACRO_MAIN)));
            assert_eq!(ws.decl.len(), 1);
            assert_eq!(ws.macros.len(), 1);
            assert_eq!(ws.dependencies.node_count(), 0);
            assert_eq!(ws.dependencies.edge_count(), 0);
            ensure!(ws.node_indices.is_empty());
            Ok(())
        }

        fn workspace_add_node_test() -> Result<()> {
            let key = {
                let path = format!("{TEST_DATA}/stack_project/stack.c");
                let clang: Clang =
                    Clang::new().map_err(|_| anyhow!("Could not initialize Clang"))?;
                let index: Index = Index::new(&clang, true, true);

                let tu: TranslationUnit = index.parser(&path).parse()?;
                let entity: Entity<'_> = tu.get_entity().get_children()[0];
                EntityKey::from_entity(&entity)
            };

            let mut ws = stack_workspace()?;
            ws.add_node(&key)?;
            assert_eq!(ws.root_function_name, STACK_MAIN);
            assert_eq!(ws.candidates.len(), 1);
            ensure!(ws.decl.is_empty());
            assert_eq!(ws.dependencies.node_count(), 1);
            assert_eq!(ws.dependencies.edge_count(), 0);
            assert_eq!(ws.node_indices.len(), 1);
            ensure!(ws.node_indices.contains_key(&key));
            ensure!(ws
                .dependencies
                .node_indices()
                .all(|idx| idx == ws.node_indices[&key]));
            Ok(())
        }

        fn workspace_add_edge_test() -> Result<()> {
            let keys = {
                let path = format!("{TEST_DATA}/stack_project/stack.c");
                let clang: Clang =
                    Clang::new().map_err(|_| anyhow!("Could not initialize Clang"))?;
                let index: Index = Index::new(&clang, true, true);

                let tu: TranslationUnit = index.parser(&path).parse()?;
                let entity: Entity<'_> = tu.get_entity().get_children()[0];
                let key1 = EntityKey::from_entity(&entity);
                let entity2: Entity<'_> = tu.get_entity().get_children()[1];
                let key2 = EntityKey::from_entity(&entity2);
                (key1, key2)
            };

            let mut ws = stack_workspace()?;
            ws.add_node(&keys.0)?;
            ws.add_node(&keys.1)?;
            ws.add_edge(&keys.0, &keys.1)?;
            assert_eq!(ws.root_function_name, STACK_MAIN);
            assert_eq!(ws.candidates.len(), 1);
            ensure!(ws.decl.is_empty());
            assert_eq!(ws.dependencies.node_count(), 2);
            assert_eq!(ws.dependencies.edge_count(), 1);
            assert_eq!(ws.node_indices.len(), 2);
            ensure!(ws.node_indices.contains_key(&keys.0));
            ensure!(ws.node_indices.contains_key(&keys.1));
            ensure!(ws
                .dependencies
                .node_indices()
                .all(|idx| idx == ws.node_indices[&keys.0] || idx == ws.node_indices[&keys.1]));
            Ok(())
        }

        fn workspace_explore_entity_test() -> Result<()> {
            let mut ws = stack_workspace()?;
            let key = ws.discover_root()?.clone();
            let mut explored: HashSet<EntityKey> = HashSet::new();
            let mut to_explore: VecDeque<EntityKey> = VecDeque::new();
            let decl_before = ws.decl.clone();
            ws.explore_entity(&key, &mut explored, &mut to_explore)?;
            assert_eq!(ws.root_function_name, STACK_MAIN);
            ensure!(ws.candidates.is_empty());
            assert_eq!(ws.decl, decl_before);
            assert!(explored == HashSet::from([key]));
            ensure!(!to_explore.is_empty());
            Ok(())
        }

        fn workspace_explore_entity_ext_test() -> Result<()> {
            let mut ws = ext_workspace()?;
            let key = ws.discover_root()?.clone();
            let mut explored: HashSet<EntityKey> = HashSet::new();
            let mut to_explore: VecDeque<EntityKey> = VecDeque::new();
            let decl_before = ws.decl.clone();
            ws.explore_entity(&key, &mut explored, &mut to_explore)?;
            assert_eq!(ws.root_function_name, EXT_MAIN);
            ensure!(ws.candidates.is_empty());
            assert_eq!(ws.decl, decl_before);
            assert!(explored == HashSet::from([key.clone()]));

            let ignore1 = to_explore
                .pop_front()
                .with_context(|| "To explore is empty")?;

            ws.explore_entity(&ignore1, &mut explored, &mut to_explore)?;
            assert_eq!(ws.root_function_name, EXT_MAIN);
            ensure!(ws.candidates.is_empty());
            assert_eq!(ws.decl, decl_before);
            assert!(explored == HashSet::from([key, ignore1]));
            Ok(())
        }

        fn workspace_resolve_dependencies_simple_test() -> Result<()> {
            let mut ws = simple_workspace()?;
            let dependencies = ws.resolve_dependencies()?;
            assert_eq!(dependencies.len(), 3);
            Ok(())
        }

        fn workspace_resolve_dependencies_ext_test() -> Result<()> {
            let mut ws = ext_workspace()?;
            let dependencies = ws.resolve_dependencies()?;
            assert_eq!(dependencies.len(), 1);
            Ok(())
        }

        fn workspace_emit_code_simple_test() -> Result<()> {
            let mut ws = simple_workspace()?;
            let dependencies = ws.resolve_dependencies()?;
            let code = ws.emit_code(&dependencies)?;
            let expected = std::fs::read(format!("{TEST_DATA}/simple_expected.c"))?;
            assert_eq!(code.trim_ascii(), expected);
            Ok(())
        }

        fn run_simple_test() -> Result<()> {
            let project_root = format!("{TEST_DATA}/simple");
            let root_file = format!("{project_root}/simple.c");
            let root_function = SIMPLE_MAIN;
            let out_path_str = format!("{TEST_DATA}/simple_out.c");
            delete_file(&out_path_str, true)?;
            extract_root(&project_root, &root_file, root_function, &out_path_str, 5)?;
            let out_path = check_path(&out_path_str)?;
            let out_content = std::fs::read(&out_path)?;
            let expected = std::fs::read(format!("{TEST_DATA}/simple_expected.c"))?;
            assert_eq!(out_content.trim_ascii(), expected);
            std::fs::remove_file(&out_path_str)?;
            Ok(())
        }

        fn run_with_make_test() -> Result<()> {
            let project_root = format!("{TEST_DATA}/with_make");
            let root_file = format!("{project_root}/main.c");
            let root_function = "main";
            let out_path_str = format!("{TEST_DATA}/with_make_out.c");
            delete_file(&out_path_str, true)?;
            extract_root(&project_root, &root_file, root_function, &out_path_str, 5)?;
            let out_path = check_path(&out_path_str)?;
            let out_content = std::fs::read(&out_path)?;
            let expected = std::fs::read(format!("{TEST_DATA}/with_make_expected.c"))?;
            assert_eq!(
                String::from_utf8_lossy(out_content.trim_ascii()),
                String::from_utf8_lossy(&expected)
            );
            std::fs::remove_file(&out_path_str)?;
            Ok(())
        }

        fn run_ext_test() -> Result<()> {
            let project_root = format!("{TEST_DATA}/ext");
            let root_file = format!("{project_root}/ext.c");
            let root_function = EXT_MAIN;
            let out_path_str = format!("{TEST_DATA}/ext_out.c");
            delete_file(&out_path_str, true)?;
            extract_root(&project_root, &root_file, root_function, &out_path_str, 5)?;
            let out_path = check_path(&out_path_str)?;
            let out_content = String::from_utf8_lossy(std::fs::read(&out_path)?.trim_ascii())
                .lines()
                .skip(2)
                .collect::<Vec<_>>()
                .join("\n");
            let out_content = out_content.trim();
            let expected = std::fs::read(format!("{TEST_DATA}/ext_expected.c"))?;
            assert_eq!(out_content, String::from_utf8_lossy(&expected));
            std::fs::remove_file(&out_path_str)?;
            Ok(())
        }

        fn run_macro_test() -> Result<()> {
            let project_root = format!("{TEST_DATA}/macro");
            let root_file = format!("{project_root}/abs.c");
            let root_function = MACRO_MAIN;
            let out_path_str = format!("{TEST_DATA}/macro_out.c");
            delete_file(&out_path_str, true)?;
            extract_root(&project_root, &root_file, root_function, &out_path_str, 5)?;
            let out_path = check_path(&out_path_str)?;
            let out_content = std::fs::read(&out_path)?;
            let out_content = out_content.trim_ascii();
            let expected = std::fs::read(format!("{TEST_DATA}/macro_expected.c"))?;
            assert_eq!(
                String::from_utf8_lossy(out_content),
                String::from_utf8_lossy(&expected)
            );
            std::fs::remove_file(&out_path_str)?;
            Ok(())
        }

        extract_code_test()?;
        workspace_new_test()?;
        workspace_index_file_test()?;
        workspace_discover_candidates_test()?;
        workspace_discover_root_stack_test()?;
        workspace_discover_root_ext_test()?;
        workspace_discover_root_const_test()?;
        workspace_discover_root_macro_test()?;
        workspace_add_node_test()?;
        workspace_add_edge_test()?;
        workspace_add_node_test()?;
        workspace_explore_entity_test()?;
        workspace_explore_entity_ext_test()?;
        workspace_resolve_dependencies_simple_test()?;
        workspace_resolve_dependencies_ext_test()?;
        workspace_emit_code_simple_test()?;
        run_simple_test()?;
        run_with_make_test()?;
        run_ext_test()?;
        run_macro_test()
    }
}
