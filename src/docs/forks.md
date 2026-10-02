Discards forks from a CSV file.
The file must contain a column (by default 'fork', see --column) whose value is 1 for forks and 0 for other projects. Entries without a value are reported as errors.
Prints statistics about the number of forks found in the file and writes the non-forked projects to a new CSV file.
By default, the output file name is the same as the input file name with ".non-forks.csv" appended.

Output CSV file format:
  * Same columns as the input file