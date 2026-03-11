Removes duplicate rows from a CSV file.

Two rows are considered duplicates if they share the same value in a user-specified column. By default, the command uses the 'id' column, which typically contains repository IDs.

Prints statistics about the number of duplicates found and writes the deduplicated rows to a new CSV file.

By default, the output file name is the input file name with '.unique.csv' appended. The output format matches the input format, and the header is preserved.