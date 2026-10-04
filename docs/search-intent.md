# Intent search

Ordinary single-word searches retain operation-name exact and typo priority,
substring discovery, and operation-name fuzzy matching.

Multiword searches match all supplied words as whole, case-insensitive Unicode
alphanumeric tokens across operation names, paths, summaries, descriptions,
display names, effective groups, and aliases. Punctuation separates tokens;
extra spaces do not matter. No words are discarded: `pull request` can match
“Get a pull request”, while `pull a request` requires the article too. Tokens
are not stemmed (`requests` differs from `request`) or fuzzily joined across
fields. Complete matches in names and concise summaries rank above scattered
or incidental description matches. Ties use stable API and command ordering.

A leading HTTP method in a multiword query, in any case, restricts results to
that method: `GET pull request` (also `get pull request`) finds retrieval
operations, and `POST create` finds POST creation operations. This is a filter,
not a requirement that the summary literally contain the method. A method
alone retains ordinary single-word discovery behavior.

Empty or whitespace-only input returns no results. `--api` restricts the API.
Explicit `regex:` queries retain Rust regex syntax, case sensitivity unless
requested by the pattern, and Unicode semantics; intent rules do not apply.
