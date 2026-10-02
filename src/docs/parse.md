Parses source files and extracts functions whose bodies contain at least one user-specified keyword. The input file must be a valid CSV file containing the columns 'id' and 'name', where 'id' identifies the repository and 'name' is the path to the source file. Other columns are ignored.

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

Parse errors are handled according to the policy selected with --failures:
  * ignore: the errors are ignored and the statistics are computed as if there was no error;
  * skip-file: no function is extracted from the file, and its row in the function logs has -1 for every statistic;
  * skip-function: the function is not extracted, and its row in the functions file has -1 for every statistic;
  * abort: the run stops at the first file with a parse error.

Output functions CSV format:
  * id: repository ID
  * path: path to the extracted function file
  * name: function or method name
  * position: starting line and column in the original source file
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
  * parse_error: position of the first parse error relative to the function, or none

Output function logs CSV format:
  * id: repository ID
  * name: source file path
  * language: programming language
  * functions: number of functions found in the file
  * functions_with_kw: number of retained functions
  * ...: number of retained functions matching each keyword file
  * parse_error: position of the first parse error in the file, none, or not-found