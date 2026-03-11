Collect pull requests of GitHub projects. The input file must be a valid CSV file where one of the columns (\"name\") contains the full names of the projects, and another one (\"id\") contains their ids.\n\
The program sends requests to the GitHub API to collect metadata about the pull requests of the projects in the input file, as well as the comments in each pull request.\n\
Projects are chosen randomly without replacement. The metadata of the pull requests is saved in a new CSV file.\nIf the program is interrupted, it \
can be restarted and will continue from where it left off.\n The comments of each pull request are saved in separate CSV files in the target directory.\n\
By default, the name of the output file is the same as the input file with the suffix '.pulls.csv'.