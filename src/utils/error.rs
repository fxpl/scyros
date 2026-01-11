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

//! Utility types and functions for handling errors in the program.
//! The backtrace of the error is captured when it is created.

use snafu::Snafu;
use std::fmt::{Debug, Display};

/// Error type for the program.
#[derive(Debug, Snafu)]
#[snafu(display("{}", msg))]
pub struct Error {
    /// Error message.
    msg: String,

    /// Backtrace of the error.
    backtrace: snafu::Backtrace,
}

impl Error {
    /// Create a new error with a custom message.
    ///
    /// # Arguments
    ///
    /// * `msg` - Error message.
    ///
    /// # Returns
    ///
    /// A new error whose backtrace starts at the call to this function.
    pub fn new(msg: &str) -> Self {
        Error {
            msg: msg.to_string(),
            backtrace: snafu::Backtrace::capture(),
        }
    }

    /// Return a string representation of the error.
    ///
    /// # Arguments
    ///
    /// * `debug` - Whether to include the backtrace in the string.
    pub fn to_string(self, debug: bool) -> String {
        if debug {
            format!(">> {}\n\nBacktrace:\n{}", self.msg, self.backtrace)
        } else {
            self.msg
        }
    }

    /// Chain a new error message to the current error.
    ///
    /// # Arguments
    ///
    /// * `msg` - Additional error message.
    ///
    /// # Returns
    ///
    /// A new error with the new message on top followed by the current one.
    pub fn chain(self, msg: &str) -> Self {
        Error {
            msg: format!("{}\n>> {}", msg, self.msg),
            backtrace: self.backtrace,
        }
    }

    /// Return a result with the error as the value.
    pub fn to_res<T>(self) -> Result<T, Self> {
        Err(self)
    }
}

/// Map a different error type to the program's error type.
///
/// # Arguments
///
/// * `err` - Error to map.
/// * `msg` - Additional error message to chain.
///
/// # Returns
///
/// A new error with the new message followed by the original error.
///
pub fn map<S>(err: S, msg: &str) -> Error
where
    S: Display,
{
    Error::new(&err.to_string()).chain(msg)
}

/// Map a result with a different error type to a result with the program's error type.
/// This function is lazy, i.e., it only evaluates the error message if the result is an error.
///
/// # Arguments
///
/// * `res` - Result to map.
/// * `msg` - Additional error message to chain.
///
/// # Returns
///
/// The original result if it is Ok, otherwise a new error with the new message followed by the original error.
pub fn map_err<T, S>(res: Result<T, S>, msg: &str) -> Result<T, Error>
where
    S: Display,
{
    res.map_err(|s| map(s, msg))
}

/// Map a result with a different error type to a result with the program's error type.
/// This function is lazy, i.e., it only evaluates the error message if the result is an error.
///
/// # Arguments
///
/// * `res` - Result to map.
/// * `msg` - Additional error message to chain.
///
/// # Returns
///
/// The original result if it is Ok, otherwise a new error with the new message followed by the original error.
pub fn map_err_debug<T, S>(res: Result<T, S>, msg: &str) -> Result<T, Error>
where
    S: Debug,
{
    res.map_err(|s| Error::new(&format!("{}\n>> {:?}", msg, s)))
}

/// Map an option to a result with the program's error type.
/// This function is lazy, i.e., it only evaluates the error message if the option is None.
///
/// # Arguments
///
/// * `opt` - The optional value to map.
/// * `msg` - Error message to return if the option is None.
///
/// # Returns
///
/// Ok if the option is Some, otherwise an error with the given message.
pub fn ok_or_else<T>(opt: Option<T>, msg: &str) -> Result<T, Error> {
    opt.ok_or_else(|| Error::new(msg))
}

/// Return a string representation of a result.
///
/// # Arguments
///
/// * `res` - Result of the program.
/// * `debug` - Whether to include the backtrace in the string.
pub fn res_to_string<T>(res: Result<T, Error>, debug: bool) -> String {
    match res {
        Ok(_) => "Program finished successfully.".to_string(),
        Err(e) => format!("Program terminated with an error:\n{}", e.to_string(debug)),
    }
}

#[cfg(test)]
mod tests {

    use super::*;
    use std::io::Error as IoError;

