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

//! Utility functions for file operations and I/O.

use pathdiff::diff_paths;
use polars::io::SerWriter;
use polars::prelude::{CsvReadOptions, CsvWriter, Schema};
use polars::{frame::DataFrame, io::SerReader};
use walkdir::WalkDir;

use super::error::*;
use std::fs;
use std::io::BufWriter;
use std::path::{Component, PathBuf};
use std::sync::Arc;
use std::{
    fs::File,
    io::{BufRead, BufReader, Lines},
    path::Path,
};

#[derive(PartialEq, Eq, Debug, Copy, Clone)]
pub enum FileMode {
    Read,
    Overwrite,
    Append,
}

/// Opens a file. In overwrite or append mode, creates the file if it does not exist.
///
/// # Arguments
///
/// * `path` - The path to the file.
/// * `mode` - The mode to open the file in.
///
/// # Returns
///
/// A file in the specified mode or an error if the file could not be opened or created.
pub fn open_file(path: &str, mode: FileMode) -> Result<File, Error> {
    let file_path = Path::new(path);

    if let Some(parent) = file_path.parent() {
        if let Some(parent_path) = parent.to_str() {
            create_dir(parent_path)?;
        }
    }

    map_err(
        match mode {
            FileMode::Read => std::fs::File::open(path),
            FileMode::Overwrite => std::fs::OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .open(path),
            FileMode::Append => std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(path),
        },
        &format!("Could not open {}", path),
    )
}

pub fn check_path(path: &str) -> Result<PathBuf, Error> {
    if Path::new(path).exists() {
        Ok(PathBuf::from(path))
    } else {
        Error::new(&format!("File or directory {} not found", path)).to_res()
    }
}

// TODO : Test
/// Loads a file into memory if its size is less than a given limit.
///
/// # Arguments
///
/// * `path` - The path to the file.
/// * `memory_limit` - The maximum size of the file in bytes.
///
/// # Returns
///
/// An array of bytes containing the content of the file or an error if the file could not be read.
/// Two kinds of errors are possible:
/// * If the file size exceeds the memory limit, returns the size of the file.
/// * If the file could not be read, returns an error.
pub fn load_file(path: &str, memory_limit: u64) -> Result<Result<Vec<u8>, u64>, Error> {
    let metadata = map_err(
        std::fs::metadata(path),
        &format!("Could not fetch metadata for file {}", path),
    )?;
    let file_size = metadata.len();
    if file_size > memory_limit {
        Ok(Err(file_size))
    } else {
        map_err(
            std::fs::read(path).map(Ok),
            &format!("Could not load file {}", path),
        )
    }
}

/// Returns an iterator on the lines of a file.
///
/// # Arguments
///
/// * `path` - The path to the file to read the lines from.
///
/// # Returns
///
/// A vector containing all the lines of the file or an error if the file could not be read.
pub fn file_lines(path: &str) -> Result<Lines<BufReader<File>>, Error> {
    Ok(std::io::BufReader::new(open_file(path, FileMode::Read)?).lines())
}

/// Counts the number of lines in a file.
///
/// # Arguments
///
/// * `path` - The path to the file.
pub fn file_lines_count(path: &str) -> Result<usize, Error> {
    Ok(file_lines(path)?.count())
}

/// Creates a directory without returning an error if it already exists or one of its parents does not exist.
/// If one of the parents does not exist, creates it as well.
///
/// # Arguments
///
/// * `path` - The path to the directory.
///
/// # Returns
///
/// An error if the directory could not be created.
pub fn create_dir<P>(path: P) -> Result<(), Error>
where
    P: AsRef<Path>,
{
    let path_buf = path.as_ref().to_path_buf();
    match std::fs::create_dir_all(&path_buf) {
        Ok(_) => Ok(()),
        Err(e) => {
            if e.kind() != std::io::ErrorKind::AlreadyExists {
                Error::new(&format!(
                    "Could not create directory {}: {}",
                    path_buf.display(),
                    e
                ))
                .to_res()
            } else {
                Ok(())
            }
        }
    }
}

/// Deletes a directory.
///
/// # Arguments
///
/// * `path` - The path to the directory.
/// * `silent` - If true, does not raise an error if the directory does not exist.
///
/// # Returns
///
/// An error if the directory could not be deleted.
pub fn delete_dir<P>(path: P, silent: bool) -> Result<(), Error>
where
    P: AsRef<Path>,
{
    let path_buf = path.as_ref().to_path_buf();
    match std::fs::remove_dir_all(&path_buf) {
        Ok(_) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound && silent => Ok(()),
        Err(e) => Error::new(&format!(
            "Could not delete directory {}: {}",
            path_buf.display(),
            e
        ))
        .to_res(),
    }
}

