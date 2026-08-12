Detects duplicate files in a dataset and retains only unique files.

The input file must be a valid CSV file containing a column of file paths. By default, this column is named 'name', but another column can be selected with --header.

Three similarity criteria are available through -s:
  * exact: files must match byte-for-byte. This is the only criterion sensitive to whitespace, comments and token order.
  * bow: files are compared by their bag of tokens, which ignores token order and whitespace. Any added, removed or altered token still breaks the match.
  * overlap: files are compared by how many tokens they have in common, so files differing by a few statements still count as duplicates. This finds near-miss duplicates that the other two criteria miss, at a substantially higher runtime cost.

The exact and bow criteria hash every file and group files with equal hashes, so their cost grows linearly with the dataset. The overlap criterion has to compare files against one another, and uses the prefix filtering technique of SourcererCC to keep the number of comparisons manageable.

Two files are duplicates under the overlap criterion when the number of tokens they share reaches --threshold (0.8 by default) times the token count of the longer of the two.  A threshold of 1.0 therefore requires the two files to hold exactly the same tokens with the same multiplicities, while lower thresholds tolerate more edits, report more duplicates and take longer to run.

Comparing every pair of files does not scale, so each file is first reduced to a prefix of its rarest tokens, and only files whose prefixes share enough tokens are then compared in full. --prefix-depth sets how far that prefix may be extended: a deeper prefix rejects more files before the full comparison, but costs more index lookups to build and to query. The depth is a ceiling rather than a target, since the detector stops extending the prefix as soon as the extra lookups cost more than the comparisons they save.

Files that are similar without being identical are similar in their syntax, and so are written in the same language. Under the overlap criterion files are therefore only compared against other files of the same language, which also keeps each group of comparisons small. Languages are read from the JSON files passed to --languages, which map file extensions to language names. Without --languages, every file is compared against every other one regardless of language. The other two criteria compare file contents alone and ignore --languages entirely.

Files too large to load are ignored.

The command writes two CSV files: one containing the unique files and one containing the mapping from each file to the representative of its duplicate group. By default, these files are named by appending '.unique.csv' and '.duplicates_map.csv' to the input file name.

Output unique-files CSV format:
  * All columns from the input file, plus count for the duplicate-group size

Output duplicates-map CSV format:
  * name: file path
  * original: representative file path
