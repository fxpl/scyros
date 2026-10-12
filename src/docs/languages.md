Collects the programming languages used by GitHub repositories.

The input file must be a valid CSV file with the IDs of the repositories (column 'repo_id'), their full names (column 'repo_name') and their source (column 'source'). The other repository columns of the input ('repo_license' and 'latest_commit') are copied to the output, and its other columns are ignored. With --infer-source, repositories without a source are on GitHub if they have an ID, and local otherwise.

Repositories are processed in random order using a reproducible seed. For each repository, the command queries the GitHub API for its language breakdown. Optionally, the output of a previous run with the same columns can be used as a cache to reuse earlier results.

If a repository cannot be queried, for example because it was deleted or turned private, its row has an error code in the 'error' column, such as E404, and an empty languages column. Rows of the input that already have an error are copied without being queried. At the end, the command prints how many rows have each code; --debug also prints every error with its details. The codes are explained by scyros explain.

Results are written to a CSV file. By default, the output file name is the input file name with the suffix '.languages.csv'.

With `--threads`, several repositories are queried at the same time (one by default). The rows are then written in the order in which the queries complete.

If interrupted, the command can resume from the existing output file unless --force is used. A random subset of repositories can also be processed.

Columns of the input can be read under other names with --rename OLD=NEW, for example --rename id=repo_id.

Output CSV format:
  * repo_id: repository ID;
  * source: where the repository comes from;
  * repo_name: full repository name (owner/repository);
  * repo_license: repository license, copied from the input;
  * latest_commit: commit of the repository, copied from the input;
  * url: link to the repository on GitHub, at its commit if known;
  * error: error code if the repository could not be queried, empty otherwise;
  * languages: semicolon-separated 'language:size' pairs.