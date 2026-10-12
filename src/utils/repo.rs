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

//! Repositories, files and functions, and the CSV columns that describe them.

use anyhow::{bail, ensure, Context, Result};
use polars::prelude::{DataFrame, DataType, Field};
use std::fmt;
use std::path::Path;
use std::str::FromStr;

use super::github::{github_url, is_zipball_root};
use super::github_api::{HTTP_ERROR_PREFIX, OTHER_ERROR_PREFIX};

/// Column with the local path of a repository or a file.
pub const LOCAL_PATH: &str = "local_path";
/// Column with the path of a file inside its repository.
pub const FILE: &str = "file";
/// Column with the GitHub link to a repository, a file or a function.
pub const URL: &str = "url";
/// Column with the name of a function.
pub const FUNCTION_NAME: &str = "function_name";
/// Column with the line where a function starts, from 1.
pub const LINE: &str = "line";
/// Column with the column where a function starts, from 1.
pub const COLUMN: &str = "column";
/// Column with the number of a pull request.
pub const PR_NUMBER: &str = "pr_number";

/// Where a repository comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    /// Hosted on GitHub.
    GitHub,
    /// Stored on the local file system.
    Local,
}

/// Writes the value of the `source` column.
impl fmt::Display for Source {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Source::GitHub => "github",
            Source::Local => "local",
        })
    }
}

impl FromStr for Source {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self> {
        match s.to_lowercase().as_str() {
            "github" => Ok(Source::GitHub),
            "local" => Ok(Source::Local),
            _ => bail!("Unknown source '{s}', expected 'github' or 'local'"),
        }
    }
}

/// A column describing a repository.
/// The variants are in the order of the columns in the files.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum RepoColumn {
    /// GitHub id.
    RepoId,
    /// Where the repository comes from.
    Source,
    /// Full name (`owner/repository`).
    RepoName,
    /// License.
    RepoLicense,
    /// Commit SHA.
    LatestCommit,
}

impl RepoColumn {
    /// All the repository columns, in order.
    pub const ALL: [Self; 5] = [
        Self::RepoId,
        Self::Source,
        Self::RepoName,
        Self::RepoLicense,
        Self::LatestCommit,
    ];

    /// Returns the name of the column.
    pub const fn name(self) -> &'static str {
        match self {
            Self::RepoId => "repo_id",
            Self::Source => "source",
            Self::RepoName => "repo_name",
            Self::RepoLicense => "repo_license",
            Self::LatestCommit => "latest_commit",
        }
    }

    /// Returns the names and types of all the repository columns.
    /// Ids are integers. The other columns are strings, even commit SHAs that look like numbers.
    pub fn fields() -> Vec<Field> {
        Self::ALL
            .map(|column| {
                let dtype: DataType = match column {
                    Self::RepoId => DataType::UInt32,
                    Self::Source | Self::RepoName | Self::RepoLicense | Self::LatestCommit => {
                        DataType::String
                    }
                };
                Field::new(column.name().into(), dtype)
            })
            .to_vec()
    }
}

impl fmt::Display for RepoColumn {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// A unit of analysis: a repository, a file or a function.
pub trait Unit {
    /// Returns the names of the unit's columns, without its link.
    fn columns() -> Vec<&'static str>;

    /// Returns the values of the unit's columns, without its link. Unknown values are empty.
    fn values(&self) -> Vec<String>;

    /// Returns the GitHub link to the unit, if it is known.
    fn url(&self) -> Option<String>;

    /// Returns the header of an output file: the unit's columns, then `url`.
    fn header() -> Vec<&'static str> {
        let mut header: Vec<&'static str> = Self::columns();
        header.push(URL);
        header
    }

    /// Returns the row of the unit in an output file: its values, then its link, empty if unknown.
    fn row(&self) -> Vec<String> {
        let mut row: Vec<String> = self.values();
        row.push(self.url().unwrap_or_default());
        row
    }
}

/// A repository.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Repo {
    /// GitHub id.
    pub id: Option<u32>,
    /// Where the repository comes from.
    pub source: Source,
    /// Full name (`owner/repository`).
    pub name: Option<String>,
    /// License.
    pub license: Option<String>,
    /// Commit SHA.
    pub commit: Option<String>,
}

