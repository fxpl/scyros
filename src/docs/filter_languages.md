Filters a CSV file produced by the 'languages' command to keep only repositories that contain code written in at least one user-specified language.

The languages to keep are read from a JSON file. The format of the JSON file is as follows:
{
    "languages": ["lang1", "lang2", ...],    // list of languages to keep
}

Repositories that are unreachable, such as deleted or private repositories, are discarded before filtering. A repository is retained if its languages field contains at least one language from the provided list.

By default, the filtered data are written to a CSV file whose name is the input file name with the suffix '.filtered_lang.csv'.

Output CSV format:
  * Same columns as the input file