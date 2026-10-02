Filters a CSV file containing metadata of GitHub repositories according to user-specified criteria.
The input file must be a valid CSV file produced by the 'metadata' command.
Repositories that could not be queried by the 'metadata' command (their name column contains an HTTP error such as 'http/2 404') are always discarded.
Repositories can be filtered using the following criteria:
  *  Size: repositories with a size (in kB) below a specified threshold are discarded.
  *  Age: repositories with an age (in days) below a specified threshold are discarded.
  *  Disabled: disabled repositories can be excluded.
  *  Non-code: repositories that do not contain source code can be excluded (for example, repositories containing only documentation, data, or binary files).
The filtered metadata are written to a new CSV file. By default, the output file name is the input file name with the suffix '.filtered.csv'.

Output CSV file format:
  * all columns from the input file except 'created' and 'pushed', which are replaced by:
  * age: repository age in days, computed as the difference between the last push and the repository creation date, or 0 if the last push is earlier than the creation date;