impl Repo {
    /// Reads the repository in a row of a dataframe.
    /// Missing columns and empty cells are unknown values.
    /// Fails if the row has no `source`, unless `infer_source` is set.
    ///
    /// # Arguments
    ///
    /// * `df` - Dataframe with the repository columns.
    /// * `row` - Index of the row.
    /// * `infer_source` - Whether a missing source is inferred: GitHub if the row has an id, and local otherwise.
    pub fn from_csv_row(df: &DataFrame, row: usize, infer_source: bool) -> Result<Self> {
        let string = |column: RepoColumn| -> Result<Option<String>> {
            let Ok(values) = df.column(column.name()) else {
                return Ok(None);
            };
            Ok(values
                .str()
                .with_context(|| format!("Column {column} does not contain strings"))?
                .get(row)
                .filter(|s| !s.is_empty())
                .map(str::to_string))
        };
        let id: Option<u32> = match df.column(RepoColumn::RepoId.name()) {
            Ok(values) => values
                .u32()
                .with_context(|| format!("Column {} does not contain ids", RepoColumn::RepoId))?
                .get(row),
            Err(_) => None,
        };
        let source: Source = match string(RepoColumn::Source)? {
            Some(source) => source
                .parse()
                .with_context(|| format!("Could not parse row {row}"))?,
            None if infer_source && id.is_some() => Source::GitHub,
            None if infer_source => Source::Local,
            None => bail!("Row {row} has no {}", RepoColumn::Source),
        };
        Ok(Self {
            id,
            source,
            name: string(RepoColumn::RepoName)?,
            license: string(RepoColumn::RepoLicense)?,
            commit: string(RepoColumn::LatestCommit)?,
        })
    }

    /// Returns the error that phases using the GitHub API write in `repo_name` for a repository not on GitHub.
    pub fn unsupported_source(&self) -> String {
        format!("{OTHER_ERROR_PREFIX} unsupported source {}", self.source)
    }

    /// Returns the error that an earlier step wrote in `repo_name` instead of the name, if any.
    pub fn error(&self) -> Option<&str> {
        self.name.as_deref().filter(|name| {
            name.starts_with(HTTP_ERROR_PREFIX) || name.starts_with(OTHER_ERROR_PREFIX)
        })
    }

    /// Returns the GitHub link to a page of the repository: its name, then `path`.
    /// There is no link if the repository is not on GitHub, or if its name is unknown or an error.
    ///
    /// # Arguments
    ///
    /// * `path` - Segments of the path of the page after the name.
    fn link<'a>(&'a self, path: impl IntoIterator<Item = &'a str>) -> Option<String> {
        let name: &str = self
            .name
            .as_deref()
            .filter(|_| self.source == Source::GitHub && self.error().is_none())?;
        Some(github_url(name.split('/').chain(path)))
    }

    /// Returns the value of a column for this repository. Unknown values are empty.
    ///
    /// # Arguments
    ///
    /// * `column` - The column.
    pub fn value(&self, column: RepoColumn) -> String {
        match column {
            RepoColumn::RepoId => self.id.map(|id| id.to_string()).unwrap_or_default(),
            RepoColumn::Source => self.source.to_string(),
            RepoColumn::RepoName => self.name.clone().unwrap_or_default(),
            RepoColumn::RepoLicense => self.license.clone().unwrap_or_default(),
            RepoColumn::LatestCommit => self.commit.clone().unwrap_or_default(),
        }
    }
}

impl Unit for Repo {
    /// All the repository columns.
    fn columns() -> Vec<&'static str> {
        RepoColumn::ALL.map(RepoColumn::name).to_vec()
    }

    fn values(&self) -> Vec<String> {
        RepoColumn::ALL.map(|column| self.value(column)).to_vec()
    }

    /// Links to the commit if known, and to the default branch otherwise.
    fn url(&self) -> Option<String> {
        match &self.commit {
            Some(commit) => self.link(["tree", commit]),
            None => self.link([]),
        }
    }
}

/// A file in a repository.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepoFile {
    /// Repository of the file.
    pub repo: Repo,
    /// Local path of the file.
    pub local_path: String,
    /// Path of the file inside its repository, with `/` separators.
    pub path_in_repo: String,
}

