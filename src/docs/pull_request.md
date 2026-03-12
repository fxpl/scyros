Collects pull requests for GitHub repositories and stores their metadata together with the associated discussion comments.
The input file must be a valid CSV file containing one column with repository IDs and one column with full repository names. By default, these columns are named 'id' and 'name', but both can be customized.

Repositories are processed in random order using a reproducible shuffle controlled by a seed. For each repository, the command queries the GitHub API to retrieve all pull requests, including open, closed, and merged pull requests.

For each pull request, the command also retrieves the pull request body and all associated comments, including general discussion comments, code review comments, and review summaries. These comments are written to a separate CSV file in the destination directory.

The pull request metadata are written to a CSV file. By default, the output file name is the input file name with the suffix .pulls.csv.

If the program is interrupted, it can be restarted and will resume from the repositories already present in the output file, unless --force is used. A random subset of repositories can also be processed by specifying --sub.

Output pull-requests CSV format:
  * id: repository ID
  * name: full repository name (owner/repository)
  * pr_number: pull request number
  * file_path: path to the CSV file containing the pull request discussion
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