/// Deletes a file.
///
/// # Arguments
///
/// * `path` - The path to the file.
/// * `silent` - If true, does not raise an error if the file does not exist.
///
/// # Returns
///
/// An error if the file could not be deleted.
///
pub fn delete_file<P>(path: P, silent: bool) -> Result<(), Error>
where
    P: AsRef<Path>,
{
    let path_buf = path.as_ref().to_path_buf();
    match std::fs::remove_file(&path_buf) {
        Ok(_) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound && silent => Ok(()),
        Err(e) => Error::new(&format!(
            "Could not delete file {}: {}",
            path_buf.display(),
            e
        ))
        .to_res(),
    }
}

pub fn write_file<P, C>(path: P, content: C) -> Result<(), Error>
where
    P: AsRef<Path>,
    C: AsRef<[u8]>,
{
    let path = path.as_ref();

    if let Some(parent) = path.parent() {
        create_dir(parent)?;
    }
    map_err(
        fs::write(path, content),
        &format!("Could not write to {}", path.display()),
    )?;

    Ok(())
}

pub fn open_csv(
    path: &str,
    schema: Option<Schema>,
    columns: Option<Vec<&str>>,
) -> Result<DataFrame, Error> {
    map_err(
        CsvReadOptions::default()
            .with_columns(
                columns
                    .map(|cols| Arc::from(cols.into_iter().map(|s| s.into()).collect::<Vec<_>>())),
            )
            .with_schema_overwrite(schema.map(Arc::new))
            .with_has_header(true)
            .into_reader_with_file_handle(BufReader::new(open_file(path, FileMode::Read)?))
            .finish(),
        &format!("Could not read {}", path),
    )
}

pub fn write_csv(path: &str, df: &mut DataFrame) -> Result<(), Error> {
    map_err(
        CsvWriter::new(BufWriter::new(open_file(path, FileMode::Overwrite)?))
            .include_header(true)
            .with_separator(b',')
            .finish(df),
        &format!("Could not write to {}", path),
    )
}

/// Returns a list of files with a given extension in a directory and its subdirectories,
/// sorted by their proximity to a pivot file.
/// The proximity is defined as the number of directory levels to go up from the pivot file
/// to reach the common ancestor directory, and then the number of directory levels to go down
/// to reach the file.
///
/// # Arguments
///
/// * `root_dir` - The root directory to search for files.
/// * `pivot_file` - The pivot file to measure the proximity from.
/// * `ext` - The file extension to filter the files by (case insensitive).
///
/// # Returns
///
/// A vector of paths to the files with the given extension, sorted by their proximity to the pivot file,
/// or an error if the root directory or the pivot file do not exist, or if the pivot file is not in the root directory.
///
/// # Examples
/// If the directory structure is as follows:
/// ```text
/// src/
/// ├── main.rs
/// ├── utils/
/// │   ├── snippets/
/// │   │   ├── example.rs
/// │   │   └── example.py
/// │   ├── foo.rs
/// │   ├── bar.rs
/// │   └── bar.py
/// └── io/
///     └── fs.rs
/// ```
/// and the pivot file is `src/utils/foo.rs`, calling
/// `files_sorted_by_proximity("src", "src/utils/foo.rs", "rs")`
/// will return the files in the following order:
/// 1. `src/utils/foo.rs` (0 ups, 0 downs)
/// 2. `src/utils/bar.rs` (1 up, 1 down)
/// 3. `src/utils/snippets/example.rs` (1 up, 2 downs)
/// 4. `src/main.rs` (2 ups, 1 down)
/// 5. `src/io/fs.rs` (2 ups, 2 down)
pub fn files_sorted_by_proximity(
    root_dir: impl AsRef<Path>,
    pivot_file: impl AsRef<Path>,
    ext: &str,
) -> Result<Vec<PathBuf>, Error> {
    let pivot_file = pivot_file.as_ref();
    let root_dir = root_dir.as_ref();

    if !pivot_file.exists() {
        Error::new(&format!("Pivot file {:?} does not exist", pivot_file)).to_res()
    } else {
        let pivot_canon = match pivot_file.canonicalize() {
            Ok(p) => p,
            Err(e) => Error::new(&format!(
                "Could not canonicalize pivot file {:?}: {}",
                pivot_file, e
            ))
            .to_res()?,
        };
        let root_canon = match root_dir.canonicalize() {
            Ok(p) => p,
            Err(e) => Error::new(&format!(
                "Could not canonicalize root dir {:?}: {}",
                root_dir, e
            ))
            .to_res()?,
        };

        if !pivot_canon.starts_with(&root_canon) {
            Error::new(&format!(
                "Pivot file {:?} is not in root dir {:?}",
                pivot_file, root_dir
            ))
            .to_res()
        } else {
            let mut files: Vec<PathBuf> = WalkDir::new(root_dir)
                .into_iter()
                .filter_map(Result::ok)
                .filter(|e| e.file_type().is_file())
                .map(|e| e.into_path())
                .filter(|p| {
                    p.extension()
                        .and_then(|e| e.to_str())
                        .map(|e| e.eq_ignore_ascii_case(ext))
                        .unwrap_or(false)
                })
                .collect();

            files.sort_by_key(|p| {
                // Safe unwrap because pivot_file is guaranteed to be a file
                let rel: PathBuf = diff_paths(p, pivot_file).unwrap();

                let mut ups = 0;
                let mut total = 0;
                for comp in rel.components() {
                    if matches!(comp, Component::ParentDir) {
                        ups += 1;
                    } else if !matches!(comp, Component::CurDir) {
                        total += 1;
                    }
                }
                (ups, total)
            });

            Ok(files)
        }
    }
}

