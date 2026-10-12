Collects pull requests for GitHub repositories and stores their metadata together with the associated discussion comments.
The input file must be a valid CSV file with the IDs of the repositories (column 'repo_id'), their full names (column 'repo_name') and their source (column 'source'). The other repository columns of the input ('repo_license' and 'latest_commit') are copied to the output, and its other columns are ignored. With --infer-source, repositories without a source are on GitHub if they have an ID, and local otherwise. Columns of the input can be read under other names with --rename OLD=NEW, for example --rename id=repo_id.

Repositories are processed in random order using a reproducible shuffle controlled by a seed. For each repository, the command queries the GitHub API to retrieve all pull requests, including open, closed, and merged pull requests.

For each pull request, the command also retrieves the pull request body and all associated comments, including general discussion comments, code review comments, and review summaries. These comments are written to a separate CSV file in the destination directory.

The pull request metadata are written to a CSV file. By default, the output file name is the input file name with the suffix .pulls.csv.

A repository that is not on GitHub gets a single row with the message 'error: unsupported source local' in its repo_name column, and empty url and pull request columns. If the pull requests of a repository cannot be retrieved, no row is written, and the repository is queried again by the next run.

If the program is interrupted, it can be restarted and will resume from the repositories already present in the output file, unless --force is used. A random subset of repositories can also be processed by specifying --sub.

Output pull-requests CSV format:
  * repo_id: repository ID
  * source: where the repository comes from
  * repo_name: full repository name (owner/repository)
  * repo_license, latest_commit: copied from the input
  * pr_number: pull request number
  * url: link to the pull request on GitHub
  * local_discussion_path: local path to the CSV file containing the pull request discussion
  * user: login of the pull request author
  * user_id: GitHub user ID of the pull request author
  * created_at: creation timestamp
  * updated_at: last update timestamp
  * closed_at: closing timestamp, or 0 if the pull request was not closed
  * merged_at: merge timestamp, or 0 if the pull request was not merged
  * draft: whether the pull request is a draft (1) or not (0)
  * state: pull request state

Output pull-request discussion CSV format:
  * id: comment ID
  * user: login of the comment author
  * user_id: GitHub user ID of the comment author
  * type: comment type: body, discussion, code, review, or error
  * created_at: comment timestamp
  * body: comment text