impl RepoFile {
    /// Returns a file of a repository stored in `project_dir`.
    /// Its path inside the repository has `/` separators, without the root folder of GitHub zip archives.
    /// Fails if the file is not in `project_dir`.
    ///
    /// # Arguments
    ///
    /// * `repo` - Repository of the file.
    /// * `path` - Local path of the file.
    /// * `project_dir` - Directory of the repository.
    pub fn from_disk(repo: Repo, path: &Path, project_dir: &Path) -> Result<Self> {
        let not_in_repo = || {
            format!(
                "{} is not in the directory {} of its repository",
                path.display(),
                project_dir.display()
            )
        };
        let mut parts: Vec<String> = path
            .strip_prefix(project_dir)
            .with_context(not_in_repo)?
            .components()
            .map(|c| c.as_os_str().to_string_lossy().into_owned())
            .collect();
        if repo.source == Source::GitHub && parts.len() > 1 && is_zipball_root(&parts[0]) {
            parts.remove(0);
        }
        ensure!(!parts.is_empty(), not_in_repo());
        Ok(Self {
            local_path: path.to_string_lossy().into_owned(),
            path_in_repo: parts.join("/"),
            repo,
        })
    }

    /// Reads the file in a row of a dataframe with the columns `local_path` and `file`.
    ///
    /// # Arguments
    ///
    /// * `df` - The dataframe.
    /// * `row` - Index of the row.
    /// * `infer_source` - Whether a missing source is inferred (see [`Repo::from_csv_row`]).
    pub fn from_csv_row(df: &DataFrame, row: usize, infer_source: bool) -> Result<Self> {
        let string = |column: &str| -> Result<String> {
            df.column(column)?
                .str()
                .with_context(|| format!("Column {column} does not contain strings"))?
                .get(row)
                .filter(|s| !s.is_empty())
                .map(str::to_string)
                .with_context(|| format!("Row {row} has no {column}"))
        };
        Ok(Self {
            repo: Repo::from_csv_row(df, row, infer_source)?,
            local_path: string(LOCAL_PATH)?,
            path_in_repo: string(FILE)?,
        })
    }
}

impl Unit for RepoFile {
    /// Repository columns, then `local_path` and `file`.
    fn columns() -> Vec<&'static str> {
        let mut columns: Vec<&'static str> = Repo::columns();
        columns.extend([LOCAL_PATH, FILE]);
        columns
    }

    fn values(&self) -> Vec<String> {
        let mut values: Vec<String> = self.repo.values();
        values.extend([self.local_path.clone(), self.path_in_repo.clone()]);
        values
    }

    /// Links to the file at the commit of its repository if known, and on the default branch otherwise.
    fn url(&self) -> Option<String> {
        let commit: &str = self.repo.commit.as_deref().unwrap_or("HEAD");
        self.repo.link(
            ["blob", commit]
                .into_iter()
                .chain(self.path_in_repo.split('/')),
        )
    }
}

/// A function in a file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Function<'a> {
    /// File of the function.
    pub file: &'a RepoFile,
    /// Name of the function. Empty for anonymous functions.
    pub name: String,
    /// Line where the function starts, from 1.
    pub line: usize,
    /// Column where the function starts, from 1.
    pub column: usize,
}

impl Unit for Function<'_> {
    /// File columns, then `function_name`, `line` and `column`.
    fn columns() -> Vec<&'static str> {
        let mut columns: Vec<&'static str> = RepoFile::columns();
        columns.extend([FUNCTION_NAME, LINE, COLUMN]);
        columns
    }

    fn values(&self) -> Vec<String> {
        let mut values: Vec<String> = self.file.values();
        values.extend([
            self.name.clone(),
            self.line.to_string(),
            self.column.to_string(),
        ]);
        values
    }

    /// Links to the first line of the function.
    fn url(&self) -> Option<String> {
        Some(format!("{}#L{}", self.file.url()?, self.line))
    }
}

/// A pull request of a repository.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PullRequest<'a> {
    /// Repository of the pull request.
    pub repo: &'a Repo,
    /// Number of the pull request.
    pub number: u32,
}