#[cfg(test)]
mod io_tests {

    use std::io::Write;
    use std::path::Path;

    use super::*;

    #[test]
    fn read_file_test() {
        let file = open_file("tests/data/non_existent_file.txt", FileMode::Read);
        assert!(file.is_err());

        let file = open_file("tests/data/empty.csv", FileMode::Read);
        assert!(file.is_ok());
    }

    #[test]
    fn file_lines_test() {
        let path = "tests/data/small_file.csv";
        let lines = file_lines(path);
        assert!(lines.is_ok());
        let mut lines = lines.unwrap();
        assert_eq!(lines.next().unwrap().unwrap(), "id,name,fork");
        assert_eq!(lines.next().unwrap().unwrap(), "0,a,1");
        assert_eq!(lines.next().unwrap().unwrap(), "1,b,0");
        assert_eq!(lines.next().unwrap().unwrap(), "2,c,1");
        assert_eq!(lines.next().unwrap().unwrap(), "3,d,0");
        assert!(lines.next().is_none());
    }

    #[test]
    fn create_delete_dir_test() {
        let test_dir = "tests";
        assert!(create_dir(test_dir).is_ok());

        assert!(delete_dir(&format!("{}/new_dir", test_dir), true).is_ok());
        assert!(delete_dir(&format!("{}/new_dir", test_dir), false).is_err());

        let new_dir = format!("{}/new_dir/new_dir", test_dir);
        assert!(!Path::new(&new_dir).exists());
        assert!(create_dir(&new_dir).is_ok());
        assert!(Path::new(&new_dir).exists());

        assert!(delete_dir(&format!("{}/new_dir", test_dir), false).is_ok());
        assert!(!Path::new(&new_dir).exists());
    }

    #[test]
    fn create_delete_file_test() {
        let test_file = "tests/new_file.txt";

        assert!(delete_file(test_file, true).is_ok());
        assert!(delete_file(test_file, false).is_err());

        assert!(!Path::new(&test_file).exists());

        assert!(open_file(test_file, FileMode::Overwrite).is_ok());

        assert!(Path::new(&test_file).exists());

        assert!(delete_file(test_file, false).is_ok());

        assert!(!Path::new(&test_file).exists());
    }

    #[test]
    fn write_file_test() {
        let path = "tests/data/abc.txt";

        {
            let file = open_file(path, FileMode::Overwrite);
            assert!(file.is_ok());
            let mut file = file.unwrap();
            assert!(write!(file, "abc").is_ok());
        }

        let content = std::fs::read_to_string(path).unwrap();
        let lines: Vec<&str> = content.lines().collect();
        assert!(lines.len() == 1);
        assert_eq!(lines[0], "abc");

        {
            let file = open_file(path, FileMode::Append);
            assert!(file.is_ok());
            let mut file = file.unwrap();
            assert!(write!(file, "okok").is_ok());
        }

        let content = std::fs::read_to_string(path).unwrap();
        let lines: Vec<&str> = content.lines().collect();
        assert!(lines.len() == 1);
        assert_eq!(lines[0], "abcokok");

        {
            let file = open_file(path, FileMode::Overwrite);
            assert!(file.is_ok());
            let mut file = file.unwrap();
            assert!(write!(file, "abc").is_ok());
        }

        let content = std::fs::read_to_string(path).unwrap();
        let lines: Vec<&str> = content.lines().collect();
        assert!(lines.len() == 1);
        assert_eq!(lines[0], "abc");
    }

    #[test]
    fn line_count_test() {
        let count = file_lines_count("tests/data/small_file.csv");
        assert!(count.is_ok());
        assert_eq!(count.unwrap(), 5);

        assert!(file_lines_count("tests/data/non_existent_file.csv").is_err());
    }

    #[test]
    fn files_sorted_by_proximity_test() {
        let root_dir = "tests/data/test_project";
        let pivot_file = "tests/data/test_project/utils/foo.rs";

        let files = files_sorted_by_proximity(root_dir, pivot_file, "rs");
        println!("{:?}", files);
        assert!(files.is_ok());
        let files = files
            .unwrap()
            .into_iter()
            .map(|p| p.to_str().unwrap().to_string())
            .collect::<Vec<_>>();
        let expected_files = vec![
            "tests/data/test_project/utils/foo.rs",
            "tests/data/test_project/utils/bar.rs",
            "tests/data/test_project/utils/snippets/example.rs",
            "tests/data/test_project/main.rs",
            "tests/data/test_project/io/fs.rs",
        ];
        assert_eq!(files, expected_files);
    }
}
