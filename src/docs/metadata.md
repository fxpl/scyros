Collects metadata for GitHub repositories listed in a CSV file.

The input must be a valid CSV file with the IDs of the repositories (column 'repo_id'), their full names (column 'repo_name') and their source (column 'source'). The other repository columns of the input ('repo_license' and 'latest_commit') are copied to the output, and its other columns are ignored. With --infer-source, repositories without a source are on GitHub if they have an ID, and local otherwise.

The command queries the GitHub API to retrieve metadata for each repository. Repositories are processed in random order without replacement. The collected metadata are written to a new CSV file.

With `--threads`, several requests are sent at the same time (one by default). The rows are then written in the order in which the requests complete.

By default, the output file name is the input file name with the suffix '.metadata.csv'.

If a repository cannot be queried, for example because it was deleted or turned private, its row has an error code in the 'error' column, such as E404, and empty metadata columns. Rows of the input that already have an error are copied without being queried. At the end, the command prints how many rows have each code; --debug also prints every error with its details. The codes are explained by scyros explain.

If the program is interrupted, it can be restarted and will resume from where it left off. Optionally, the output of a previous run with the same columns can be used as a cache to avoid repeating requests.

Columns of the input can be read under other names with --rename OLD=NEW, for example --rename id=repo_id.

Output CSV file format:
  * repo_id: repository ID;
  * source: where the repository comes from;
  * repo_name: full repository name (owner/repository);
  * repo_license: repository license, or 'unknown';
  * latest_commit: commit SHA, copied from the input;
  * url: link to the repository on GitHub;
  * error: error code if the repository could not be queried, empty otherwise;
  * language: primary programming language;
  * created: repository creation date;
  * pushed: date of most recent push;
  * updated: date of most recent update;
  * fork: whether the repository is a fork (1) or not (0);
  * disabled: whether the repository is disabled (1) or not (0);
  * archived: whether the repository is archived (1) or not (0);
  * stars: number of stars;
  * forks: number of forks;
  * issues: number of open issues;
  * has_issues: whether issues are enabled (1) or not (0);
  * watchers_count: number of watchers;
  * subscribers: number of subscribers;
  * size: repository size in kB.
