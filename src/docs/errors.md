Error codes say why a step could not process a row: a repository, a file or a function. `scyros explain <CODE>` explains a code.

metadata, languages, latest_commit, pr, download and parse write the rows they cannot process with a code in their 'error' column, after 'url'. The cells they could not fetch are empty, and the values of the input are kept. Rows of the input that already have an error are copied without being processed again, and filter_metadata and filter_languages discard them. At the end of a command, the terminal shows how many rows have each code. With --debug, it also shows every error with its details.

Errors returned by GitHub have the code of their HTTP status, such as E404. The other codes start with E0.

Codes:
  * E001 network failure: a request got no answer from GitHub, for example because of the network. The error is usually temporary, but resuming a step does not retry the rows already in its output: remove them from the output, or run the step again with --force.
  * E002 invalid answer: GitHub answered, but the answer could not be read.
  * E010 unsupported source: the repository is not on GitHub, so it cannot be queried or downloaded.
  * E011 no commit: the default branch of the repository has no commit.
  * E020 extraction failed: the archive of the repository could not be extracted.
  * E030 parse error: the file or the function has a parse error, and was skipped because of the --failures policy of parse.
  * E031 file too large: the file is too large to be parsed.
  * E401 bad credentials: GitHub rejected the token.
  * E403 forbidden: GitHub refused access to the repository, for example because it is blocked. Rate limits are not errors: they are waited out.
  * E404 not found: the repository was deleted, turned private, or never existed.
  * E409 empty repository: the repository has no content.
  * E451 unavailable for legal reasons: the repository is blocked, for example after a DMCA takedown.
  * E5xx server error: GitHub failed to answer, even after a few retries. The error is usually temporary, but resuming a step does not retry the rows already in its output: remove them from the output, or run the step again with --force.

Other HTTP statuses have the code of the status and the description 'HTTP error'.
