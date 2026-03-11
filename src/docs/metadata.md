Collects metadata for GitHub repositories listed in a CSV file.

The input must be a valid CSV file where one column (by default 'name') contains the full repository names and another column (by default 'id') contains their IDs. Both column names can be customized.

The command queries the GitHub API to retrieve metadata for each repository. Repositories are processed in random order without replacement. The collected metadata are written to a new CSV file.

By default, the output file name is the input file name with the suffix '.metadata.csv'.

If the program is interrupted, it can be restarted and will resume from where it left off. Optionally, a cache file can be used to store API responses and avoid repeating requests.

Output CSV file format:
  * id: repository ID;
  * name: full repository name (owner/repository);
  * language: primary programming language;
  * created: repository creation date;
  * pushed: date of most recent push;
  * updated: date of most recent update;
  * fork: whether the repository is a fork (1) or not (0);
  * disabled: whether the repository is disabled (1) or not (0);
  * stars: number of stars
  * forks: number of forks;
  * issues: number of open issues;
  * has_issues: whether issues are enabled (1) or not (0);
  * watchers_count: number of watchers;
  * susbcribers: number of subscribers;
  * size: repository size in kB;
  * license: repository license.
