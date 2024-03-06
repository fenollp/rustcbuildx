use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    env,
    fmt::Write,
    fs::{self, create_dir_all, read_dir, read_to_string, File},
    io::{BufRead, BufReader, ErrorKind, Read},
    process::{Command, ExitCode, Stdio},
    time::Instant,
    unreachable,
};

use anyhow::{bail, Context, Result};
use camino::{Utf8Path, Utf8PathBuf};
use cli::{envs, exit_code, help, pull};
use env_logger::{Env, Target};
use envs::{called_from_build_script, RUSTCBUILDX, RUSTCBUILDX_LOG, RUSTCBUILDX_LOG_STYLE};
use log::{debug, error, info, warn};
use mktemp::Temp;

use crate::{
    envs::{base_image, docker_syntax, maybe_log, pass_env},
    parse::RustcArgs,
    pops::Popped,
};

mod cli;
mod envs;
mod parse;
mod pops;

const PKG: &str = env!("CARGO_PKG_NAME");

// NOTE: this RUSTC_WRAPPER program only ever gets called by `cargo`, so we save
//       ourselves some trouble and assume std::path::{Path, PathBuf} are UTF-8.
//       Or in the words of this crate: https://github.com/camino-rs/camino/tree/8bec62382e1bce1326ee48f6bf93c46e7a4fde0b#:~:text=there%20are%20already%20many%20systems%2C%20such%20as%20cargo%2C%20that%20only%20support%20utf-8%20paths.%20if%20your%20own%20tool%20interacts%20with%20any%20such%20system%2C%20you%20can%20assume%20that%20paths%20are%20valid%20utf-8%20without%20creating%20any%20additional%20burdens%20on%20consumers.

fn main() -> ExitCode {
    let args = env::args().skip(1).collect(); // drops $0
    let vars = env::vars().collect();
    faillible_main(args, vars).unwrap_or(ExitCode::FAILURE)
}

fn faillible_main(args: VecDeque<String>, vars: BTreeMap<String, String>) -> Result<ExitCode> {
    let called_from_build_script = called_from_build_script(&vars);

    let argz = args.iter().take(3).map(AsRef::as_ref).collect::<Vec<_>>();

    let argv = |times| {
        let mut argv = args.clone();
        for _ in 0..times {
            argv.pop_front(); // shift 1
        }
        argv.into_iter().collect()
    };

    match argz[..] {
        [] | ["-h"|"--help"|"-V"|"--version"] => Ok(help()),
        ["pull"] => pull(),
        ["env", ..] => Ok(envs(argv(1))),
        [rustc, "-", ..] =>
             call_rustc(rustc, argv(1)),
        [driver, _rustc, "-"|"--crate-name", ..] => {
            // TODO: wrap driver+rustc calls as well
            // driver: e.g. /home/maison/.rustup/toolchains/stable-x86_64-unknown-linux-gnu/bin/clippy-driver
            // cf. https://github.com/rust-lang/rust-clippy/tree/da27c979e29e78362b7a2a91ebcf605cb01da94c#using-clippy-driver
             call_rustc(driver, argv(2))
         }
        [rustc, opt, ..] if called_from_build_script && opt.starts_with('-') && opt != "-" =>
            // Special case for crates whose build.rs calls rustc, using RUSTC_WRAPPER,
            // but arriving at a wrong conclusion (here: activates nightly-only features, somehow)
            // Workaround: we defer to local rustc instead.
            // See https://github.com/rust-lang/rust-analyzer/issues/12973#issuecomment-1208162732
            // Note https://github.com/rust-lang/cargo/issues/5499#issuecomment-387418947
            // Culprits:
            //   https://github.com/dtolnay/anyhow/blob/05e413219e97f101d8f39a90902e5c5d39f951fe/build.rs#L88
            //   https://github.com/dtolnay/thiserror/blob/e9ea67c7e251764c3c2d839b6c06d9f35b154647/build.rs#L65
             call_rustc(rustc, argv(1)),
        [rustc, "--crate-name", crate_name, ..] if !called_from_build_script =>
             bake_rustc(crate_name, argv(1), || call_rustc(rustc, argv(1)))
            .inspect_err(|e| {
                error!(target:crate_name, "Failure: {e}");
                eprintln!("Failure: {e}");
            }),
        _ => panic!("RUSTC_WRAPPER={PKG}'s input unexpected:\n\targz = {argz:?}\n\targs = {args:?}\n\tenvs = {vars:?}\n"),
    }
}

#[test]
fn passthrough_getting_rust_target_specific_information() {
    #[rustfmt::skip]
    let first_few_args = &[
        "$PWD/rustcbuildx/rustcbuildx",
        "$HOME/.rustup/toolchains/stable-x86_64-unknown-linux-gnu/bin/rustc",
        "-",
        "--crate-name", "___",
        "--print=file-names",
        "--crate-type", "bin",
        "--crate-type", "rlib",
        "--crate-type", "dylib",
        "--crate-type", "cdylib",
        "--crate-type", "staticlib",
        "--crate-type", "proc-macro",
        "--print=sysroot",
        "--print=split-debuginfo",
        "--print=crate-name",
        "--print=cfg",
    ]
    .into_iter()
    .take(4)
    .map(ToOwned::to_owned)
    .collect::<Vec<String>>();

    let first_few_args =
        first_few_args.iter().skip(1).take(3).map(String::as_str).collect::<Vec<_>>();
    assert_eq!(
        match &first_few_args[..] {
            [_rustc, "-", ..] | [_rustc, _ /*driver*/, "-", ..] => 1,
            [_rustc, "--crate-name", _crate_name, ..] => 2,
            _ => 3,
        },
        1
    );
}

fn call_rustc(rustc: &str, args: Vec<String>) -> Result<ExitCode> {
    // NOTE: not running inside Docker: local install SHOULD match Docker image setup
    // Meaning: it's up to the user to craft their desired $RUSTCBUILDX_BASE_IMAGE
    let code = Command::new(rustc)
        .args(&args)
        .spawn()
        .with_context(|| format!("Failed to spawn rustc {rustc} with {args:?}"))?
        .wait()
        .with_context(|| format!("Failed to wait for rustc {rustc} with {args:?}"))?
        .code();
    Ok(exit_code(code))
}

