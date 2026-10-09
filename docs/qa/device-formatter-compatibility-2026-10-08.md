# Renderer diagnostic format compatibility

This note describes the diagnostic parser and its source tests. It does not establish Windows runtime, visual, dependency-rights or release acceptance.

The parser handles the formatter patterns used by the locked tracing and renderer dependencies, including adjacent ANSI styles, fixed fieldless spans, escaped adapter information and multiline shader diagnostics. Its synthetic fixtures exercise those source-defined formats.

Public observations are restricted to bounded, recognized fields and redacted excerpts. Variable resource labels, paths and driver prose are not treated as publishable diagnostic text. Unsupported formatting remains unclassified. A recognized error category is an observation, not proof of its cause.

The tests cover valid formatter output and hostile or malformed variants. Source and policy identities, current-run evidence and release decisions are checked independently by their respective consumers. Passing these parser tests does not grant a publication or licensing approval.
