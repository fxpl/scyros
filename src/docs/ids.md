Collects random IDs of public GitHub repositories on GitHub and record their names and fork status.
Repository IDs can be sampled uniformly at random with replacement, meaning the same ID may be sampled multiple times, or in order. 

Note: GitHub assigns repository IDs in approximately increasing chronological order.

By default, the maximum allowed ID corresponds to a repository created on 2026-01-05.

Results are written to a CSV file at the path specified by the user. If the program is interrupted, it can be restarted and will resume from the last sampled ID.

IDs are processed in sequential batches of 100, with one GitHub API request per batch.

Output CSV file format:
 * id: repository ID.
 * name: full repository name (owner/repository).
 * fork: whether the repository is a fork (1) or not (0).
 * requests: number of GitHub API requests performed (approximatively row_number / 100).