impl Unit for PullRequest<'_> {
    /// Repository columns, then `pr_number`.
    fn columns() -> Vec<&'static str> {
        let mut columns: Vec<&'static str> = Repo::columns();
        columns.push(PR_NUMBER);
        columns
    }

    fn values(&self) -> Vec<String> {
        let mut values: Vec<String> = self.repo.values();
        values.push(self.number.to_string());
        values
    }

    /// Links to the pull request.
    fn url(&self) -> Option<String> {
        let number: String = self.number.to_string();
        self.repo.link(["pull", number.as_str()])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use polars::df;

    fn repo(source: Source, name: Option<&str>, commit: Option<&str>) -> Repo {
        Repo {
            id: Some(1),
            source,
            name: name.map(str::to_string),
            license: None,
            commit: commit.map(str::to_string),
        }
    }

    #[test]
    fn source_parsing() -> Result<()> {
        assert_eq!("github".parse::<Source>()?, Source::GitHub);
        assert_eq!("GitHub".parse::<Source>()?, Source::GitHub);
        assert_eq!("local".parse::<Source>()?, Source::Local);
        assert!("bitbucket".parse::<Source>().is_err());
        Ok(())
    }

    #[test]
    fn columns_are_in_declaration_order() {
        let mut sorted = RepoColumn::ALL;
        sorted.sort();
        assert_eq!(sorted, RepoColumn::ALL);
    }

    /// Returns a file of a repository at a path inside the repository.
    fn file(repo: Repo, path_in_repo: &str) -> RepoFile {
        RepoFile {
            repo,
            local_path: format!("dest/{path_in_repo}"),
            path_in_repo: path_in_repo.to_string(),
        }
    }

    #[test]
    fn urls() {
        let pinned = repo(Source::GitHub, Some("owner/repo"), Some("abc123"));
        assert_eq!(
            pinned.url().as_deref(),
            Some("https://github.com/owner/repo/tree/abc123")
        );
        assert_eq!(
            file(pinned, "src/main.c").url().as_deref(),
            Some("https://github.com/owner/repo/blob/abc123/src/main.c")
        );

        let unpinned = repo(Source::GitHub, Some("owner/repo"), None);
        assert_eq!(
            unpinned.url().as_deref(),
            Some("https://github.com/owner/repo")
        );
        assert_eq!(
            file(unpinned, "main.c").url().as_deref(),
            Some("https://github.com/owner/repo/blob/HEAD/main.c")
        );

        assert_eq!(repo(Source::GitHub, None, Some("abc")).url(), None);
        assert_eq!(
            file(
                repo(Source::Local, Some("owner/repo"), Some("abc")),
                "main.c"
            )
            .url(),
            None
        );
    }

    #[test]
    fn errors_have_no_link() {
        for error in ["http/2 404", "error: unsupported source local"] {
            let failed = repo(Source::GitHub, Some(error), Some("abc"));
            assert_eq!(failed.error(), Some(error));
            assert_eq!(failed.url(), None);
            assert_eq!(
                PullRequest {
                    repo: &failed,
                    number: 3
                }
                .url(),
                None
            );
            assert_eq!(file(failed, "main.c").url(), None);
        }
        assert_eq!(repo(Source::GitHub, Some("owner/repo"), None).error(), None);
    }

    #[test]
    fn urls_are_percent_encoded() {
        assert_eq!(
            file(
                repo(Source::GitHub, Some("owner/repo"), Some("abc")),
                "my dir/a#b?.c"
            )
            .url()
            .as_deref(),
            Some("https://github.com/owner/repo/blob/abc/my%20dir/a%23b%3F.c")
        );
    }

    #[test]
    fn paths_in_repo() -> Result<()> {
        let path_in_repo = |source: Source, path: &Path, project: &Path| -> Result<String> {
            Ok(
                RepoFile::from_disk(repo(source, Some("owner/repo"), None), path, project)?
                    .path_in_repo,
            )
        };
        let project = Path::new("dest/0/1-38d21aa");
        let in_zipball = project
            .join("owner-repo-38d21aa")
            .join("src")
            .join("main.c");
        assert_eq!(
            path_in_repo(Source::GitHub, &in_zipball, project)?,
            "src/main.c"
        );
        // A clone of a GitHub repository has no root folder to remove.
        assert_eq!(
            path_in_repo(Source::GitHub, &project.join("src").join("main.c"), project)?,
            "src/main.c"
        );
        assert_eq!(
            path_in_repo(Source::Local, &in_zipball, project)?,
            "owner-repo-38d21aa/src/main.c"
        );
        assert!(path_in_repo(Source::Local, Path::new("elsewhere/main.c"), project).is_err());
        Ok(())
    }

    #[test]
    fn columns_and_values() -> Result<()> {
        let df = df!(
            "repo_id" => [1u32, 2],
            "source" => ["github", "github"],
            "repo_name" => [Some("owner/repo"), None],
            "stars" => [5, 3],
            "latest_commit" => [Some("abc"), None],
        )?;
        assert_eq!(
            Repo::header(),
            vec![
                "repo_id",
                "source",
                "repo_name",
                "repo_license",
                "latest_commit",
                "url"
            ]
        );

        let first = Repo::from_csv_row(&df, 0, false)?;
        assert_eq!(first, repo(Source::GitHub, Some("owner/repo"), Some("abc")));
        assert_eq!(
            first.row(),
            vec![
                "1",
                "github",
                "owner/repo",
                "",
                "abc",
                "https://github.com/owner/repo/tree/abc"
            ]
        );

        // Missing values are written as empty cells.
        let second = Repo::from_csv_row(&df, 1, false)?;
        assert_eq!(second.row(), vec!["2", "github", "", "", "", ""]);
        Ok(())
    }

    #[test]
    fn missing_sources() -> Result<()> {
        let df = df!(
            "repo_id" => [Some(1u32), None, None],
            "source" => [None, None, Some("github")],
        )?;
        assert!(Repo::from_csv_row(&df, 0, false).is_err());
        assert_eq!(Repo::from_csv_row(&df, 0, true)?.source, Source::GitHub);
        assert_eq!(Repo::from_csv_row(&df, 1, true)?.source, Source::Local);
        assert_eq!(Repo::from_csv_row(&df, 2, false)?.source, Source::GitHub);
        Ok(())
    }

    #[test]
    fn file_values() -> Result<()> {
        let file = RepoFile::from_disk(
            repo(Source::GitHub, Some("owner/repo"), Some("abc")),
            &Path::new("dest").join("owner-repo-38d21aa").join("main.c"),
            Path::new("dest"),
        )?;
        assert_eq!(file.path_in_repo, "main.c");
        assert_eq!(
            RepoFile::header(),
            vec![
                "repo_id",
                "source",
                "repo_name",
                "repo_license",
                "latest_commit",
                "local_path",
                "file",
                "url"
            ]
        );
        assert_eq!(
            file.row(),
            vec![
                "1",
                "github",
                "owner/repo",
                "",
                "abc",
                file.local_path.as_str(),
                "main.c",
                "https://github.com/owner/repo/blob/abc/main.c"
            ]
        );
        // Files of repositories not on GitHub have no link.
        let local = RepoFile::from_disk(
            repo(Source::Local, Some("owner/repo"), None),
            &Path::new("dest").join("main.c"),
            Path::new("dest"),
        )?;
        assert_eq!(local.row().last().map(String::as_str), Some(""));
        Ok(())
    }

    #[test]
    fn files_are_read() -> Result<()> {
        let df = df!(
            "repo_id" => [1u32, 2, 3],
            "local_path" => [Some("dest/main.c"), None, Some("dest/lib.c")],
            "file" => [Some("main.c"), Some("main.c"), None],
        )?;
        let file = RepoFile::from_csv_row(&df, 0, true)?;
        assert_eq!(file.local_path, "dest/main.c");
        assert_eq!(file.path_in_repo, "main.c");
        // Every file has a path and a path inside its repository.
        assert!(RepoFile::from_csv_row(&df, 1, true).is_err());
        assert!(RepoFile::from_csv_row(&df, 2, true).is_err());
        Ok(())
    }

    #[test]
    fn functions() {
        let file = RepoFile {
            repo: repo(Source::GitHub, Some("owner/repo"), Some("abc")),
            local_path: "dest/main.c".to_string(),
            path_in_repo: "main.c".to_string(),
        };
        let function = Function {
            file: &file,
            name: "main".to_string(),
            line: 12,
            column: 5,
        };
        assert_eq!(
            Function::header()[5..],
            [
                "local_path",
                "file",
                "function_name",
                "line",
                "column",
                "url"
            ]
        );
        assert_eq!(
            function.row()[5..],
            [
                "dest/main.c",
                "main.c",
                "main",
                "12",
                "5",
                "https://github.com/owner/repo/blob/abc/main.c#L12"
            ]
        );
    }

    #[test]
    fn pull_requests() {
        let github = repo(Source::GitHub, Some("owner/repo"), Some("abc"));
        let pr = PullRequest {
            repo: &github,
            number: 3,
        };
        assert_eq!(PullRequest::header()[5..], ["pr_number", "url"]);
        assert_eq!(pr.row()[5..], ["3", "https://github.com/owner/repo/pull/3"]);
        let local = repo(Source::Local, Some("owner/repo"), None);
        assert_eq!(
            PullRequest {
                repo: &local,
                number: 3
            }
            .url(),
            None
        );
    }
}
