Parses source files and extracts functions whose bodies contain at least one user-specified keyword. The input file must be a valid CSV file with the columns 'local_path', the local path to each source file, and 'file', its path inside its repository, such as the file log of download. The input must also have a 'source' column, unless --infer-source is used: files without a source are then on GitHub if they have a repository ID, and local otherwise. The other repository columns of the input ('repo_id', 'repo_name', 'repo_license' and 'latest_commit') are copied to both output files. Other columns are ignored. Columns of the input can be read under other names with --rename OLD=NEW, for example --rename path=local_path.

The language of each file is given by its extension, as defined in the keyword JSON files. Supported languages are C, C++, C#, Fortran, Go, Java, Python, Scala, Typescript and Rust. Files in other languages are skipped with a warning.

Files are processed in random order using a reproducible shuffle controlled by a seed. Each file is parsed with Tree-sitter using the grammar for its language. Functions are retained only if their body contains at least one keyword from the provided keyword JSON files. Keyword matching is performed after removing comments and string literals. Keywords can be interpreted as regular expressions or whole words according to the --regex flag. Keywords are matched without regard to letter case, unless --case-sensitive is used. File extensions are always case-sensitive: '.c' does not match a file named 'main.C'.
The format of the keyword JSON files is as follows:

{
  "languages": [
    {
      "name": "LanguageName",
      "extensions": [".ext1", ".ext2", ...],
      "keywords": ["localKeyword1", "localKeyword2", ...]    // optional
    },
    ...
  ],
  "keywords": ["globalKeyword1", "globalKeyword2", ...]      // optional
}

For each retained function, the command writes the function source code to a separate file in a directory named after the source file with the suffix .functions, unless --count is used. It also computes structural statistics such as the number and nesting depth of loops, conditionals, and function calls, as well as parameter counts.

The command writes two CSV files: one containing function-level statistics and one containing file-level parsing statistics. By default, these files are named by appending '.functions.csv' and '.function_logs.csv' to the input file name.

Both files have a 'url' column with links to the files and to the first line of the functions on GitHub, at the commit of the repositories, or on their default branch without a commit. Links are empty for files that are not on GitHub.

Parse errors are handled according to the policy selected with --failures:
  * ignore: the errors are ignored and the statistics are computed as if there was no error;
  * skip-file: no function is extracted from the file, and its row in the function logs has the error code E030 and empty statistics;
  * skip-function: the function is not extracted, and its row in the functions file has the error code E030 and empty statistics;
  * abort: the run stops at the first file with a parse error.

Files too large to be parsed get the error code E031 and empty statistics in the function logs. At the end, the command prints how many files and functions have each code; --debug also prints every error with its details. The codes are explained by scyros explain.

Output functions CSV format:
  * repo_id, source, repo_name, repo_license, latest_commit: repository columns, copied from the input, empty when unknown
  * local_path: local path to the source file
  * file: path of the source file inside its repository
  * function_name: function or method name
  * line, column: starting line and column of the function in the source file
  * url: link to the first line of the function on GitHub
  * error: error code if the function was skipped, empty otherwise
  * local_function_path: path to the extracted function file (not written with --count)
  * language: programming language
  * loc: number of lines in the function
  * words: number of words in the function
  * ...: number of matches for each keyword file
  * loop_statements: number of loop statements
  * loop_nestings: maximum loop nesting depth
  * if_statements: number of conditional statements
  * if_nestings: maximum conditional nesting depth
  * functions_calls: number of function or method calls
  * function_calls_nestings: maximum nesting depth of function or method calls
  * params: number of parameters
  * param_kw_match: number of parameters whose type matches a keyword
  * return_kw_match: whether the return type matches a keyword
  * parse_error_line, parse_error_column: position of the first parse error relative to the function, empty without error

Output function logs CSV format:
  * repo_id, source, repo_name, repo_license, latest_commit: repository columns, copied from the input, empty when unknown
  * local_path: local path to the source file
  * file: path of the source file inside its repository
  * url: link to the file on GitHub
  * error: error code if the file was skipped, empty otherwise
  * language: programming language
  * functions: number of functions found in the file
  * functions_with_kw: number of retained functions
  * ...: number of retained functions matching each keyword file
  * parse_error_line, parse_error_column: position of the first parse error in the file, empty without error