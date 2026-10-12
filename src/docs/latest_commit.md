Pins GitHub repositories to the latest commit of their default branch, so that download and the links in the following steps use this commit.

The input file must be a valid CSV file with the IDs of the repositories (column 'repo_id') and their source (column 'source'). The other repository columns of the input ('repo_name' and 'repo_license') are copied to the output, and its other columns are ignored. With --infer-source, repositories without a source are on GitHub if they have an ID, and local otherwise.

Repositories are processed in random order using a reproducible seed. For each repository, the command queries the GitHub API for the latest commit of its default branch, and writes it in the latest_commit column, replacing the commit of the input if any. This step is optional: download downloads repositories without a commit from their default branch.

If a repository cannot be queried, for example because it was deleted, turned private or has no commit, its row contains the error message in the repo_name column and its url is empty. The message is the HTTP status, such as 'http/2 404', or starts with 'error:' for other errors, such as network errors or repositories that are not on GitHub. download does not download these repositories.

Results are written to a CSV file. By default, the output file name is the input file name with the suffix '.latest_commit.csv'.

With `--threads`, several repositories are queried at the same time (one by default). The rows are then written in the order in which the queries complete.

If interrupted, the command can resume from the existing output file unless --force is used. A random subset of repositories can also be processed.

Columns of the input can be read under other names with --rename OLD=NEW, for example --rename id=repo_id.

Output CSV format:
  * repo_id: repository ID;
  * source: where the repository comes from;
  * repo_name: full repository name (owner/repository);
  * repo_license: repository license, copied from the input;
  * latest_commit: SHA of the latest commit of the default branch;
  * url: link to the repository on GitHub, at its latest commit.
