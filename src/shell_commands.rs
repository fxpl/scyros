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

use std::{fmt::Display, process::Command};

/// DSL for usefull shell commands.
pub enum ShellCommand<'a> {
    /// Find command to search for files in a directory. Internally uses `find`.
    Find {
        /// Underlying builder object.
        builder: FindCommand,
    },

    /// Unzip a file. Internally uses `unzip`.
    Unzip {
        /// The name of the file to unzip (without .zip).
        filename: &'a str,
    },
}

impl ShellCommand<'_> {
    /// Returns an error message if the command fails.
    /// The error message is specific to the command and contains the name of the file or directory where the error occured.s
    fn error_msg(&self) -> String {
        match self {
            ShellCommand::Find { builder: find } => {
                format!("Find command failed on {}", &find.filename)
            }
            ShellCommand::Unzip { filename } => format!("Could not unzip {}.zip", filename),
        }
    }

    /// Returns the associated Command object with the appropriate arguments.
    pub fn command(self) -> Command {
        match self {
            ShellCommand::Find { builder: find } => find.command,
            ShellCommand::Unzip { filename } => {
                let mut cmd = Command::new("unzip");
                cmd.arg(format!("{}.zip", filename)).arg("-d").arg(filename);
                cmd
            }
        }
    }

    /// Runs the command and returns the output as a string.
    ///
    /// # Panics
    ///
    /// If the command fails.
    pub fn run(self) -> String {
        let error_msg = self.error_msg();
        let output = self.command().output().expect(&error_msg);
        String::from_utf8_lossy(&output.stdout).to_string()
    }
}

/// File type to search for when running the `find` command.
/// In command line arguments, the file type is represented by a single character after the `-type` flag.
pub enum FileType {
    /// Regular file, type 'f'.
    File,
    /// Directory, type 'd'.
    Directory,
    /// Symbolic link, type 'l'.
    SymbolicLink,
}

impl Display for FileType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}",
            match self {
                FileType::File => "f",
                FileType::Directory => "d",
                FileType::SymbolicLink => "l",
            }
        )
    }
}

/// DSL for the `find` command.
/// Comes with a builder pattern to add options to the command.
pub struct FindCommand {
    /// The name of the directory to search in.
    filename: String,
    /// The command object to build.
    command: Command,
}

impl FindCommand {
    /// Creates a new FindCommand object which search in the specified directory.
    ///
    /// # Arguments
    ///
    /// * `filename` - The name of the directory to search in.
    pub fn new(filename: &str) -> Self {
        let mut cmd = Command::new("find");
        cmd.arg(filename);
        FindCommand {
            filename: filename.to_string(),
            command: cmd,
        }
    }

    /// Lists only files with the specified file extension.
    ///
    /// # Arguments
    ///
    /// * `ext` - The extension of the files to search for.
    pub fn file_extension(mut self, ext: &str) -> Self {
        self.command.arg("-name");
        self.command.arg(format!("*.{}", ext));
        self
    }

    /// Lists only files with the specified file extensions.
    ///
    /// # Arguments
    ///
    /// * `extensions` - An iterator over the extensions of the files to search for.
    pub fn file_extensions<'a, I>(mut self, extensions: I) -> Self
    where
        I: IntoIterator<Item = &'a String>,
    {
        self.command.arg("(");
        let mut joined_ext = extensions
            .into_iter()
            .flat_map(|ext| vec!["-name".to_string(), format!("*.{}", ext), "-o".to_string()])
            .collect::<Vec<String>>();
        // Remove the last "-o" from the list.
        joined_ext.pop();
        self.command.args(joined_ext);
        self.command.arg(")");
        self
    }

    /// Lists only files with the specified file type.
    ///
    /// # Arguments
    ///
    /// * `file_type` - The type of the files to search for.
    pub fn file_type(mut self, file_type: FileType) -> Self {
        self.command.arg("-type");
        self.command.arg(file_type.to_string());
        self
    }

    /// Lists only empty files or directories.
    pub fn empty(mut self) -> Self {
        self.command.arg("-empty");
        self
    }

    /// Deletes the files or directories found.
    pub fn delete(mut self) -> Self {
        self.command.arg("-delete");
        self
    }

    /// Negates the next expression.
    pub fn not(mut self) -> Self {
        self.command.arg("!");
        self
    }
}
