# Security Policy

## Supported versions

The latest released version receives security fixes.

## Reporting a vulnerability

Please report privately rather than in a public issue: open a
[GitHub security advisory](https://github.com/CSCPadova/lytk/security/advisories/new),
or email spanio@dei.unipd.it. We aim to acknowledge within a week.

Please include the input file that triggers the problem and the version of lytk.

## Threat model

lytk parses untrusted files (LilyPond, MusicXML, MXL, MIDI, ABC, Humdrum). Bugs
reachable from a malicious input file are in scope, in particular:

- panics or unbounded memory/CPU use while parsing (a reachable
  denial-of-service for any service that converts user uploads)
- path traversal or unintended file reads — note that LilyPond `\include` is
  followed by design, so callers that accept untrusted `.ly` files should run
  conversion in a sandbox and pass only trusted `-I` search paths
- decompression bombs in `.mxl` archives (these are read with a size cap)

Release builds keep `overflow-checks` on, so an integer overflow panics rather
than silently producing corrupt output. Out of scope: crashes from inputs the
caller has already been told are trusted, and resource use proportional to a
legitimately large score.
