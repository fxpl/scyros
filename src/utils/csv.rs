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

//! Utility functions for working with CSV files.

use super::error::*;
use super::fs::*;
use csv::{Reader, StringRecord};
use std::collections::HashMap;
use std::fs::File;
use std::hash::Hash;
use std::io;
use std::io::BufWriter;
use std::io::Write;
use std::str::FromStr;

#[derive(Debug)]
pub struct CSVFile {
    path: String,
    writer: Option<BufWriter<File>>,
}

impl Write for CSVFile {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.writer
            .as_mut()
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::ReadOnlyFilesystem,
                    "The file is not opened in write mode",
                )
            })?
            .write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.writer
            .as_mut()
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::ReadOnlyFilesystem,
                    "The file is not opened in write mode",
                )
            })?
            .flush()
    }
}

impl CSVFile {
    /// Opens a CSV file in the specified mode.
    ///
    /// # Arguments
    ///
    /// * `path` - The path to the CSV file.
    /// * `mode` - The mode to open the file in.
    ///
    /// # Returns
    ///
    /// A CSV file in the specified mode or an error if the file could not be opened.
    pub fn new(path: &str, mode: FileMode) -> Result<Self, Error> {
        Ok(Self {
            path: path.to_string(),
            writer: {
                let file: File = open_file(path, mode)?;
                if mode == FileMode::Read {
                    None
                } else {
                    Some(BufWriter::new(file))
                }
            },
        })
    }

    // TODO: Test
    /// Switches the mode of the file.
    ///
    /// # Arguments
    ///
    /// * `mode` - The new mode to switch to.
    ///
    /// # Returns
    ///
    /// The file in the new mode or an error if the file could not be opened.
    pub fn switch_mode(self, mode: FileMode) -> Result<Self, Error> {
        Self::new(&self.path, mode)
    }

    /// Opens a reader for this file
    fn read(&self) -> Result<Reader<File>, Error> {
        if self.writer.is_some() {
            Error::new(&format!(
                "Cannot read from {} since it is in write-only mode",
                self.path
            ))
            .to_res()
        } else {
            Ok(csv::ReaderBuilder::new()
                .has_headers(true)
                .double_quote(false)
                .escape(Some(b'\\'))
                .from_reader(open_file(&self.path, FileMode::Read)?))
        }
    }

    // TODO: Test
    /// Writes a header to this file if it is empty or if the force flag is set.
    ///
    /// # Arguments
    ///
    /// * `header` - The header to write.
    /// * `force` - If true, the header is written even if the file is not empty.
    ///
    /// # Returns
    ///
    /// An error if the header could not be written or if the metadata of the file could not be read.
    pub fn write_header(&mut self, header: &[&str]) -> Result<(), Error> {
        match self.writer.as_mut() {
            None => Error::new(&format!(
                "Cannot write to {} since it is in read-only mode",
                self.path
            ))
            .to_res(),
            Some(f) => {
                if map_err(f.get_ref().metadata(), "Could not read metadata of file")?.len() == 0 {
                    map_err(
                        writeln!(self, "{}", header.join(",")),
                        "Error writing header",
                    )
                } else {
                    Ok(())
                }
            }
        }
    }

    // TODO: Test
    /// Extracts information from the records of this file and returns it in a vector.
    ///
    /// # Arguments
    ///
    /// * `extractor` - A function that extracts information from a record.
    ///
    /// # Returns
    ///
    /// A vector containing the extracted information or an error if the file could not be read or the information could not be extracted.
    pub fn extract<T, F>(&self, extractor: F) -> Result<Vec<T>, Error>
    where
        F: Fn(usize, StringRecord) -> Result<T, Error>,
    {
        let mut res = Vec::<T>::new();

        for (line, x) in self.read()?.records().enumerate() {
            let record: StringRecord = map_err(x, "Could not read record")?;
            res.push(extractor(line, record)?);
        }
        Ok(res)
    }

    /// Loads the values of a column from this file into a vector.
    /// If the specified column cannot be parsed, an error is returned.
    /// If the file does not exist or is invalid, an error is returned.
    ///
    /// # Arguments
    ///
    /// * `i` - The index of the column containing the ids.
    ///
    /// # Returns
    ///
    /// A vector containing the values of the specified column or an error if the file could not be read or the column could not be parsed.
    ///
    pub fn column<T>(&self, i: usize) -> Result<Vec<T>, Error>
    where
        T: FromStr,
    {
        self.extract(|line, record| {
            ok_or_else(
                record.get(i),
                &format!(
                    "Record {}: Record length is {} but the requested column is {}",
                    line,
                    record.len(),
                    i,
                ),
            )
            .and_then(|entry| {
                entry
                    .parse::<T>()
                    .map_err(|_| Error::new(&format!("Could not parse record {}", line)))
            })
        })
    }

    /// Loads the lines of this file into a hash map.
    /// One of the columns of the file serves as the keys of the hash map.
    ///
    /// # Arguments
    ///
    /// * `cache_path` - The cache file.
    /// * `i` - The index of the column containing the ids of the projects.
    ///
    /// # Returns
    ///
    /// A hash map containing the lines of the file indexed by the specified column or an error if the file could not be parsed.
    ///
    pub fn indexed_lines<T>(&self, i: usize) -> Result<HashMap<T, String>, Error>
    where
        T: FromStr + Eq + Hash,
    {
        let keys: Vec<T> = self.column(i)?;
        let lines: Vec<String> =
            map_err(std::fs::read_to_string(&self.path), "Could not read file")?
                .lines()
                .map(|s| s.to_string())
                .collect();
        if lines.is_empty() {
            Ok(HashMap::new())
        } else {
            Ok(keys.into_iter().zip(lines[1..].to_vec()).collect())
        }
    }
}