    #[test]
    fn to_string_test() {
        let msg = "Test error";
        let msg2 = "Another test error";
        assert!(Error::new(msg).chain(msg2).to_string(true).contains(&msg));
        assert!(Error::new(msg).chain(msg2).to_string(true).contains(&msg2));
        assert!(Error::new(msg).chain(msg2).to_string(false).contains(&msg));
        assert!(Error::new(msg).chain(msg2).to_string(false).contains(&msg2));

        assert!(map(Error::new(msg), msg2).to_string(true).contains(&msg));
        assert!(map(Error::new(msg), msg2).to_string(true).contains(&msg2));
        assert!(map(Error::new(msg), msg2).to_string(false).contains(&msg));
        assert!(map(Error::new(msg), msg2).to_string(false).contains(&msg2));
    }

    #[test]
    fn chain_test() {
        let msg1 = "Test error 1";
        let msg2 = "Test error 2";
        let msg3 = "Test error 3";
        let e = Error::new(msg1).chain(&msg2).chain(&msg3);
        assert!(e.msg.contains(&msg1));
        assert!(e.msg.contains(&msg2));
        assert!(e.msg.contains(&msg3));
    }

    #[test]
    fn to_res_test() {
        let e = Error::new("Test error");
        assert!(e.to_res::<()>().is_err());
    }

    #[test]
    fn map_test() {
        let err = IoError::new(std::io::ErrorKind::Other, "Test error");
        let err_msg = err.to_string();
        let new_msg = "Test error";
        let new_msg2 = "Test error 2";
        let new_err = map(map(err, &new_msg), &new_msg2);
        assert!(new_err.msg.contains(&err_msg));
        assert!(new_err.msg.contains(new_msg));
        assert!(new_err.msg.contains(&new_msg2));
    }

    #[test]
    fn map_err_test() {
        let err = IoError::new(std::io::ErrorKind::Other, "Test error");
        let err_msg = err.to_string();
        let new_msg: String = "Test error".to_string();
        let new_msg2: String = "Test error 2".to_string();
        let new_res: Result<(), Error> = map_err(map_err(Err(err), &new_msg), &new_msg2);
        assert!(new_res.is_err());
        let new_err = new_res.err().unwrap();
        assert!(new_err.msg.contains(&err_msg));
        assert!(new_err.msg.contains(&new_msg));
        assert!(new_err.msg.contains(&new_msg2));

        let res: Result<(), IoError> = Ok(());
        let new_res: Result<(), Error> = map_err(res, &new_msg);
        assert!(new_res.is_ok());
        assert_eq!(new_res.unwrap(), ());
    }

    #[test]
    fn map_err_debug_test() {
        let err: Vec<usize> = Vec::new();
        let err_msg = format!("{:?}", err);
        let new_msg = "Test error";
        let new_res: Result<(), Error> = map_err_debug(Err(err), new_msg);
        assert!(new_res.is_err());
        let new_err = new_res.err().unwrap();
        assert!(new_err.msg.contains(&err_msg));
        assert!(new_err.msg.contains(&new_msg));

        let res: Result<(), Vec<usize>> = Ok(());
        let new_res: Result<(), Error> = map_err_debug(res, new_msg);
        assert!(new_res.is_ok());
        assert_eq!(new_res.unwrap(), ());
    }

    #[test]
    fn ok_or_else_test() {
        let opt: Option<usize> = Some(0);
        let msg = "Test error";
        let res: Result<usize, Error> = ok_or_else(opt, msg);
        assert!(res.is_ok());
        assert_eq!(res.unwrap(), 0);

        let opt: Option<usize> = None;
        let res: Result<usize, Error> = ok_or_else(opt, msg);
        assert!(res.is_err());
        let err = res.err().unwrap();
        assert!(err.msg.contains(&msg));
    }

    #[test]
    fn res_to_string_test() {
        let msg = "Test error";
        let msg2 = "Another test error";
        assert!(res_to_string(map_err(Error::new(msg).to_res::<()>(), &msg2), true).contains(msg));
        assert!(res_to_string(map_err(Error::new(msg).to_res::<()>(), &msg2), true).contains(msg2));
        assert!(res_to_string(map_err(Error::new(msg).to_res::<()>(), &msg2), false).contains(msg));
        assert!(
            res_to_string(map_err(Error::new(msg).to_res::<()>(), &msg2), false).contains(msg2)
        );
    }
}
