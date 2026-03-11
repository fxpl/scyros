Detects duplicate files in a dataset, returning only unique files. The input and output are CSV files storing file paths.\n\
The name of the column storing file paths in the input CSV file can be specified (default is 'name').\n\
    The similarity criterion can be either exact match or token-based (i.e., invariant to token order and whitespaces).