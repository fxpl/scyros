"Parse all the files in the input file and extract functions whose body contains one of the provided keywords. \
            All parsed files repositories are logged in a CSV file where statistics about the functions are stored. \
            These statistics include the number of lines of code, the number of words, the number of keywords matched, the number of conditional statements, loops,
            and the maximum nesting level of these statements.\n\
            The name of the log file is the same as the input file with the extension \".functions\". \
            The functions are stored in a folder with the same name as the file and the extension \"_functions\".\n\
            The supported languages are C, C++, Java, Python and Fortran."