//TODO: https://github.com/messense/cargo-options/blob/086d7470cae34b0e694a62237e258fbd35384e93/examples/cargo-mimic.rs
// maybe https://lib.rs/crates/clap-cargo

use std::{
    env,
    path::PathBuf,
    process::{Command, ExitCode},
};

use anyhow::{anyhow, bail, Result};

/*

\cargo green +nightly fmt --all

\cargo green clippy --locked --frozen --offline --all-targets --all-features -- -D warnings --no-deps -W clippy::cast_lossless -W clippy::redundant_closure_for_method_calls -W clippy::str_to_string

\cargo green auditable build --locked --frozen --offline --all-targets --all-features

\cargo green test --all-targets --all-features --locked --frozen --offline

\cargo green nextest run --all-targets --all-features --locked --frozen --offline

*/

//TODO test
// \cargo green +nightly --version # check displayed vsn
// \cargo green --version # check != displayed vsn
// \cargo green # check displays help

fn main() -> Result<ExitCode, Box<dyn std::error::Error>> {
    let pwd = "/home/pete/.cargo/git/checkouts/cross-dac8861107f29545/88f49ff";
    let repo = gix::open(pwd).map_err(|e| anyhow!("Failed reading Git repo at {pwd}: {e}"))?;
    for entry in repo.entries() {
        eprintln!(">>> {entry:?}")
    }

    let mut args = env::args().skip(1); // skips $0

    // skips "green"
    if args.next().is_none() {
        // Warn when running this via `cargo run -p cargo-green -- ..`
        eprintln!(
            r#"
    The `cargo-green` binary needs to be called via cargo, e.g.:
        cargo green build
"#
        );
        return Ok(ExitCode::FAILURE);
    }

    let mut cmd = Command::new(env::var("CARGO").unwrap_or("cargo".into()));
    if let Some(arg) = args.next().as_deref() {
        cmd.arg(arg);
        if arg == "supergreen" {
            // FIXME: handle commands
        }
        if arg.starts_with('+') {
            cmd = Command::new(which::which("cargo").unwrap_or("cargo".into()));
            cmd.arg(arg);
            // TODO: reinterpret `rustc` when given `+toolchain`
        }
    }
    cmd.args(args);

    if let Err(e) = (|| -> Result<()> {
        let bin = ensure_binary_exists("rustcbuildx")?;

        // TODO pull-images
        // TODO read package.metadata.green
        // TODO: TUI above cargo output

        cmd.env("RUSTCBUILDX_LOG", env::var("RUSTCBUILDX_LOG").unwrap_or("info".to_owned()));
        cmd.env(
            "RUSTCBUILDX_LOG_PATH",
            env::var("RUSTCBUILDX_LOG_PATH").unwrap_or("/tmp/cargo-green.log".to_owned()),
        );
        if let Ok(ctx) = env::var("RUSTCBUILDX_CACHE_IMAGE") {
            cmd.env("RUSTCBUILDX_CACHE_IMAGE", ctx);
        }
        if let Ok(wrapper) = env::var("RUSTC_WRAPPER") {
            bail!(
                r#"
    You called `cargo-green` but a $RUSTC_WRAPPER is already set (to {wrapper})
        We don't know what to do...
"#
            )
        }
        cmd.env("RUSTC_WRAPPER", bin);

        Ok(())
    })() {
        eprintln!("{e}");
        return Ok(ExitCode::FAILURE);
    }

    // eprintln!(">>> {:?}", cmd.get_program()); // FIXME: drop
    // eprintln!(">>> {:?}", cmd.get_args()); // FIXME: drop
    // eprintln!(">>> {:?}", cmd.get_envs()); // FIXME: drop
    let status = cmd.status()?;
    Ok(status.code().map_or(ExitCode::FAILURE, |code| ExitCode::from(code as u8)))
}

fn ensure_binary_exists(name: &'static str) -> Result<PathBuf> {
    which::which(name).map_err(|_| {
        anyhow!(
            r#"
    You called `cargo-green` but its dependency `rustcbuildx` cannot be found.
    Please run:
        # \cargo install --locked rustcbuildx
"#
        )
    })
}
