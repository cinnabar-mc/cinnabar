/// Generates committed pages, or checks them without changing the book.
fn main() -> anyhow::Result<()> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    anyhow::ensure!(
        args.is_empty() || args == ["--check"],
        "usage: mod-docs [--check]"
    );
    mod_docs::generate(&mod_docs::repository(), !args.is_empty())
}
