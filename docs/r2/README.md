# docs/r2

This directory holds the documents of the workflow run, which builds the
schema, the derived store and the host that turn what Halvo captures into
workflows.

- `trials/` will hold the records of the cost trials of phase P0: what each
  capture layer costs on the hosted macOS and Windows runners.
- The text on the egress check will say what the check watches and how to read
  its result.
- The retention text will say how long each kind of record is kept and when it
  is deleted.

No command reads these files. Git checks out every file here as text with LF
line ends, as `.gitattributes` in this directory says.
