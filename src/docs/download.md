Downloads GitHub repositories from a CSV file, or reads local ones, and filters their contents using user-defined extension and keyword rules.

Each repository of the input is either analyzed in place, if it has a 'local_path' with its directory, or downloaded from GitHub with its 'repo_id' otherwise. Inputs can mix both kinds of repositories. Downloads require --tokens and --dest; without downloads, --threads sets the number of threads. The input must also have a 'source' column, unless --infer-source is used: repositories without a source are then on GitHub if they have an ID, and local otherwise. The other repository columns of the input ('repo_name', 'repo_license' and 'latest_commit') are copied to both output files, so that every file keeps the repository it comes from. Other columns are ignored. Columns of the input can be read under other names with --rename OLD=NEW, for example --rename path=local_path.

Repositories are processed in random order using a reproducible seed. Repositories without a local path are fetched from GitHub at their commit, or from their default branch when they have no commit, and extracted locally. The commit of a repository downloaded from its default branch is read from the archive and written to the 'latest_commit' column. Each repository is then scanned for files whose extensions match those defined in one or more keyword JSON files. Keywords are either interpreted as regular expressions or whole words according to the --regex flag. Keywords are matched without regard to letter case, unless --case-sensitive is used. File extensions are always case-sensitive: '.c' does not match a file named 'main.C'.
Files that do not match the allowed extensions or contain none of the keywords are deleted according to --prune: in the downloaded repositories only (downloads, the default), also in the directories given by the input (all), or nowhere (none). Repositories that GitHub does not provide (for example deleted ones, E404), repositories without a local path that are not on GitHub (E010), and archives that cannot be extracted (E020) are logged as error rows: their project log row has an error code in the 'error' column, and empty statistics. Network failures and server errors are retried a few times before giving an error row. Repositories with an error code in the 'error' column of the input, from an earlier step such as metadata, are copied with their code, without being downloaded or analyzed. At the end, the command prints how many rows have each code; --debug also prints every error with its details. The codes are explained by scyros explain. Local paths that do not exist are skipped with a warning.

Both logs have a 'url' column with links to the repositories and to the files on GitHub, at the commit of the repositories, or on their default branch without a commit. Links are empty for repositories that are not on GitHub or have no name.

The command writes two CSV files: a project-level log with aggregate statistics and a file-level log with one row per retained file. By default, their names are the input file name with the suffixes '.project_log.csv' and '.file_log.csv'.

If the command is run again without --force, it resumes from the existing project log, where repositories are identified by their ID, or by their local path when they have no ID. The format of the keyword JSON files is as follows:
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
  * repo_id, source, repo_name, repo_license, latest_commit: repository columns, copied from the input, empty when unknown
  * url: link to the repository on GitHub
  * error: error code if the repository could not be downloaded or analyzed, empty otherwise
  * local_path: local repository path, empty if the repository could not be downloaded
  * files / loc / words — totals before keyword filtering
  * files_with_kw / loc_with_kw / words_with_kw — totals for files matching at least one keyword set
  * files_with_... / loc_of_files_with_... / words_of_files_with_... — totals for each keyword file
  * ... — number of keyword matches for each keyword file

Output file log format:
  * repo_id, source, repo_name, repo_license, latest_commit: repository columns of the repository of the file
  * local_path: local file path
  * file: path of the file inside its repository
  * url: link to the file on GitHub
  * language: language inferred from the file extension
  * loc: number of lines
  * words: number of words
  * ...: number of keyword matches for each keyword file