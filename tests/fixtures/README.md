# Test fixtures

These files are test data, mostly third-party scores. **lytk's MIT licence does
not cover them**: each one keeps the terms of its source. They are used only by
the test suite and are not shipped in the wheels or the source distribution.

| Files | Source | Terms |
|---|---|---|
| `ly/chopin_n.ly` | The Mutopia Project | CC BY-SA 4.0 (see the file header) |
| `ly/example.ly` | The Mutopia Project | CC BY-SA 3.0 (see the file header) |
| `ly/pedal.ly`, `ly/repeats.ly` | The Mutopia Project | Public domain (see the file header) |
| `ly/example2.ly` | baroquemusic.it (MAC200120) | CC BY-NC-ND 4.0 (see the file header) |
| `ly/*.ly` with a `texidoc` header | LilyPond regression tests (`input/regression/`) | LilyPond's licence (GPL) |
| `xml/*.xml` | The MusicXML test suite distributed with LilyPond (`input/regression/musicxml/`) | Upstream terms |
| `midi/*.midi` | Rendered from the `ly/` scores of the same name | As the source score |
| `abc/*.abc` | Written for lytk's tests | MIT, as lytk |
| `ly/<uuid>.ly`, `mxl/*.mxl`, `musicxml/*.mxl` | Provenance not recorded | Treat as the original authors' |

When adding a fixture, record its source and terms here.
