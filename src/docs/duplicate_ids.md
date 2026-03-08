Discards duplicates in a CSV file. Two entries are considered to be duplicates if they share the same value in a column specified by the user.
By default, this column stores repository ids, and has "id" as a header.
Prints statistics about the number of duplicates found in the file and write the unique rows to a new CSV file.
By default, the output file name is the same as the input file name with ".unique.csv" appended. The format of the output file is the same as the input file, and the header is preserved.