/// Cleans a string to be safely stored in a CSV file by removing quotes and replacing commas and newlines with spaces.
///
/// # Arguments
///
/// * `s` - The string to clean.
///
/// # Returns
///
/// A cleaned string safe for CSV storage.
pub fn clean_string_to_csv(s: &str) -> String {
    s.replace("\"", "")
        .replace(",", " ")
        .lines()
        .collect::<Vec<&str>>()
        .join(" ")
        .trim()
        .to_string()
}

#[cfg(test)]
mod tests {

    use std::net::IpAddr;

    use super::*;

    #[test]
    fn new_test() {
        let before = std::fs::read_to_string("tests/data/small_file.csv").unwrap();
        assert!(CSVFile::new("tests/data/small_file.csv", FileMode::Read).is_ok());
        assert!(CSVFile::new("tests/data/small_file.csv", FileMode::Append).is_ok());
        let after = std::fs::read_to_string("tests/data/small_file.csv").unwrap();
        assert_eq!(before, after);
        assert!(CSVFile::new("tests/data/empty.csv", FileMode::Overwrite).is_ok());
        assert!(CSVFile::new("tests/data/non_existent.csv", FileMode::Read).is_err());
        assert!(CSVFile::new("tests/data/non_existent.csv", FileMode::Append).is_ok());
        assert!(delete_file("tests/data/non_existent.csv", false).is_ok());
        assert!(CSVFile::new("tests/data/non_existent.csv", FileMode::Overwrite).is_ok());
        assert!(delete_file("tests/data/non_existent.csv", false).is_ok());
    }

    #[test]
    fn read_test() {
        assert!(CSVFile::new("tests/data/small_file.csv", FileMode::Read)
            .unwrap()
            .read()
            .is_ok());
        assert!(CSVFile::new("tests/data/small_file.csv", FileMode::Append)
            .unwrap()
            .read()
            .is_err());
        assert!(CSVFile::new("tests/data/empty.csv", FileMode::Overwrite)
            .unwrap()
            .read()
            .is_err());
    }

    #[test]
    fn empty_column_test() {
        let file = CSVFile::new("tests/data/empty.csv", FileMode::Read).unwrap();
        for i in 0..10 {
            let ids = file.column::<u64>(i);
            assert!(ids.is_ok());
            assert!(ids.unwrap().is_empty());

            let ids = file.column::<u64>(i);
            assert!(ids.is_ok());
            assert!(ids.unwrap().is_empty());
        }
    }

    #[test]
    fn column_test() {
        let file = CSVFile::new("tests/data/small_file.csv", FileMode::Read).unwrap();

        let ids = file.column::<usize>(0);
        assert!(ids.is_ok());
        assert_eq!(ids.unwrap(), vec![0, 1, 2, 3]);

        let names = file.column::<String>(1);
        assert!(names.is_ok());
        assert_eq!(names.unwrap(), vec!["a", "b", "c", "d"]);
        let forks = file.column::<u8>(2);
        assert!(forks.is_ok());
        assert_eq!(forks.unwrap(), vec![1, 0, 1, 0]);

        let ips = file.column::<IpAddr>(3);
        assert!(ips.is_err());

        let ids = file.column::<i32>(1);
        assert!(ids.is_err());

        let file = CSVFile::new("tests/data/invalid_csv.csv", FileMode::Read).unwrap();
        assert!(file.column::<i8>(0).is_err());
    }
    #[test]
    fn indexed_lines_test() {
        let file = CSVFile::new("tests/data/small_file.csv", FileMode::Read).unwrap();
        let indexed_lines = file.indexed_lines::<i32>(0);
        assert!(indexed_lines.is_ok());
        let indexed_lines = indexed_lines.unwrap();
        assert_eq!(indexed_lines.len(), 4);
        assert_eq!(indexed_lines.get(&0).unwrap(), "0,a,1");
        assert_eq!(indexed_lines.get(&1).unwrap(), "1,b,0");
        assert_eq!(indexed_lines.get(&2).unwrap(), "2,c,1");
        assert_eq!(indexed_lines.get(&3).unwrap(), "3,d,0");

        let indexed_lines = file.indexed_lines::<String>(1);
        assert!(indexed_lines.is_ok());
        let indexed_lines = indexed_lines.unwrap();
        assert_eq!(indexed_lines.len(), 4);
        assert_eq!(indexed_lines.get(&"a".to_string()).unwrap(), "0,a,1");
        assert_eq!(indexed_lines.get(&"b".to_string()).unwrap(), "1,b,0");
        assert_eq!(indexed_lines.get(&"c".to_string()).unwrap(), "2,c,1");
        assert_eq!(indexed_lines.get(&"d".to_string()).unwrap(), "3,d,0");

        assert!(file.indexed_lines::<bool>(3).is_err());
        assert!(file.indexed_lines::<u64>(1).is_err());

        let empty = CSVFile::new("tests/data/empty.csv", FileMode::Read).unwrap();

        let indexed_lines = empty.indexed_lines::<IpAddr>(0);
        assert!(indexed_lines.is_ok());
        assert_eq!(indexed_lines.unwrap().len(), 0);
    }
}
