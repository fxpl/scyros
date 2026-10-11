Removes duplicate rows from a CSV file.

Two rows are considered duplicates if they share the same value in a user-specified column. By default, the command uses the 'repo_id' column, which contains repository IDs.

Prints statistics about the number of duplicates found and writes the deduplicated rows to a new CSV file.

By default, the output file name is the input file name with '.unique.csv' appended. 

Columns of the input can be read under other names with --rename OLD=NEW, for example --rename id=repo_id.

Output CSV file format:
  * Same columns as the input file