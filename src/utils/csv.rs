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

use super::fs::*;
use anyhow::{anyhow, bail, Context, Result};
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
    pub fn new(path: &str, mode: FileMode) -> Result<Self> {
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
    pub fn switch_mode(self, mode: FileMode) -> Result<Self> {
        Self::new(&self.path, mode)
    }

    /// Opens a reader for this file
    fn read(&self) -> Result<Reader<File>> {
        if self.writer.is_some() {
            bail!(
                "Cannot read from {} since it is in write-only mode",
                self.path
            )
        } else {
            Ok(csv::ReaderBuilder::new().from_reader(open_file(&self.path, FileMode::Read)?))
        }
    }

    // TODO: Test
    /// Writes a header to this file if it is empty.
    ///
    /// # Arguments
    ///
    /// * `header` - The header fields, escaped per RFC 4180.
    ///
    /// # Returns
    ///
    /// An error if the file is in read-only mode or the write fails.
    pub fn write_header<I, S>(&mut self, header: I) -> Result<()>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<[u8]>,
    {
        let writer = self.writer.as_mut().ok_or_else(|| {
            anyhow!(
                "Cannot write to {} since it is in read-only mode",
                self.path
            )
        })?;
        if writer.get_ref().metadata()?.len() != 0 {
            return Ok(());
        }
        let mut csv_writer = csv::WriterBuilder::new()
            .has_headers(false)
            .from_writer(writer);
        csv_writer.write_record(header)?;
        csv_writer.flush()?;
        Ok(())
    }

    /// Writes one record to this file, escaping fields per RFC 4180.
    ///
    /// # Arguments
    ///
    /// * `fields` - The fields of the record.
    ///
    /// # Returns
    ///
    /// An error if the file is in read-only mode or the write fails.
    pub fn write_record<I, S>(&mut self, fields: I) -> Result<()>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<[u8]>,
    {
        let writer = self.writer.as_mut().ok_or_else(|| {
            anyhow!(
                "Cannot write to {} since it is in read-only mode",
                self.path
            )
        })?;
        let mut csv_writer = csv::WriterBuilder::new()
            .has_headers(false)
            .from_writer(writer);
        csv_writer.write_record(fields)?;
        csv_writer.flush()?;
        Ok(())
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
    pub fn extract<T, F>(&self, extractor: F) -> Result<Vec<T>>
    where
        F: Fn(usize, StringRecord) -> Result<T>,
    {
        let mut res = Vec::<T>::new();

        for (line, x) in self.read()?.records().enumerate() {
            res.push(extractor(line, x?)?);
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
    pub fn column<T>(&self, i: usize) -> Result<Vec<T>>
    where
        T: FromStr,
    {
        self.extract(|line, record| {
            record
                .get(i)
                .with_context(|| {
                    format!(
                        "Record {}: Record length is {} but the requested column is {}",
                        line,
                        record.len(),
                        i,
                    )
                })
                .and_then(|entry| {
                    entry
                        .parse::<T>()
                        .map_err(|_| anyhow!("Could not parse record {line}"))
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
    pub fn indexed_lines<T>(&self, i: usize) -> Result<HashMap<T, String>>
    where
        T: FromStr + Eq + Hash,
    {
        let keys: Vec<T> = self.column(i)?;
        let lines: Vec<String> = std::fs::read_to_string(&self.path)?
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

    use anyhow::ensure;

    use super::*;

    #[test]
    fn new_test() -> Result<()> {
        let before = std::fs::read_to_string("tests/data/small_file.csv")?;
        CSVFile::new("tests/data/small_file.csv", FileMode::Read)?;
        CSVFile::new("tests/data/small_file.csv", FileMode::Append)?;
        let after = std::fs::read_to_string("tests/data/small_file.csv")?;
        assert_eq!(before, after);
        CSVFile::new("tests/data/empty.csv", FileMode::Overwrite)?;
        ensure!(CSVFile::new("tests/data/non_existent.csv", FileMode::Read).is_err());
        CSVFile::new("tests/data/non_existent.csv", FileMode::Append)?;
        delete_file("tests/data/non_existent.csv", false)?;
        CSVFile::new("tests/data/non_existent.csv", FileMode::Overwrite)?;
        delete_file("tests/data/non_existent.csv", false)
    }

    #[test]
    fn read_test() -> Result<()> {
        CSVFile::new("tests/data/small_file.csv", FileMode::Read)?.read()?;
        ensure!(CSVFile::new("tests/data/small_file.csv", FileMode::Append)?
            .read()
            .is_err());
        ensure!(CSVFile::new("tests/data/empty.csv", FileMode::Overwrite)?
            .read()
            .is_err());
        Ok(())
    }

    #[test]
    fn empty_column_test() -> Result<()> {
        let file = CSVFile::new("tests/data/empty.csv", FileMode::Read)?;
        for i in 0..10 {
            let ids = file.column::<u64>(i)?;
            ensure!(ids.is_empty());

            let ids = file.column::<u64>(i)?;
            ensure!(ids.is_empty());
        }
        Ok(())
    }

    #[test]
    fn column_test() -> Result<()> {
        let file = CSVFile::new("tests/data/small_file.csv", FileMode::Read)?;

        let ids = file.column::<usize>(0)?;
        assert_eq!(ids, vec![0, 1, 2, 3]);

        let names = file.column::<String>(1)?;
        assert_eq!(names, vec!["a", "b", "c", "d"]);
        let forks = file.column::<u8>(2)?;
        assert_eq!(forks, vec![1, 0, 1, 0]);

        let ips = file.column::<IpAddr>(3);
        ensure!(ips.is_err());

        let ids = file.column::<i32>(1);
        ensure!(ids.is_err());

        let file = CSVFile::new("tests/data/invalid_csv.csv", FileMode::Read)?;
        ensure!(file.column::<i8>(0).is_err());
        Ok(())
    }
    #[test]
    fn indexed_lines_test() -> Result<()> {
        let file = CSVFile::new("tests/data/small_file.csv", FileMode::Read)?;
        let indexed_lines = file.indexed_lines::<i32>(0)?;
        assert_eq!(indexed_lines.len(), 4);
        assert_eq!(
            indexed_lines
                .get(&0)
                .with_context(|| "Could not find index 0")?,
            "0,a,1"
        );
        assert_eq!(
            indexed_lines
                .get(&1)
                .with_context(|| "Could not find index 1")?,
            "1,b,0"
        );
        assert_eq!(
            indexed_lines
                .get(&2)
                .with_context(|| "Could not find index 2")?,
            "2,c,1"
        );
        assert_eq!(
            indexed_lines
                .get(&3)
                .with_context(|| "Could not find index 3")?,
            "3,d,0"
        );

        let indexed_lines = file.indexed_lines::<String>(1)?;
        assert_eq!(indexed_lines.len(), 4);
        assert_eq!(
            indexed_lines
                .get("a")
                .with_context(|| "Could not find index 'a'")?,
            "0,a,1"
        );
        assert_eq!(
            indexed_lines
                .get("b")
                .with_context(|| "Could not find index 'b'")?,
            "1,b,0"
        );
        assert_eq!(
            indexed_lines
                .get("c")
                .with_context(|| "Could not find index 'c'")?,
            "2,c,1"
        );
        assert_eq!(
            indexed_lines
                .get("d")
                .with_context(|| "Could not find index 'd'")?,
            "3,d,0"
        );

        ensure!(file.indexed_lines::<bool>(3).is_err());
        ensure!(file.indexed_lines::<u64>(1).is_err());

        let empty = CSVFile::new("tests/data/empty.csv", FileMode::Read)?;

        let indexed_lines = empty.indexed_lines::<IpAddr>(0)?;
        assert_eq!(indexed_lines.len(), 0);
        Ok(())
    }

    #[test]
    fn write_record_test() -> Result<()> {
        let path = "tests/data/write_record_test.csv";
        let _ = delete_file(path, true);

        let mut file = CSVFile::new(path, FileMode::Overwrite)?;
        file.write_header(&["name", "note"])?;
        file.write_record(["alice", "hello"])?;
        file.write_record(["bob", "he said \"hi\""])?;
        file.write_record(["carol", "a, b, c"])?;
        file.flush()?;

        // Read it back via CSVFile and check round-trip.
        let read_file = CSVFile::new(path, FileMode::Read)?;
        let names = read_file.column::<String>(0)?;
        let notes = read_file.column::<String>(1)?;
        assert_eq!(names, vec!["alice", "bob", "carol"]);
        assert_eq!(notes, vec!["hello", "he said \"hi\"", "a, b, c"]);

        delete_file(path, false)?;
        Ok(())
    }
}
