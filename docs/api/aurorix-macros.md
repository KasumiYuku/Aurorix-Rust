# aurorix-macros API 索引

> 本文件由 `cargo run -p xtask -- gen-api` 从源码生成，请勿手改。完整签名与泛型约束用 `cargo doc --workspace --no-deps --open` 查看。

## `src/lib.rs`

- `pub fn command(attr: TokenStream, item: TokenStream) -> TokenStream`
- `pub const #const_name: (&'static str, bool) = (#command_name, #admin)`

