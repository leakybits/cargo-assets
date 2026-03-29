fmt:
    cargo fmt
    taplo fmt

lint:
    cargo clippy --all-targets
    cargo fmt --check
    taplo fmt --check

hooks:
    cp hooks/* .git/hooks/
