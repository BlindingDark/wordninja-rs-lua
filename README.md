# Word Ninja Rust Lua

[Wordninja Rust](https://github.com/chengyuhui/wordninja-rs) lua native module.

[![MIT licensed][mit-badge]][mit-url] [![AUR][aur-badge]][aur-url]

[mit-badge]: https://img.shields.io/badge/license-MIT-blue.svg
[mit-url]: LICENSE
[aur-badge]: https://img.shields.io/aur/version/wordninja-rs.svg
[aur-url]: https://aur.archlinux.org/packages/wordninja-rs

**You MUST rename target file to `wordninja.so` (or `wordninja.dll` if use Windows) before require it in Lua.**

## How to use

Depends `rust` `lua5.4`/`lua5.3`

``` shell
$ cargo build --release --features "lua54"
$ cp target/release/libwordninja_rs_lua.so wordninja.so

$ lua5.4 -e 'print(require("wordninja").split("iloverust"))'
i love rust
```
