Collects random IDs of public projects on GitHub, along with their names and whether each project is a fork.
IDs are selected at random with replacement (so the same ID can appear multiple times).
On Github, project IDs are generally assigned in increasing chronological order.
By default, the maximum queryable ID corresponds to a project created on 2026/01/05.
The IDs are stored in a CSV file at a location provided as an argument.
If the program is interrupted, it can be restarted and will resume from the last ID sampled.
IDs are sampled in sequential chunks of 100.

Output CSV file format:
 * id: ids of the projects.
 * name: full names (i.e., username/projectname) of the projects.
 * fork: whether projects are fork (1) or not (0).
 * requests: number of requests made to the Github API (roughly row number / 100).