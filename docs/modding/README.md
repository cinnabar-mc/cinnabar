# Cinnabar Modding

This directory is a standalone mdBook. Its committed Markdown needs only
[mdBook 0.5.4](https://github.com/rust-lang/mdBook/releases/tag/v0.5.4) to build:

```sh
mdbook build docs/modding
```

It uses no custom preprocessors, network access or files outside this directory.
The guides explain how to build personal, paired and packet mods. Existing author
documentation is preserved in the appendices.

From a full Cinnabar checkout, regenerate the API pages and navigation with:

```sh
cargo run --locked -p mod-docs
cargo run --locked -p mod-docs -- --check
cargo test --locked -p mod-docs
```

Edit API documentation in the WIT source. Each interface documents its grants;
every named type and function needs a doc comment. Limit values come directly
from the Rust constants. The `committed_book_is_current` test rejects changed,
missing and obsolete generated pages, including the book's summary.