fn bake_rustc(
    crate_name: &str,
    arguments: Vec<String>,
    fallback: impl Fn() -> Result<ExitCode>,
) -> Result<ExitCode> {
    if env::var_os(RUSTCBUILDX).map(|x| x == "1").unwrap_or_default() {
        bail!("It's turtles all the way down!")
    }
    env::set_var(RUSTCBUILDX, "1");

    let debug = maybe_log();
    if let Some(log_file) = debug {
        env_logger::Builder::from_env(
            Env::default().filter_or(RUSTCBUILDX_LOG, "debug").write_style(RUSTCBUILDX_LOG_STYLE),
        )
        .target(Target::Pipe(Box::new(log_file()?)))
        .init();
    }

    let krate = format!("{PKG}:{crate_name}");
    info!(target:&krate, "{PKG}@{vsn} wraps `rustc` calls to BuildKit builders",
        vsn = env!("CARGO_PKG_VERSION"),
    );

    let pwd = env::current_dir().context("Failed to get $PWD")?;
    let pwd: Utf8PathBuf = pwd.try_into().context("Path's UTF-8 encoding is corrupted")?;

    let (st, args) = parse::as_rustc(&pwd, crate_name, arguments, debug.is_some())?;
    info!(target:&krate, "{st:?}");
    let RustcArgs { crate_type, emit, externs, metadata, incremental, input, out_dir, target_path } =
        st;

    let crate_out = env::var("OUT_DIR").ok().and_then(|x| x.ends_with("/out").then_some(x)); // NOTE: not `out_dir`

    let full_crate_id = format!("{crate_type}-{crate_name}-{metadata}");
    let krate = full_crate_id.as_str();

    env::vars().for_each(|(k, v)| debug!(target:&krate, "env is set: {k}={v:?}")); // TODO: drop

    // https://github.com/rust-lang/cargo/issues/12059
    let mut all_externs = BTreeSet::new();
    let externs_prefix = |part: &str| Utf8Path::new(&target_path).join(format!("externs_{part}"));
    let crate_externs = externs_prefix(&format!("{crate_name}-{metadata}"));

    let ext = match crate_type.as_str() {
        "lib" => "rmeta".to_owned(),
        "bin" | "test" | "proc-macro" => "rlib".to_owned(),
        _ => bail!("BUG: unexpected crate-type: '{crate_type}'"),
    };
    // https://rustc-dev-guide.rust-lang.org/backend/libs-and-metadata.html#rmeta
    // > [rmeta] is created if the --emit=metadata CLI option is used.
    let ext = if emit.contains("metadata") { "rmeta".to_owned() } else { ext };

    if crate_type == "proc-macro" {
        // This way crates that depend on this know they must require it as .so
        let guard = format!("{crate_externs}_proc-macro");
        info!(target:&krate, "opening (RW) {guard}");
        fs::write(&guard, "").with_context(|| format!("Failed to `touch {guard}`"))?;
    };

    let mut short_externs = BTreeSet::new();
    for xtern in &externs {
        all_externs.insert(xtern.clone());

        if !xtern.starts_with("lib") {
            bail!("CONTRACT: cargo gave unexpected extern [^lib]: {xtern:?}")
        }
        let xtern = xtern.strip_prefix("lib").expect("PROOF: ~ ^lib");
        let xtern = if xtern.ends_with(".rlib") {
            xtern.strip_suffix(".rlib")
        } else if xtern.ends_with(".rmeta") {
            xtern.strip_suffix(".rmeta")
        } else if xtern.ends_with(".so") {
            xtern.strip_suffix(".so")
        } else {
            bail!("CONTRACT: cargo gave unexpected extern: {xtern:?}")
        }
        .expect("PROOF: all cases match");
        short_externs.insert(xtern.to_owned());

        let xtern_crate_externs = externs_prefix(xtern);
        info!(target:&krate, "checking (RO) extern's externs {xtern_crate_externs}");
        if file_exists_and_is_not_empty(&xtern_crate_externs)
            .with_context(|| format!("Failed to `test -s {crate_externs}`"))?
        {
            info!(target:&krate, "opening (RO) crate externs {xtern_crate_externs}");
            let fd = File::open(&xtern_crate_externs)
                .with_context(|| format!("Failed to `cat {xtern_crate_externs}`"))?;
            for line in BufReader::new(fd).lines() {
                let transitive =
                    line.with_context(|| format!("Corrupted {xtern_crate_externs}"))?;
                assert_ne!(transitive, "");

                let guard = externs_prefix(&format!("{transitive}_proc-macro"));
                info!(target:&krate, "checking (RO) extern's guard {guard}");
                let actual_extern =
                    if file_exists(&guard).with_context(|| format!("Failed to `stat {guard}`"))? {
                        format!("lib{transitive}.so")
                    } else {
                        format!("lib{transitive}.{ext}")
                    };
                all_externs.insert(actual_extern.clone());

                // ^ this algo tried to "keep track" of actual paths to transitive deps artifacts
                //   however some edge cases (at least 1) go through. That fix seems to bust cache on 2nd builds though v

                if debug.is_some() {
                    let deps_dir = Utf8Path::new(&target_path).join("deps");
                    info!(target:&krate, "listing existing an extern crate's extern matches {deps_dir}/lib*.*");
                    let listing = read_dir(&deps_dir)
                        .with_context(|| format!("Failed reading directory {deps_dir}"))?
                        // TODO: at least context() error
                        .filter_map(std::result::Result::ok)
                        .filter_map(|p| {
                            let p = p.path();
                            p.file_name().map(|p| p.to_string_lossy().to_string())
                        })
                        .filter(|p| p.contains(&transitive))
                        .filter(|p| !p.ends_with(&format!("{transitive}.d")))
                        .map(|p| p.to_string())
                        .collect::<Vec<_>>();
                    if listing != vec![actual_extern.clone()] {
                        warn!("instead of [{actual_extern}], listing found {listing:?}");
                    }
                    //all_externs.extend(listing.into_iter());
                    // TODO: move to after for loop
                }

                short_externs.insert(transitive);
            }
        }
    }
    info!(target:&krate, "checking (RO) externs {crate_externs}");
    if !file_exists_and_is_not_empty(&crate_externs)
        .with_context(|| format!("Failed to `test -s {crate_externs}`"))?
    {
        let mut shorts = String::new();
        for short_extern in &short_externs {
            shorts.push_str(short_extern);
            shorts.push('\n');
        }
        info!(target:&krate, "writing (RW) externs to {crate_externs}");
        fs::write(&crate_externs, shorts)
            .with_context(|| format!("Failed creating crate externs {crate_externs}"))?;
    }
    let all_externs = all_externs;
    info!(target:&krate, "crate_externs: {crate_externs}");
    if debug.is_some() {
        debug!(target:&krate, "{crate_externs} = {data}", data = match read_to_string(&crate_externs) {
            Ok(data) => data,
            Err(e) => e.to_string(),
        });
    }

    create_dir_all(&out_dir).with_context(|| format!("Failed to `mkdir -p {out_dir}`"))?;
    if let Some(ref incremental) = incremental {
        create_dir_all(incremental)
            .with_context(|| format!("Failed to `mkdir -p {incremental}`"))?;
    }

    let hm = |prefix: &str, basename: &str, pop: usize| {
        let stage = format!("{prefix}-{full_crate_id}");
        assert_eq!(pop, prefix.chars().filter(|c| *c == '_').count());
        let not_lowalnums = |c: char| {
            !("._-".contains(c) || c.is_ascii_digit() || (c.is_alphabetic() && c.is_lowercase()))
        };
        let basename = basename.replace(not_lowalnums, "_");
        let name = format!("input_{prefix}--{basename}");
        let target = input.clone().popped(pop);
        (Some((name, target)), stage)
    };

    // TODO: impl non-default build.rs https://doc.rust-lang.org/cargo/reference/manifest.html?highlight=build.rs#the-build-field
    let (input_mount, rustc_stage) = match input.iter().rev().take(4).collect::<Vec<_>>()[..] {
        ["build.rs"] => (None, format!("finalbuildrs-{full_crate_id}")),
        ["lib.rs", "src"] => (None, format!("final-{full_crate_id}")),
        ["main.rs", "src"] => (None, format!("final-{full_crate_id}")),
        ["build.rs", "src", basename, ..] => hm("build__rs", basename, 2), // TODO: un-ducktape
        ["build.rs", basename, ..] => hm("build_rs", basename, 1),
        ["lib.rs", "src", basename, ..] => hm("src_lib_rs", basename, 2),
        // e.g. $HOME/.cargo/registry/src/github.com-1ecc6299db9ec823/fnv-1.0.7/lib.rs
        ["lib.rs", basename, ..] => hm("lib_rs", basename, 1),
        // e.g. $HOME/.cargo/registry/src/github.com-1ecc6299db9ec823/untrusted-0.7.1/src/untrusted.rs
        [rsfile, "src", basename, ..] if rsfile.ends_with(".rs") => hm("src__rs", basename, 2),
        // When compiling openssl-sys-0.9.95 on stable-x86_64-unknown-linux-gnu:
        //   /home/runner/.cargo/registry/src/index.crates.io-6f17d22bba15001f/openssl-sys-0.9.95/build/main.rs
        ["main.rs", "build", basename, ..] if crate_name == "build_script_main" => {
            // TODO: that's ducktape. Read Cargo.toml to match [package]build = "build/main.rs" ?
            // or just catchall >=4
            hm("main__rs", basename, 2)
        }
        // /home/runner/.cargo/registry/src/index.crates.io-6f17d22bba15001f/cargo-deny-0.14.3/src/cargo-deny/main.rs crate_type=bin
        ["main.rs", "cargo-deny", "src", basename] => hm("main___rs", basename, 3), // TODO: un-ducktape
        _ => unreachable!("Unexpected input file {input:?}"),
    };
    // TODO cli=deny: explore mounts: do they include?  -L native=/home/runner/instst/release/build/ring-3c73f9fd9a67ce28/out
    // nner/instst/release/build/zstd-sys-f571b8facf393189/out -L native=/home/runner/instst/release/build/ring-3c73f9fd9a67ce28/out`
    // Found a bug in this script!
    //      Running `CARGO=/home/runner/.rustup/toolchains/stable-x86_64-unknown-linux-gnu/bin/cargo CARGO_BIN_NAME=cargo-deny CARGO_CRATE_NAME=cargo_deny CARGO_MANIFEST_DIR=/home/runner/.cargo/registry/src/index.crates.io-6f17d22bba15001f/cargo-deny-0.14.3 CARGO_PKG_AUTHORS='Embark <opensource@embark-studios.com>:Jake Shadle <jake.shadle@embark-studios.com>' CARGO_PKG_DESCRIPTION='Cargo plugin to help you manage large dependency graphs' CARGO_PKG_HOMEPAGE='https://github.com/EmbarkStudios/cargo-deny' CARGO_PKG_LICENSE='MIT OR Apache-2.0' CARGO_PKG_LICENSE_FILE='' CARGO_PKG_NAME=cargo-deny CARGO_PKG_README=README.md CARGO_PKG_REPOSITORY='https://github.com/EmbarkStudios/cargo-deny' CARGO_PKG_RUST_VERSION=1.70.0 CARGO_PKG_VERSION=0.14.3 CARGO_PKG_VERSION_MAJOR=0 CARGO_PKG_VERSION_MINOR=14 CARGO_PKG_VERSION_PATCH=3 CARGO_PKG_VERSION_PRE='' CARGO_PRIMARY_PACKAGE=1 LD_LIBRARY_PATH='/home/runner/instst/release/deps:/home/runner/.rustup/toolchains/stable-x86_64-unknown-linux-gnu/lib:/home/runner/.rustup/toolchains/stable-x86_64-unknown-linux-gnu/lib'
    // /home/runner/work/rustcbuildx/rustcbuildx/rustcbuildx /home/runner/.rustup/toolchains/stable-x86_64-unknown-linux-gnu/bin/rustc
    // --crate-name cargo_deny --edition=2021
    // /home/runner/.cargo/registry/src/index.crates.io-6f17d22bba15001f/cargo-deny-0.14.3/src/cargo-deny/main.rs --error-format=json --json=diagnostic-rendered-ansi,artifacts,future-incompat
    // --crate-type bin --emit=dep-info,link -C opt-level=3 -C embed-bitcode=no --cfg 'feature="default"' -C metadata=3bcfd2691bae63ba -C extra-filename=-3bcfd2691bae63ba --out-dir /home/runner/instst/release/deps -L dependency=/home/runner/instst/release/deps --extern anyhow=/home/runner/instst/release/deps/libanyhow-94b7395c89edd8b1.rlib --extern askalono=/home/runner/instst/release/deps/libaskalono-ddae43dc4d3d2420.rlib --extern bitvec=/home/runner/instst/release/deps/libbitvec-d623938ca4bca493.rlib --extern camino=/home/runner/instst/release/deps/libcamino-146d319673d173c5.rlib --extern cargo_deny=/home/runner/instst/release/deps/libcargo_deny-06e4f61b912645ef.rlib --extern clap=/home/runner/instst/release/deps/libclap-2c7e3f9347d4597a.rlib --extern codespan=/home/runner/instst/release/deps/libcodespan-a43ae384a6e01906.rlib --extern codespan_reporting=/home/runner/instst/release/deps/libcodespan_reporting-40482f50e693ecb1.rlib --extern crossbeam=/home/runner/instst/release/deps/libcrossbeam-522c8a6792edb38b.rlib --extern fern=/home/runner/instst/release/deps/libfern-82ebb4a2d795cc6e.rlib --extern gix=/home/runner/instst/release/deps/libgix-2269c2d16d598f3b.rlib --extern globset=/home/runner/instst/release/deps/libglobset-1cb53a4633a9e532.rlib --extern goblin=/home/runner/instst/release/deps/libgoblin-a375a2c21dba8a6d.rlib --extern home=/home/runner/instst/release/deps/libhome-a03540b18a902546.rlib --extern krates=/home/runner/instst/release/deps/libkrates-2923f082e623ae47.rlib --extern log=/home/runner/instst/release/deps/liblog-e9c072abf79b5d2b.rlib --extern nu_ansi_term=/home/runner/instst/release/deps/libnu_ansi_term-14990884fe5cdb0f.rlib --extern parking_lot=/home/runner/instst/release/deps/libparking_lot-b8d4a6184ea5481b.rlib --extern rayon=/home/runner/instst/release/deps/librayon-103cc3573de4615f.rlib --extern reqwest=/home/runner/instst/release/deps/libreqwest-c7fbedc59852c9c7.rlib --extern ring=/home/runner/instst/release/deps/libring-691303221da8a7f0.rlib --extern rustsec=/home/runner/instst/release/deps/librustsec-bf5acc14d6976003.rlib --extern semver=/home/runner/instst/release/deps/libsemver-81c2747e00bddbb9.rlib --extern serde=/home/runner/instst/release/deps/libserde-3f028327f7dabe68.rlib --extern serde_json=/home/runner/instst/release/deps/libserde_json-48447a61a356a1fd.rlib --extern smallvec=/home/runner/instst/release/deps/libsmallvec-8853bfbdb0f0c104.rlib --extern spdx=/home/runner/instst/release/deps/libspdx-dcd7ef5eaf2c5c4d.rlib --extern strum=/home/runner/instst/release/deps/libstrum-97202369a5147909.rlib --extern tame_index=/home/runner/instst/release/deps/libtame_index-93d5aa0a6979a91f.rlib --extern time=/home/runner/instst/release/deps/libtime-c0960b055b327693.rlib --extern toml=/home/runner/instst/release/deps/libtoml-04310ccc9f3732b0.rlib --extern twox_hash=/home/runner/instst/release/deps/libtwox_hash-6feb0d177b42c269.rlib --extern url=/home/runner/instst/release/deps/liburl-27ace54f5002c0aa.rlib --extern walkdir=/home/runner/instst/release/deps/libwalkdir-0fa0886e334a678c.rlib --cap-lints warn -L native=/home/runner/instst/release/build/zstd-sys-f571b8facf393189/out -L native=/home/runner/instst/release/build/ring-3c73f9fd9a67ce28/out`
    // thread 'main' panicked at src/main.rs:357:14:
    // internal error: entered unreachable code: Unexpected input file "/home/runner/.cargo/registry/src/index.crates.io-6f17d22bba15001f/cargo-deny-0.14.3/src/cargo-deny/main.rs"
    // note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace
    // error: could not compile `cargo-deny` (bin "cargo-deny")
    info!(target:&krate, "picked {rustc_stage} for {suf:?}", suf=input.iter().rev().take(4).collect::<Vec<_>>());
    assert!(!matches!(input_mount, Some((_,ref x)) if x.ends_with("/.cargo/registry")));

    let incremental_stage = format!("incremental-{metadata}");
    let out_stage = format!("out-{metadata}");
    let stdio_stage = format!("stdio-{metadata}");
    // let mut toolchain = input_mount
    //     .as_ref()
    //     .map(|(_imn, imt)| -> Result<Option<String>> {
    //         let check = |file_name| -> Result<bool> {
    //             let p = Utf8Path::new(imt).join(file_name);
    //             info!(target:&krate, "checking (RO) toolchain file {p}");
    //             file_exists_and_is_not_empty(&p)
    //                 .with_context(|| format!("Failed to `test -s {p:?}`"))
    //         };
    //         for file_name in &["rust-toolchain.toml", "rust-toolchain"] {
    //             if check(file_name)? {
    //                 return Ok(Some(file_name.to_owned().to_owned()));
    //             }
    //         }
    //         Ok(None)
    //     })
    //     .transpose()?
    //     .flatten()
    //     .map(|toolchain_file_name|
    //         // https://rust-lang.github.io/rustup/overrides.html
    //         // NOTE: without this, the crate's rust-toolchain gets installed and used and (for the mentioned crate)
    //         //   fails due to (yet)unknown rustc CLI arg: `error: Unrecognized option: 'diagnostic-width'`
    //         // e.g. https://github.com/xacrimon/dashmap/blob/v5.4.0/rust-toolchain
    //         (format!("toolchain-{metadata}"), toolchain_file_name));
    // if true {
    //     // TODO: test building something involving rust-toolchain.toml
    //     toolchain = None;
    // }

    let mut dockerfile = String::new();

    // const RUSTUP_TOOLCHAIN: &str = "rustup-toolchain";
    // if let Some((stage, _)) = toolchain.as_ref() {
    //     dockerfile.push_str(&format!("FROM rust AS {stage}\n"));
    //     dockerfile
    //         .push_str(&format!("    RUN rustup default | cut -d- -f1 >/{RUSTUP_TOOLCHAIN}\n"));
    // }

    dockerfile.push_str(&format!("FROM rust AS {rustc_stage}\n"));
    dockerfile.push_str(&format!("WORKDIR {out_dir}\n"));

    // TODO: disable remote cache for incremental builds?
    if let Some(incremental) = &incremental {
        dockerfile.push_str(&format!("WORKDIR {incremental}\n"));
    }

    dockerfile.push_str("ENV \\\n");
    for (var, val) in env::vars() {
        if pass_env(var.as_str()) {
            let val = (!val.is_empty())
                .then_some(val)
                .map(|x: String| format!("{x:?}"))
                .unwrap_or_default();
            dockerfile.push_str(&format!("  {var}={val} \\\n"));
        }
    }
    dockerfile.push_str("  RUSTCBUILDX=1\n");

    let cwd = if let Some((name, target)) = input_mount.as_ref() {
        // Reuse previous contexts

        // TODO: WORKDIR was removed as it changed during a single `cargo build`
        // Looks like removing it isn't an issue, however we need more testing.
        // dockerfile.push_str(&format!("WORKDIR {pwd}\n"));
        dockerfile.push_str("RUN \\\n");
        dockerfile.push_str(&format!("  --mount=type=bind,from={name},target={target} \\\n"));

        // TODO: --build-arg BUILDKIT_CONTEXT_KEEP_GIT_DIR=0 https://docs.docker.com/engine/reference/builder/#buildkit-built-in-build-args

        None
    } else {
        // Save/send local workspace

        // TODO: drop
        // note .as_str() is to use &str's ends_with
        assert_eq!((input.is_relative(), input.as_str().ends_with(".rs")), (true, true));

        // TODO: try just bind mount instead of copying to a tmpdir
        // TODO: try not FWDing .git/* and equivalent BUILDKIT_CONTEXT_KEEP_GIT_DIR=0
        // TODO: try filtering out CARGO_TARGET_DIR also
        // https://docs.docker.com/language/rust/develop/
        // RUN --mount=type=bind,source=src,target=src \
        //     --mount=type=bind,source=Cargo.toml,target=Cargo.toml \
        //     --mount=type=bind,source=Cargo.lock,target=Cargo.lock \
        // TODO: try `target = "target:$other"` https://docs.docker.com/build/bake/reference/#targetcontexts

        let cwd = Temp::new_dir().context("Failed to create tmpdir 'cwd'")?;
        let Some(cwd_path) = Utf8Path::from_path(cwd.as_path()) else {
            bail!("Path's UTF-8 encoding is corrupted: {cwd:?}")
        };

        // TODO: use tmpfs when on *NIX
        // TODO: cache these folders
        if pwd.join(".git").is_dir() {
            info!(target:&krate, "copying all git files under {}", pwd.join(".git"));
            let output = Command::new("git")
                .arg("ls-files")
                .arg(&pwd)
                .output()
                .with_context(|| format!("Failed calling `git ls-files {pwd}`"))?;
            if !output.status.success() {
                bail!("Failed `git ls-files {pwd}`: {:?}", output.stderr)
            }
            // TODO: buffer reads to this command's output
            // NOTE: unsorted output lines
            for f in String::from_utf8(output.stdout).context("Parsing `git ls-files`")?.lines() {
                info!(target:&krate, "copying git repo file {f}");
                copy_file(Utf8Path::new(f), cwd_path)?;
            }
        } else {
            info!(target:&krate, "copying all files under {pwd}");
            copy_files(&pwd, cwd_path)?;
        }

        dockerfile.push_str(&format!("WORKDIR {pwd}\n"));
        dockerfile.push_str("COPY --from=cwd / .\n");
        dockerfile.push_str("RUN \\\n");

        Some(cwd)
    };

    if let Some(crate_out) = crate_out.as_ref() {
        let named = crate_out_name(crate_out);
        dockerfile.push_str(&format!("  --mount=type=bind,from={named},target={crate_out} \\\n"));
    }

    // if let Some((stage, _file_name)) = toolchain.as_ref() {
    //     dockerfile.push_str(&format!("  --mount=type=bind,from={stage},source=/{RUSTUP_TOOLCHAIN},target=/{RUSTUP_TOOLCHAIN} \\\n"));
    // }

    debug!(target:&krate, "all_externs = {all_externs:?}");
    assert!(externs.len() <= all_externs.len());
    let bakefiles = all_externs
        .into_iter()
        .map(|xtern| -> Result<_> {
            let Some((extern_bakefile, extern_bakefile_stage)) = bakefile_and_stage(xtern.clone(), &target_path) else {
                bail!("BUG: corrupted bakefile.hcl for {xtern}")
            };

            info!(target:&krate, "extern_bakefile: {extern_bakefile}");

            dockerfile.push_str(&format!("  --mount=type=bind,from={extern_bakefile_stage},source=/{xtern},target={target_path}/deps/{xtern} \\\n"));

            Ok(extern_bakefile)
        })
        .collect::<Result<Vec<_>>>()?;

    dockerfile.push_str("     <<RUN\n"); // Start heredoc
    dockerfile.push_str("  set -eux\n");

    // if toolchain.is_some() {
    //     dockerfile
    //         .push_str(&format!("  export RUSTUP_TOOLCHAIN=\"$(cat /{RUSTUP_TOOLCHAIN})\" && \\\n"));
    // }

    // // https://rust-lang.github.io/rustup/overrides.html
    // // NOTE: without this, the crate's rust-toolchain gets installed and used.
    // // e.g. https://github.com/xacrimon/dashmap/blob/v5.4.0/rust-toolchain
    // // e.g. https://github.com/dtolnay/anyhow/blob/05e413219e97f101d8f39a90902e5c5d39f951fe/rust-toolchain.toml
    // // NOTE this is [[ -s "$input_mount_target"/rust-toolchain ]]
    // // dockerfile.push_str("  if [ -s ./rust-toolchain.toml ] || [ -s ./rust-toolchain ]; then \\\n");
    // // dockerfile.push_str("    export RUSTUP_TOOLCHAIN=\"$(rustup default | cut -d- -f1)\"; \\\n");
    // // dockerfile.push_str("  fi\n");
    // dockerfile.push_str("  export RUSTUP_TOOLCHAIN=stable");

    dockerfile.push_str("  export CARGO=\"$(which cargo)\"\n");

    // TODO: keep only paths that we explicitly mount or copy
    for var in ["PATH", "DYLD_FALLBACK_LIBRARY_PATH", "LD_LIBRARY_PATH", "LIBPATH"] {
        let Ok(val) = env::var(var) else { continue };
        if !val.is_empty() {
            dockerfile.push_str(&format!("  # export {var}=\"{val}:${var}\"\n"));
        }
    }

    // TODO: report BUG
    // buildx bake github issue dockerfile-inline and dockerfile heredoc conflict when using RUN echo "${VAR:-}"
    // error =>
    // Extra characters after interpolation expression; Expected a closing brace to end the interpolation expression, but found extra characters.
    // dockerfile.push_str("  if [ -z \"${CARGO:-}\" ]; then exit 40; fi\n");

    let io = format!("{stdio_stage}.tar"); // FIXME: revert tar "optimization"
    const IOERR: &str = "stderr";
    const IOOUT: &str = "stdout";
    dockerfile.push_str("  set +e\n");
    dockerfile.push_str(&format!("  rustc '{}' {input} >/{IOOUT} 2>/{IOERR}\n", args.join("' '")));
    dockerfile.push_str("  code=$?\n");
    dockerfile.push_str("  set -e\n");
    dockerfile.push_str(&format!("  tar cf /{io} /{IOOUT} /{IOERR}\n"));
    dockerfile.push_str(&format!("  [ $code -eq 0 ] || head /{IOOUT} /{IOERR}\n"));
    dockerfile.push_str("  exit $code\n");

    dockerfile.push_str("RUN\n"); // End of heredoc

    if let Some(incremental) = &incremental {
        dockerfile.push_str(&format!("FROM scratch AS {incremental_stage}\n"));
        dockerfile.push_str(&format!("COPY --from={rustc_stage} {incremental} /\n"));
    }
    dockerfile.push_str(&format!("FROM scratch AS {stdio_stage}\n"));
    dockerfile.push_str(&format!("COPY --from={rustc_stage} /{io} /\n"));
    dockerfile.push_str(&format!("FROM scratch AS {out_stage}\n"));
    dockerfile.push_str(&format!("COPY --from={rustc_stage} {out_dir}/*-{metadata}* /\n"));
    // NOTE: -C extra-filename=-${metadata} (starts with dash)
    // TODO: use extra filename here for fwd compat

    let dockerfile = dockerfile; // Drop mut
    {
        let dockerfile_path = Utf8Path::new(&target_path).join(format!("{metadata}.Dockerfile"));
        info!(target:&krate, "opening (RW) crate dockerfile {dockerfile_path}");
        fs::write(&dockerfile_path, &dockerfile)
            .with_context(|| format!("Failed creating dockerfile {dockerfile_path}"))?;
    }

    let mut contexts: BTreeMap<_, _> = [
        Some(("rust".to_owned(), base_image())),
        input_mount.map(|(name, target)| (name, target.to_string())),
        cwd.as_deref().map(|cwd| {
            let cwd_path = Utf8Path::from_path(cwd.as_path()).expect("PROOF: did not fail earlier");
            ("cwd".to_owned(), cwd_path.to_string())
        }),
        crate_out.map(|crate_out| (crate_out_name(&crate_out), crate_out)),
    ]
    .into_iter()
    .flatten()
    .collect();

    // TODO: ask upstream `docker buildx bake` for a "dockerfiles" []string bake setting (that concatanates) or some way to inherit multiple dockerfiles (don't forget inlined ones)
    // TODO: ask upstream `docker buildx` for orderless stages (so we can concat Dockerfiles any which way, and save another DAG)

    let mut extern_dockerfiles: BTreeMap<_, _> = bakefiles
        .into_iter()
        .map(|extern_bakefile| -> Result<_> {
            info!(target:&krate, "opening (RO) extern bakefile {extern_bakefile}");
            let mounts = used_contexts(&extern_bakefile)?;
            let mounts_len = mounts.len();
            contexts.extend(mounts.into_iter());

            let extern_dockerfile = hcl_to_dockerfile(extern_bakefile);
            Ok((extern_dockerfile, mounts_len))
        })
        .collect::<Result<_>>()?;
    let mut dockerfile_bis = String::new();
    // Concat dockerfiles from topological sort of the DAG (stages must be defined first, then used)
    // Assumes that the more deps a crate has, the later it appears in the deps tree
    // TODO: do     vvvvvvvvv better than this
    for i_mounts in 0..999999usize {
        if extern_dockerfiles.is_empty() {
            break;
        }
        let matching: Vec<_> = extern_dockerfiles
            .iter()
            .filter(|(_, v)| **v == i_mounts)
            .map(|(k, _)| k)
            .cloned()
            .collect();
        for extern_dockerfile_path in matching {
            let res = extern_dockerfiles.remove(&extern_dockerfile_path);
            assert!(res.is_some());
            info!(target:&krate, "opening (RO) extern dockerfile {extern_dockerfile_path}");
            let extern_dockerfile = read_to_string(&extern_dockerfile_path)
                .with_context(|| format!("Failed reading dockerfile {extern_dockerfile_path}"))?;
            dockerfile_bis.push_str(&extern_dockerfile);
            dockerfile_bis.push('\n');
        }
    }
    assert!(extern_dockerfiles.is_empty());
    dockerfile_bis.push_str(&dockerfile);
    drop(dockerfile); // Earlier: wrote to disk

    let stdio = env::temp_dir().join(format!("{PKG}-{stdio_stage}"));
    let Some(stdio_path) = Utf8Path::from_path(stdio.as_path()) else {
        bail!("Path's UTF-8 encoding is corrupted: {stdio:?}")
    };

    const TAB: char = '\t';
    // TODO: use https://lib.rs/crates/hcl-rs#readme-serialization-examples
    let mut bakefile = String::new();

    writeln!(
        bakefile,
        r#"
target "{out_stage}" {{
{TAB}contexts = {{"#
    )?;
    let contexts: BTreeMap<_, _> = contexts.into_iter().collect();
    for (name, uri) in contexts {
        bakefile.push_str(&format!("{TAB}{TAB}\"{name}\" = \"{uri}\",\n"));
    }
    writeln!(
        bakefile,
        r#"{TAB}}}
{TAB}dockerfile-inline = <<DOCKERFILE
# syntax={docker_syntax}
{dockerfile_bis}DOCKERFILE
{TAB}network = "none"
{TAB}output = ["{out_dir}"] # https://github.com/moby/buildkit/issues/1224
{TAB}platforms = ["local"]
{TAB}target = "{out_stage}"
}}
target "{stdio_stage}" {{
{TAB}inherits = ["{out_stage}"]
{TAB}output = ["{stdio_path}"]
{TAB}target = "{stdio_stage}"
}}"#,
        docker_syntax = docker_syntax(),
    )?;

    let mut stages = vec![out_stage.as_str(), stdio_stage.as_str()];
    if let Some(incremental) = incremental.as_ref() {
        stages.push(incremental_stage.as_str());
        writeln!(
            bakefile,
            r#"
target "{incremental_stage}" {{
{TAB}inherits = ["{out_stage}"]
{TAB}output = ["{incremental}"]
{TAB}target = "{incremental_stage}"
}}"#,
        )?;
    }

    let bakefile_path = {
        let bakefile_path = format!("{target_path}/{crate_name}-{metadata}.hcl");
        info!(target:&krate, "opening (RW) crate bakefile {bakefile_path}");
        if debug.is_some() {
            match read_to_string(&bakefile_path) {
                Ok(existing) => pretty_assertions::assert_eq!(existing, bakefile),
                Err(e) if e.kind() == ErrorKind::NotFound => {}
                Err(e) => bail!("{e}"),
            }
        }
        fs::write(&bakefile_path, bakefile)
            .with_context(|| format!("Failed creating bakefile {bakefile_path}"))?; // Don't remove HCL file
        bakefile_path
    };

    let mut cmd = Command::new("docker");
    if let Some(log_file) = debug {
        info!(target:&krate, "bakefile: {bakefile_path}");
        debug!(target:&krate, "{bakefile_path} = {data}", data = match read_to_string(&bakefile_path) {
            Ok(data) => data,
            Err(e) => e.to_string(),
        });
        cmd.arg("--debug").stdin(Stdio::null()).stdout(log_file()?).stderr(log_file()?);
    } else {
        cmd.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    }
    cmd.arg("buildx").arg("bake");
    if false {
        cmd.arg("--no-cache");
    }
    cmd.arg("--file").arg(&bakefile_path);
    cmd.args(stages);
    let start = Instant::now();
    let code = cmd
        .output()
        .with_context(|| format!("Failed calling `docker {args:?}`", args = cmd.get_args()))?
        .status
        .code();
    info!("command `docker buildx bake` ran in {}s: {code:?}", start.elapsed().as_secs());

    // TODO: buffered reading + copy to STDERR/STDOUT => give open fds in bakefile?
    {
        use tar::Archive;
        let path = stdio_path.join(&io);
        info!(target:&krate, "reading (RO) {path}");
        let file = File::open(&path).with_context(|| format!("Failed to read {path}"))?;
        let mut archive = Archive::new(file);
        for entry in
            archive.entries().with_context(|| format!("Failed getting tar entries {path}"))?
        {
            let Ok(mut entry) = entry else { continue };
            let mut data = String::new();
            let Ok(_) = entry.read_to_string(&mut data) else { continue };
            let msg = data.trim();
            debug!(target:&krate, "{path} ~= {msg:?}");
            if !msg.is_empty() {
                let epath = entry.path_bytes();
                if epath == IOOUT.as_bytes() {
                    println!("{msg}");
                } else if epath == IOOUT.as_bytes() {
                    eprintln!("{msg}");

                    let mut z = msg.split('"');
                    let mut a = z.next();
                    let mut b = z.next();
                    let mut c = z.next();
                    loop {
                        match (a, b, c) {
                            (Some("artifact"), Some(":"), Some(file)) => {
                                info!(target:&krate, "rustc wrote {file}")
                            }
                            (_, _, Some(_)) => {}
                            (_, _, None) => break,
                        }
                        (a, b, c) = (b, c, z.next());
                    }
                }
            }
        }
        fs::remove_file(&path).with_context(|| format!("Failed deleting {path}"))?;
    }

    if debug.is_none() {
        drop(stdio); // Removes stdio/std{err,out} files and stdio dir
        if let Some(cwd) = cwd {
            drop(cwd); // Removes tempdir contents
        }
        if code != Some(0) {
            warn!(target:&krate, "Falling back...");
            let res = fallback(); // Bubble up actual error & outputs
            if res.is_ok() {
                error!(target:&krate, "BUG found!");
                eprintln!("Found a bug in this script!");
            }
            return res;
        }
    }

    Ok(exit_code(code))
}

#[inline]
fn file_exists(path: impl AsRef<Utf8Path>) -> Result<bool> {
    match path.as_ref().metadata().map(|md| md.is_file()) {
        Ok(b) => Ok(b),
        Err(e) => {
            if e.kind() == ErrorKind::NotFound {
                return Ok(false);
            }
            Err(e.into())
        }
    }
}

#[inline]
fn file_exists_and_is_not_empty(path: impl AsRef<Utf8Path>) -> Result<bool> {
    match path.as_ref().metadata().map(|md| md.is_file() && md.len() > 0) {
        Ok(b) => Ok(b),
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(false),
        Err(e) => Err(e.into()),
    }
}

#[test]
fn fetches_back_used_contexts() {
    let tmp = Temp::new_file().unwrap();
    fs::write(&tmp, r#"
...
contexts = {
    "rust" = "docker-image://docker.io/library/rust:1.69.0-slim@sha256:8b85a8a6bf7ed968e24bab2eae6f390d2c9c8dbed791d3547fef584000f48f9e",
    "input_src_lib_rs--rustversion-1.0.9" = "/home/maison/.cargo/registry/src/github.com-1ecc6299db9ec823/rustversion-1.0.9",
    "crate_out-..." = "/home/maison/code/thing.git/target/debug/build/rustversion-ae69baa7face5565/out",
}
...
"#).unwrap();

    assert_eq!(
        used_contexts(Utf8Path::from_path(tmp.as_path()).unwrap()).unwrap(),
        [
            (
                "input_src_lib_rs--rustversion-1.0.9",
                "/home/maison/.cargo/registry/src/github.com-1ecc6299db9ec823/rustversion-1.0.9",
            ),
            (
                "crate_out-...",
                "/home/maison/code/thing.git/target/debug/build/rustversion-ae69baa7face5565/out",
            ),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_owned(), v.to_owned()))
        .collect()
    );
}

fn used_contexts(path: impl AsRef<Utf8Path>) -> Result<BTreeMap<String, String>> {
    let path: &Utf8Path = path.as_ref();
    let fd = File::open(path).with_context(|| format!("Failed reading {path}"))?;
    BufReader::new(fd)
        .lines()
        .map_while(Result::ok)
        .filter(|line| {
            let ln = line.trim_start();
            ln.starts_with("\"input_") || ln.starts_with("\"crate_out-")
        })
        .map(|line| {
            if let [_, name, _, target, _] = line.splitn(5, '"').collect::<Vec<_>>()[..] {
                Ok((name.to_owned(), target.to_owned()))
            } else {
                bail!("corrupted extern_bakefile {path}: {line:?}")
            }
        })
        .collect::<Result<_>>()
}

#[test]
fn bakefile_and_stage_for_rlib() {
    let xtern = "libstrsim-8ed1051e7e58e636.rlib".to_owned();
    let res = bakefile_and_stage(xtern, "./target/path");
    assert_eq!(
        res,
        Some((
            "./target/path/strsim-8ed1051e7e58e636.hcl".to_owned().into(),
            "out-8ed1051e7e58e636".to_owned()
        ))
    );
}

fn bakefile_and_stage(
    xtern: String,
    target_path: impl AsRef<Utf8Path>,
) -> Option<(Utf8PathBuf, String)> {
    assert!(xtern.starts_with("lib")); // TODO: stop doing that (stripping ^lib)
    let bk = xtern.strip_prefix("lib").and_then(|x| x.split_once('.')).map(|(x, _)| x);
    let sg = bk.and_then(|x| x.split_once('-')).map(|(_, x)| x).map(|x| format!("out-{x}"));
    let bk = bk.map(|x| target_path.as_ref().join(format!("{x}.hcl")));
    bk.zip(sg)
}

#[test]
fn crate_out_name_for_some_pkg() {
    let crate_out =
        "/home/pete/wefwefwef/buildxargs.git/target/debug/build/quote-adce79444856d618/out";
    let res = crate_out_name(crate_out);
    assert_eq!(res, "crate_out-adce79444856d618".to_owned());
}

fn crate_out_name(name: &str) -> String {
    Utf8Path::new(name)
        .parent()
        .and_then(|x| x.file_name())
        .and_then(|x| x.rsplit_once('-'))
        .map(|(_, x)| x)
        .map(|x| format!("crate_out-{x}"))
        .expect("PROOF: suffix is /out")
}

#[test]
fn a_few_hcl_to_dockerfile() {
    assert_eq!(
        hcl_to_dockerfile("target/path/strsim-8ed1051e7e58e636.hcl".into()),
        "target/path/8ed1051e7e58e636.Dockerfile".to_owned()
    );
    assert_eq!(
        hcl_to_dockerfile("target/path/blip_blap-blop-1312051e7e58e636.hcl".into()),
        "target/path/1312051e7e58e636.Dockerfile".to_owned()
    );
}

fn hcl_to_dockerfile(mut hcl: Utf8PathBuf) -> Utf8PathBuf {
    let file_name = hcl
        .file_stem()
        .and_then(|x| x.rsplit_once('-').map(|(_, x)| x.to_owned()))
        .expect("PROOF: FIXME");
    let ok = hcl.pop();
    assert!(ok);
    hcl.join(format!("{file_name}.Dockerfile"))
}

fn copy_file(f: &Utf8Path, cwd: &Utf8Path) -> Result<()> {
    let Some(f_dirname) = f.parent() else { bail!("BUG: unexpected f={f:?} cwd={cwd:?}") };
    let dst = cwd.join(f_dirname);
    create_dir_all(&dst).with_context(|| format!("Failed `mkdir -p {dst}`"))?;
    let dst = cwd.join(f);
    fs::copy(f, &dst).with_context(|| format!("Failed `cp {f} {dst}`"))?;
    Ok(())
}

fn copy_files(dir: &Utf8Path, dst: &Utf8Path) -> Result<()> {
    if dir.is_dir() {
        // TODO: deterministic iteration
        for entry in read_dir(dir).with_context(|| format!("Failed reading dir {dir}"))? {
            let entry = entry?;
            let entry = entry.path();
            let entry = entry.as_path(); // thanks, Rust
            let Some(path) = Utf8Path::from_path(entry) else {
                bail!("Path's UTF-8 encoding is corrupted: {entry:?}")
            };
            if path.is_dir() {
                copy_files(path, dst)?;
            } else {
                copy_file(path, dst)?;
            }
        }
    }
    Ok(())
}
