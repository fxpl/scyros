Collects the programming languages used by GitHub repositories and the SHA of their latest commit.

The input file must be a valid CSV file containing repository IDs and full repository names. By default, these columns are named 'id' and 'name', but the column names can be customized. Other columns are ignored.

Repositories are processed in random order using a reproducible seed. For each repository, the command queries the GitHub API for its language breakdown and latest commit SHA. Optionally, a cache file from a previous run can be used to reuse earlier results.

Results are written to a CSV file. By default, the output file name is the input file name with the suffix '.languages.csv'.

If interrupted, the command can resume from the existing output file unless --force is used. A random subset of repositories can also be processed.

Output CSV format:
  * id: repository ID;
  * name: full repository name (owner/repository);
  * languages: semicolon-separated 'language:size' pairs;
  * latest_commit: SHA of the latest commit.