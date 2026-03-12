Downloads GitHub repositories from a CSV file and filters their contents using user-defined extension and keyword rules.

In normal mode, the input file must contain the columns 'id', 'name', and 'latest_commit'. With --skip, it must instead contain 'id' and 'path' for repositories that already exist locally. Other columns are ignored.

Repositories are processed in random order using a reproducible seed. In download mode, each repository is fetched from GitHub at the specified commit, extracted locally, and scanned for files whose extensions match those defined in one or more keyword JSON files. Files that do not match the allowed extensions are removed, and files that do not contain any of the specified keywords can also be discarded.

The command writes two CSV files: a project-level log with aggregate statistics and a file-level log with one row per retained file. By default, their names are the input file name with the suffixes '.project_log.csv' and '.file_log.csv'.

If the command is run again without --force, it resumes from the existing project log. With --count, it computes statistics without deleting files. With --skip, it computes statistics from already downloaded repositories instead of downloading them from GitHub. The format of the keyword JSON files is as follows:
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

Output project log format:
  * id: repository ID
  * path: local repository path, or error if download failed
  * name: full repository name (owner/repository)
  * latest_commit: commit SHA
  * files / loc / words — totals before keyword filtering
  * files_with_kw / loc_with_kw / words_with_kw — totals for files matching at least one keyword set
  * files_with_... / loc_of_files_with_... / words_of_files_with_... — totals for each keyword file
  * ... — number of keyword matches for each keyword file

Output file log format:
  * id: repository ID
  * name: file path
  * language: language inferred from the file extension
  * loc: number of lines
  * words: number of words
  * ...: number of keyword matches for each keyword file