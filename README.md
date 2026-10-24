# Word Ninja Rust Lua

[Wordninja Rust](https://github.com/chengyuhui/wordninja-rs) lua native module.

[![MIT licensed][mit-badge]][mit-url] [![AUR][aur-badge]][aur-url]

[mit-badge]: https://img.shields.io/badge/license-MIT-blue.svg
[mit-url]: LICENSE
[aur-badge]: https://img.shields.io/aur/version/wordninja-rs-lua-git.svg
[aur-url]: https://aur.archlinux.org/packages/wordninja-rs-lua-git

**You MUST rename target file to `wordninja.so` (or `wordninja.dll` if use Windows) before require it in Lua.**

## How to use

Depends `rust` `lua5.4`/`lua5.3`

``` shell
$ cargo build --release --features "lua54"
$ cp target/release/libwordninja_rs_lua.so wordninja.so

$ lua5.4 -e 'print(require("wordninja").split("iloverust"))'
i love rust
```

## Extend the preset vocabulary

The original `wordninja.split(text)` API always uses the unchanged preset model.
The loaders below return independent models with a `model.split(text)` function
(dot syntax, not `model:split(text)`). Loading a new model never changes existing
models or the module-level splitter. Load once at initialization, not per keystroke.

### Plain word lists

Create a UTF-8 file such as `words.txt`:

```text
# word [cost]
serde
vecdeque 6
```

```lua
local wordninja = require("wordninja")
local model = wordninja.load_words("words.txt") -- optional second argument: default cost
print(model.split("useserde")) -- use serde
print(model.count, model.skipped)
```

Words are normalized to ASCII lowercase and must contain an ASCII letter or
digit, with only ASCII letters, digits, and apostrophes allowed (maximum 128
bytes). Empty lines and `#` comments are ignored; UTF-8 BOM and CRLF are supported.
Costs must be finite and in `(0, 100]`; the default is `6`. Lower costs favor keeping
a token whole, but do not force a match. Duplicate words use the last valid entry.
Explicit entries can override a preset word's cost in this model only.

Invalid lines are skipped. The result includes `count` (unique accepted words),
`skipped`, `skipped_lines` (1-based line numbers), `source`, and `default_cost`.
Unreadable/non-UTF-8 files, files larger than 1 MiB, and invalid default costs raise
Lua errors; callers can use `pcall` to retain the preset splitter on failure.

### Reuse Rime English dictionaries

```lua
local wordninja = require("wordninja")
local model = wordninja.load_rime_dir("en_dicts")
print(model.split("usevecdeque")) -- use vecdeque, if VecDeque is in the dictionaries
print(model.has_word("VecDeque"))
print(model.word_cost("the"))
```

This scans regular `*.dict.yaml` files directly in the directory, without
recursion or following symlinks. It parses the Rime YAML header (`---` through
`...`) and supports `columns` (default: `text`, `code`, `weight`), UTF-8 BOM,
CRLF, blank lines, and full-line comments. ASCII word fragments from `text` are
collected using the splitter's existing punctuation boundaries; a whole `code`
field is also collected if it is a valid word. For example, `String::from` yields
`string` and `from`; a `stringfrom` code is an additional spelling. Headers,
comments, and weight columns are not vocabulary. Rime weights are not used as
segmentation costs.

Words are lowercased and deduplicated across all files. Existing preset words
keep their original costs; only new words receive cost `6`. This avoids flattening
the preset frequency ranking when importing a large English dictionary. Input
case is preserved in the split output. Each file is limited to 16 MiB; tokens use
the same validation and 128-byte limit as plain word lists. Invalid tokens are
skipped, while malformed headers and unreadable/non-UTF-8 files raise Lua errors.
Reload the model after editing dictionaries; files are not watched automatically.
An empty directory returns a preset-only model.

The returned table includes:

- `split(text)`, `has_word(word)`, `word_cost(word)` (lookups ignore ASCII case).
- `files`, `entries` (nonblank, noncomment body rows), and `count` (unique words).
- `added`, `preset_duplicates`, `duplicates` (repeated valid token insertions),
  and `skipped` (rows with no usable token).
- `source` (the supplied directory path).

`count == added + preset_duplicates`. The module also exposes
`wordninja.word_cost(word)` to inspect the unchanged preset model.

### Dependency and tests

The `wordninja` dependency stays on the unmodified crates.io release, pinned to
`0.1.0`. No dependency fork, vendored crate, or preset word-list file is included
in this repository.

The published dependency does not expose its vocabulary or scores. `build.rs`
therefore asks `cargo metadata` for the resolved dependency's location and uses
its bundled `wordninja_words.txt` as a compile-time input. This does not assume a
Cargo cache layout and works with source replacement / `cargo vendor` as long as
the original crate files are preserved. The metadata command runs with
`--offline --locked`; the build script does not download a separate word list.
Normal Cargo dependency fetching is still required for a first build. At runtime
the vocabulary is embedded, so neither Cargo nor external preset files are needed.

`src/model.rs` adapts the MIT-licensed scoring and splitting implementation for
owned vocabulary additions. Its preset vocabulary comes only from the original
dependency. The model uses `Cow<str>` keys and adds score lookup/insertion; the
preset ranking and splitting algorithm are unchanged. Module-level `split` still
calls the original dependency directly. Tests compare the extension's preset
splitting with the original dependency on every preset entry and regression inputs.

```sh
cargo test --features lua54
cargo build --release --features lua54
lua5.4 tests/smoke.lua target/release/libwordninja_rs_lua.so
```

For Lua 5.3, use `lua53` / `lua5.3`. On Windows, pass
`target/release/wordninja_rs_lua.dll` to the smoke test. The smoke test loads the
specified artifact directly, without renaming or installing it.
