Detects duplicate files in a dataset and retains only unique files.

The input file must be a valid CSV file containing a column of file paths. By default, this column is named 'name', but another column can be selected with --header. With the exact option, files must match byte-for-byte. With bow, files are compared by bag of words, making the comparison insensitive to token order and whitespace. Files that are too large to load are ignored and excluded from duplicate detection.

The command writes two CSV files: one containing the unique files and one containing the mapping from each file to the representative of its duplicate group. By default, these files are named by appending '.unique.csv' and '.duplicates_map.csv' to the input file name.

Output unique-files CSV format:
  * All columns from the input file, plus count for the duplicate-group size

Output duplicates-map CSV format:
  * name: file path
  * original: representative file path