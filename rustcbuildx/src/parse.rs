use std::collections::BTreeSet;

use anyhow::{bail, Result};
use camino::{Utf8Path, Utf8PathBuf};
use supergreen::extensions::Popped;

// FIXME: fix bad mapping (eg. multiple crate types) + generalize
// https://github.com/declantsien/cargo-ninja/blob/42490a0c8a67bbf8c0aff56a0cb70731913fd3e3/src/rustc_config.rs

/// RustcArgs contains parts of `rustc`'s arguments
#[derive(Debug, Default, PartialEq)]
pub(crate) struct RustcArgs {
    /// 1..: --crate-type
    pub(crate) crate_type: String,

    /// 1: --emit=EMIT | --emit EMIT
    pub(crate) emit: String,

    /// 0..: --extern=EXTERN | --extern EXTERN
    pub(crate) externs: BTreeSet<String>,

    /// 1: -C metadata=METADATA
    pub(crate) metadata: String,

    /// 0|1: -C incremental=INCREMENTAL
    pub(crate) incremental: Option<Utf8PathBuf>,

    /// 1: plain path to (non-empty existing) file
    pub(crate) input: Utf8PathBuf,

    /// 1: --out-dir OUT_DIR
    pub(crate) out_dir: Utf8PathBuf,

    /// Target path:
    pub(crate) target_path: Utf8PathBuf,
}

pub(crate) fn as_rustc(
    pwd: impl AsRef<Utf8Path>,
    arguments: Vec<String>,
) -> Result<(RustcArgs, Vec<String>)> {
    let mut args = vec![];

    let mut state: RustcArgs = Default::default();

    // TODO: find something that sets value only once

    let mut s_e = true;
    let mut key = arguments.first().expect("PROOF: defo not empty").clone();

    #[allow(unused_assignments)] // TODO: but why
    let mut val = String::new();
    for arg in arguments.iter().skip(1) {
        (s_e, key, val) = if s_e {
            (!s_e, key, arg.clone()) // end
        } else {
            (!s_e, arg.clone(), "".to_owned()) // start
        };
        if s_e && val.is_empty() && arg.starts_with("--") && arg.contains('=') {
            let (lhs, rhs) = arg.split_once('=').expect("arg contains '='");
            (s_e, key, val) = (!s_e, lhs.to_owned(), rhs.to_owned());
        }

        if val.is_empty() && (key.starts_with('/') || key.ends_with(".rs")) {
            assert_eq!(state.input, "");
            // For e.g. $HOME/.cargo/registry/src/github.com-1ecc6299db9ec823/ahash-0.7.6/./build.rs
            state.input = key.as_str().replace("/./", "/").into();
            (s_e, key) = (false, "".to_owned());
            continue;
        }

        if s_e && key == "--test" && val.is_empty() {
            assert_eq!(state.crate_type, "");
            "test".clone_into(&mut state.crate_type); // Not a real `--crate-type`
            (s_e, key) = (false, "".to_owned());
            args.push("--test".to_owned());
            continue;
        }

        if val.is_empty() {
            continue;
        }

        // FIXME: drop (strips out local config for now)
        match (key.as_str(), val.as_str()) {
            ("-C", "link-arg=-fuse-ld=/usr/local/bin/mold") => {
                (s_e, key) = (false, "".to_owned());
                continue;
            }
            ("-C", "linker=/usr/bin/clang") => {
                (s_e, key) = (false, "".to_owned());
                continue;
            }
            _ => {}
        }

        match key.as_str() {
            "-C" => match val.split_once('=') {
                Some(("metadata", v)) => {
                    assert_eq!(state.metadata, "");
                    v.clone_into(&mut state.metadata);
                }
                Some(("incremental", v)) => {
                    assert_eq!(state.incremental, None);
                    state.incremental = Some(Utf8PathBuf::from(v));
                }
                _ => {}
            },
            "-L" => {
                if let Some(("dependency", v)) = val.split_once('=') {
                    if !v.starts_with('/') {
                        val = format!("dependency={}", pwd.as_ref().join(v));
                    }
                }
            }
            "--crate-type" => {
                // https://doc.rust-lang.org/cargo/reference/cargo-targets.html#the-crate-type-field
                // array, >=1 per rustc call => as many products !!! we expect a single one throughout FIXME
                // assert_eq!(state.crate_type, "");
                const ALL_CRATE_TYPES: [&str; 7] =
                    ["bin", "lib", "rlib", "dylib", "cdylib", "staticlib", "proc-macro"];
                if !ALL_CRATE_TYPES.contains(&val.as_str()) {
                    let ct = state.crate_type;
                    bail!("Unhandled --crate-type={val} (knowing {ct:?}) in {arguments:?}")
                }
                val.clone_into(&mut state.crate_type);
            }
            "--emit" => {
                assert_eq!(state.emit, "");
                // For instance: dep-info,link dep-info,metadata dep-info,metadata,link
                val.clone_into(&mut state.emit);
            }
            "--extern" => {
                if ["alloc", "core", "proc_macro", "std", "test"].contains(&val.as_str()) {
                    args.push(key.clone());
                    args.push(val);
                    continue; // Sysroot crates (e.g. https://doc.rust-lang.org/proc_macro)
                }

                let xtern = match val.split_once('=') {
                    None => &val,
                    Some((_, val)) => val,
                };

                // NOTE:
                // https://github.com/rust-lang/cargo/issues/9661
                // https://github.com/dtolnay/cxx/blob/83d9d43892d9fe67dd031e4115ae38d0ef3c4712/gen/build/src/target.rs#L10
                // https://github.com/rust-lang/cargo/issues/6100
                // This doesn't always verify: case "$extern" in "$deps_path"/*) ;; *) return 4 ;; esac
                // because $CARGO_TARGET_DIR is sometimes set to $PWD/target and sometimes $HOME/.cargo/registry/src/github.com-1ecc6299db9ec823/anstyle-parse-0.1.1
                // So we can't do: externs+=("${extern#"$deps_path"/}")
                // Anyway the goal is simply to just extract libutf8parse-03cddaef72c90e73.rmeta from $HOME/wefwefwef/buildxargs.git/target/debug/deps/libutf8parse-03cddaef72c90e73.rmeta
                // So let's just do that!
                if let Some(xtern) = Utf8Path::new(xtern).file_name() {
                    state.externs.insert(xtern.to_owned());
                } else {
                    bail!("BUG: {xtern} has no file name")
                }
            }
            "--out-dir" => {
                assert_eq!(state.out_dir, "");
                state.out_dir = val.clone().into();
                if state.out_dir.is_relative() {
                    state.out_dir = pwd.as_ref().join(&val);
                }
                val = state.out_dir.to_string();
            }
            _ => {}
        }

        args.push(key.clone());
        args.push(val);
    }

    assert_ne!(state.crate_type, "");
    assert_ne!(state.metadata, "");
    assert!(!state.incremental.as_ref().map(|x| x == "").unwrap_or_default()); // MAY be unset: only set on last calls
    assert_ne!(state.input, "");
    assert_ne!(state.out_dir, "");

    // Can't rely on $PWD nor $CARGO_TARGET_DIR because `cargo` changes them.
    // Out dir though...
    // --out-dir "$CARGO_TARGET_DIR/$PROFILE"/build/rustix-2a01a00f5bdd1924
    // --out-dir "$CARGO_TARGET_DIR/$PROFILE"/deps
    state.target_path = match &state.out_dir.iter().rev().take(3).collect::<Vec<_>>()[..] {
        ["deps", ..] => state.out_dir.clone().popped(1),
        ["examples", _profile, ..] => state.out_dir.clone().popped(2),
        [_crate_dir, "build", ..] => state.out_dir.clone().popped(2),
        nope => bail!("BUG: --out-dir path should match /deps$|.+/build/.+: {nope:?}"),
    };
    // TODO: return path makers through closures
    // TODO: namespace our files: {target_path}/{NS}/{profile}/...

    Ok((state, args))
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use crate::parse::{as_rustc, RustcArgs};

    const HOME: &str = "/home/maison";
    const PWD: &str = "$HOME/⺟/rustcbuildx.git";

    fn as_argument(arg: &str) -> String {
        arg.to_owned().replace("$PWD", PWD).replace("$HOME", HOME)
    }

    fn as_arguments(args: &[&str]) -> Vec<String> {
        args.iter().map(|x| as_argument(x)).collect()
    }

    #[test]
    fn args_when_building_final_binary() {
        #[rustfmt::skip]
        // Original ordering per rustc 1.69.0 (84c898d65 2023-04-16)
        let arguments= as_arguments(&[
            "$PWD/./dbg/debug/rustcbuildx",                                                   // this
            "$HOME/.rustup/toolchains/stable-x86_64-unknown-linux-gnu/bin/rustc",             // rustc
            "--crate-name", "rustcbuildx",                                                    // crate_name
            "--edition=2021",
            "src/main.rs",                                                                    // state.input
            "--error-format=json",
            "--json=diagnostic-rendered-ansi,artifacts,future-incompat",
            "--diagnostic-width=211",
            "--crate-type", "bin",                                                            // state.crate_type~
            "--emit=dep-info,link",                                                           // state.emit
            "-C", "embed-bitcode=no",
            "-C", "debuginfo=2",
            "-C", "metadata=710b4516f388a5e4",                                                // state.metadata
            "-C", "extra-filename=-710b4516f388a5e4",
            "--out-dir", "$PWD/target/debug/deps",                                            // state.out_dir =+> state.target_path
            "-C", "linker=/usr/bin/clang",
            "-C", "incremental=$PWD/target/debug/incremental",                                   // state.incremental
            "-L", "dependency=$PWD/target/debug/deps",
            "--extern", "anyhow=$PWD/target/debug/deps/libanyhow-f96497119bad6f50.rlib",         // state.externs
            "--extern", "env_logger=$PWD/target/debug/deps/libenv_logger-7e2d283f6e473671.rlib", // state.externs
            "--extern", "log=$PWD/target/debug/deps/liblog-27d1dc50ab631e5f.rlib",               // state.externs
            "--extern", "mktemp=$PWD/target/debug/deps/libmktemp-b84fe47f0a44f88d.rlib",         // state.externs
            "--extern", "os_pipe=$PWD/target/debug/deps/libos_pipe-f344e452b9bd1c5e.rlib",       // state.externs
            "-C", "link-arg=-fuse-ld=/usr/local/bin/mold",
        ]);

        let (st, args) = as_rustc(PWD, arguments.clone()).unwrap();

        assert_eq!(
            st,
            RustcArgs {
                crate_type: "bin".to_owned(),
                emit: "dep-info,link".to_owned(),
                externs: [
                    "libanyhow-f96497119bad6f50.rlib",
                    "libenv_logger-7e2d283f6e473671.rlib",
                    "liblog-27d1dc50ab631e5f.rlib",
                    "libmktemp-b84fe47f0a44f88d.rlib",
                    "libos_pipe-f344e452b9bd1c5e.rlib",
                ]
                .into_iter()
                .map(ToOwned::to_owned)
                .collect(),
                metadata: "710b4516f388a5e4".to_owned(),
                incremental: Some(as_argument("$PWD/target/debug/incremental").into()),
                input: as_argument("src/main.rs").into(),
                out_dir: as_argument("$PWD/target/debug/deps").into(),
                target_path: as_argument("$PWD/target/debug").into(),
            }
        );

        #[rustfmt::skip]
        assert_eq!(args,  as_arguments(&[
                "$PWD/./dbg/debug/rustcbuildx",
                "$HOME/.rustup/toolchains/stable-x86_64-unknown-linux-gnu/bin/rustc",
                "--crate-name", "rustcbuildx",
                "--edition", "2021",
                "--error-format", "json",
                "--json", "diagnostic-rendered-ansi,artifacts,future-incompat",
                "--diagnostic-width", "211",
                "--crate-type", "bin",
                "--emit", "dep-info,link",
                "-C", "embed-bitcode=no",
                "-C", "debuginfo=2",
                "-C", "metadata=710b4516f388a5e4",
                "-C", "extra-filename=-710b4516f388a5e4",
                "--out-dir", "$PWD/target/debug/deps",
                "-C", "incremental=$PWD/target/debug/incremental",
                "-L", "dependency=$PWD/target/debug/deps",
                "--extern", "anyhow=$PWD/target/debug/deps/libanyhow-f96497119bad6f50.rlib",
                "--extern", "env_logger=$PWD/target/debug/deps/libenv_logger-7e2d283f6e473671.rlib",
                "--extern", "log=$PWD/target/debug/deps/liblog-27d1dc50ab631e5f.rlib",
                "--extern", "mktemp=$PWD/target/debug/deps/libmktemp-b84fe47f0a44f88d.rlib",
                "--extern", "os_pipe=$PWD/target/debug/deps/libos_pipe-f344e452b9bd1c5e.rlib",
             ]));
    }

    #[test]
    fn args_when_building_final_test() {
        #[rustfmt::skip]
        // Original ordering per rustc 1.69.0 (84c898d65 2023-04-16)
        let arguments = as_arguments(&[
            "$PWD/./dbg/debug/rustcbuildx",                                                   // this
            "$HOME/.rustup/toolchains/stable-x86_64-unknown-linux-gnu/bin/rustc",             // rustc
            "--crate-name", "rustcbuildx",                                                    // crate_name
            "--edition=2021",
            "src/main.rs",                                                                    // state.input
            "--error-format=json",
            "--json=diagnostic-rendered-ansi,artifacts,future-incompat",
            "--diagnostic-width=347",
            "--emit=dep-info,link",
            "-C", "embed-bitcode=no",
            "-C", "debuginfo=2",
            "--test",
            "-C", "metadata=7c7a0950383d41d3",                                                // state.metadata
            "-C", "extra-filename=-7c7a0950383d41d3",
            "--out-dir", "$PWD/target/debug/deps",                                            // state.out_dir =+> state.target_path
            "-C", "linker=/usr/bin/clang",
            "-C", "incremental=$PWD/target/debug/incremental",                                // state.incremental
            "-L", "dependency=$PWD/target/debug/deps",
            "--extern", "anyhow=$PWD/target/debug/deps/libanyhow-f96497119bad6f50.rlib",
            "--extern", "env_logger=$PWD/target/debug/deps/libenv_logger-7e2d283f6e473671.rlib",
            "--extern", "log=$PWD/target/debug/deps/liblog-27d1dc50ab631e5f.rlib",
            "--extern", "mktemp=$PWD/target/debug/deps/libmktemp-b84fe47f0a44f88d.rlib",
            "--extern", "os_pipe=$PWD/target/debug/deps/libos_pipe-f344e452b9bd1c5e.rlib",
            "--extern", "pretty_assertions=$PWD/target/debug/deps/libpretty_assertions-9fa55d8a39fa5fe3.rlib",
            "-C", "link-arg=-fuse-ld=/usr/local/bin/mold",
        ]);

        let (st, args) = as_rustc(PWD, arguments.clone()).unwrap();

        assert_eq!(
            st,
            RustcArgs {
                crate_type: "test".to_owned(),
                emit: "dep-info,link".to_owned(),
                externs: [
                    "libanyhow-f96497119bad6f50.rlib",
                    "libenv_logger-7e2d283f6e473671.rlib",
                    "liblog-27d1dc50ab631e5f.rlib",
                    "libmktemp-b84fe47f0a44f88d.rlib",
                    "libos_pipe-f344e452b9bd1c5e.rlib",
                    "libpretty_assertions-9fa55d8a39fa5fe3.rlib",
                ]
                .into_iter()
                .map(ToOwned::to_owned)
                .collect(),
                metadata: "7c7a0950383d41d3".to_owned(),
                incremental: Some(as_argument("$PWD/target/debug/incremental").into()),
                input: as_argument("src/main.rs").into(),
                out_dir: as_argument("$PWD/target/debug/deps").into(),
                target_path: as_argument("$PWD/target/debug").into(),
            }
        );

        #[rustfmt::skip]
        assert_eq!(as_arguments(&[
                "$PWD/./dbg/debug/rustcbuildx",
                "$HOME/.rustup/toolchains/stable-x86_64-unknown-linux-gnu/bin/rustc",
                "--crate-name", "rustcbuildx",
                "--edition", "2021",
                "--error-format", "json",
                "--json", "diagnostic-rendered-ansi,artifacts,future-incompat",
                "--diagnostic-width", "347",
                "--emit", "dep-info,link",
                "-C", "embed-bitcode=no",
                "-C", "debuginfo=2",
                "--test",
                "-C", "metadata=7c7a0950383d41d3",
                "-C", "extra-filename=-7c7a0950383d41d3",
                "--out-dir", "$PWD/target/debug/deps",
                "-C", "incremental=$PWD/target/debug/incremental",
                "-L", "dependency=$PWD/target/debug/deps",
                "--extern", "anyhow=$PWD/target/debug/deps/libanyhow-f96497119bad6f50.rlib",
                "--extern", "env_logger=$PWD/target/debug/deps/libenv_logger-7e2d283f6e473671.rlib",
                "--extern", "log=$PWD/target/debug/deps/liblog-27d1dc50ab631e5f.rlib",
                "--extern", "mktemp=$PWD/target/debug/deps/libmktemp-b84fe47f0a44f88d.rlib",
                "--extern", "os_pipe=$PWD/target/debug/deps/libos_pipe-f344e452b9bd1c5e.rlib",
                "--extern", "pretty_assertions=$PWD/target/debug/deps/libpretty_assertions-9fa55d8a39fa5fe3.rlib",
             ]), args);
    }

    #[test]
    fn args_when_building_build_script() {
        #[rustfmt::skip]
        // Original ordering per rustc 1.69.0 (84c898d65 2023-04-16)
        let arguments = as_arguments(&[
            "$PWD/./dbg/debug/rustcbuildx",                                                   // this
            "$HOME/.rustup/toolchains/stable-x86_64-unknown-linux-gnu/bin/rustc",             // rustc
            "--crate-name", "build_script_build",                                             // crate_name
            "--edition=2021",
            "$HOME/.cargo/registry/src/index.crates.io-6f17d22bba15001f/rustix-0.38.20/build.rs", // state.input
            "--error-format=json",
            "--json=diagnostic-rendered-ansi,artifacts,future-incompat",
            "--diagnostic-width=211",
            "--crate-type", "bin",                                                            // state.crate_type~
            "--emit=dep-info,link",
            "-C", "embed-bitcode=no",
            "--cfg", "'feature=\"alloc\"'",
            "--cfg", "'feature=\"default\"'",
            "--cfg", "'feature=\"std\"'",
            "--cfg", "'feature=\"termios\"'",
            "--cfg", "'feature=\"use-libc-auxv\"'",
            "-C", "metadata=c7101a3d6c8e4dce",
            "-C", "extra-filename=-c7101a3d6c8e4dce",
            "--out-dir", "$PWD/target/debug/build/rustix-c7101a3d6c8e4dce",                   // state.out_dir =+> state.target_path
            "-C", "linker=/usr/bin/clang",
            "-L", "dependency=$PWD/target/debug/deps",
            "--cap-lints", "warn",
            "-C", "link-arg=-fuse-ld=/usr/local/bin/mold",
        ]);

        let (st, args) = as_rustc(PWD, arguments.clone()).unwrap();

        assert_eq!(
            st,
            RustcArgs {
                crate_type: "bin".to_owned(),
                emit: "dep-info,link".to_owned(),
                externs: Default::default(),
                metadata: "c7101a3d6c8e4dce".to_owned(),
                incremental: None,
                input: as_argument("$HOME/.cargo/registry/src/index.crates.io-6f17d22bba15001f/rustix-0.38.20/build.rs").into(),
                out_dir: as_argument("$PWD/target/debug/build/rustix-c7101a3d6c8e4dce").into(),
                target_path: as_argument("$PWD/target/debug").into(),
            }
        );

        #[rustfmt::skip]
        assert_eq!(args,  as_arguments(&[
                "$PWD/./dbg/debug/rustcbuildx",
                "$HOME/.rustup/toolchains/stable-x86_64-unknown-linux-gnu/bin/rustc",
                "--crate-name", "build_script_build",
                "--edition", "2021",
                "--error-format", "json",
                "--json", "diagnostic-rendered-ansi,artifacts,future-incompat",
                "--diagnostic-width", "211",
                "--crate-type", "bin",
                "--emit", "dep-info,link",
                "-C", "embed-bitcode=no",
                "--cfg", "'feature=\"alloc\"'",
                "--cfg", "'feature=\"default\"'",
                "--cfg", "'feature=\"std\"'",
                "--cfg", "'feature=\"termios\"'",
                "--cfg", "'feature=\"use-libc-auxv\"'",
                "-C", "metadata=c7101a3d6c8e4dce",
                "-C", "extra-filename=-c7101a3d6c8e4dce",
                "--out-dir", "$PWD/target/debug/build/rustix-c7101a3d6c8e4dce",
                "-L", "dependency=$PWD/target/debug/deps",
                "--cap-lints", "warn",
             ]));
    }

    #[test]
    fn args_when_building_proc_macro() {
        #[rustfmt::skip]
        // Original ordering per rustc 1.73.0
        let arguments = as_arguments(&[
            "rustcbuildx",
            "$HOME/.rustup/toolchains/stable-x86_64-unknown-linux-gnu/bin/rustc",
            "--crate-name", "time_macros",
            "--edition=2021",
            "$HOME/.cargo/registry/src/index.crates.io-6f17d22bba15001f/time-macros-0.2.14/src/lib.rs",
            "--error-format=json",
            "--json=diagnostic-rendered-ansi,artifacts,future-incompat",
            "--diagnostic-width=211",
            "--crate-type", "proc-macro",
            "--emit=dep-info,link",
            "-C", "prefer-dynamic",
            "-C", "embed-bitcode=no",
            "-C", "debug-assertions=off",
            "--cfg", "'feature=\"formatting\"'",
            "--cfg", "'feature=\"parsing\"'",
            "-C", "metadata=89438a15ab938e2f",
            "-C", "extra-filename=-89438a15ab938e2f",
            "--out-dir", "/tmp/wfrefwef__cargo-deny_0-14-3/release/deps",
            "-C", "linker=/usr/bin/clang",
            "-L", "dependency=/tmp/wfrefwef__cargo-deny_0-14-3/release/deps",
            "--extern", "time_core=/tmp/wfrefwef__cargo-deny_0-14-3/release/deps/libtime_core-c880e75c55528c08.rlib",
            "--extern", "proc_macro", // oh hi
            "--cap-lints", "warn",
            "-C", "link-arg=-fuse-ld=/usr/local/bin/mold",
        ]);

        let (st, args) = as_rustc(PWD, arguments.clone()).unwrap();

        assert_eq!(
            st,
            RustcArgs {
                crate_type: "proc-macro".to_owned(),
                emit: "dep-info,link".to_owned(),
                externs: ["libtime_core-c880e75c55528c08.rlib".to_owned()].into(),
                metadata: "89438a15ab938e2f".to_owned(),
                incremental: None,
                input: as_argument("$HOME/.cargo/registry/src/index.crates.io-6f17d22bba15001f/time-macros-0.2.14/src/lib.rs").into(),
                out_dir: as_argument("/tmp/wfrefwef__cargo-deny_0-14-3/release/deps").into(),
                target_path: as_argument("/tmp/wfrefwef__cargo-deny_0-14-3/release").into(),
            }
        );

        #[rustfmt::skip]
        assert_eq!(as_arguments(&[
                "rustcbuildx",
                "$HOME/.rustup/toolchains/stable-x86_64-unknown-linux-gnu/bin/rustc",
                "--crate-name", "time_macros",
                "--edition", "2021",
                "--error-format", "json",
                "--json", "diagnostic-rendered-ansi,artifacts,future-incompat",
                "--diagnostic-width", "211",
                "--crate-type", "proc-macro",
                "--emit", "dep-info,link",
                "-C", "prefer-dynamic",
                "-C", "embed-bitcode=no",
                "-C", "debug-assertions=off",
                "--cfg", "'feature=\"formatting\"'",
                "--cfg", "'feature=\"parsing\"'",
                "-C", "metadata=89438a15ab938e2f",
                "-C", "extra-filename=-89438a15ab938e2f",
                "--out-dir", "/tmp/wfrefwef__cargo-deny_0-14-3/release/deps",
                "-L", "dependency=/tmp/wfrefwef__cargo-deny_0-14-3/release/deps",
                "--extern", "time_core=/tmp/wfrefwef__cargo-deny_0-14-3/release/deps/libtime_core-c880e75c55528c08.rlib",
                "--extern", "proc_macro", // oh hi
                "--cap-lints", "warn",
             ]), args);
    }

    #[test]
    fn args_when_building_that_buildrs() {
        #[rustfmt::skip]
        // Original ordering per rustc 1.73.0
        let arguments = as_arguments(&[
            "rustcbuildx",
            "$HOME/.rustup/toolchains/stable-x86_64-unknown-linux-gnu/bin/rustc",
            "--crate-name", "build_script_build",
            "--edition=2021",
            "src/build.rs",
            "--error-format=json",
            "--json=diagnostic-rendered-ansi,artifacts,future-incompat",
            "--diagnostic-width=211",
            "--crate-type", "bin",
            "--emit=dep-info,link",
            "-C", "embed-bitcode=no",
            "-C", "debug-assertions=off",
            "--cfg", "feature=\"default\"",
            "-C", "metadata=96fe5c8493f1a08f",
            "-C", "extra-filename=-96fe5c8493f1a08f",
            "--out-dir", "/tmp/wfrefwef__cross@0.2.5/release/build/cross-96fe5c8493f1a08f",
            "-C", "linker=/usr/bin/clang",
            "-L", "dependency=/tmp/wfrefwef__cross@0.2.5/release/deps",
            "-C", "link-arg=-fuse-ld=/usr/local/bin/mold",
        ]);

        let (st, args) = as_rustc(PWD, arguments.clone()).unwrap();

        assert_eq!(
            st,
            RustcArgs {
                crate_type: "bin".to_owned(),
                emit: "dep-info,link".to_owned(),
                externs: Default::default(),
                metadata: "96fe5c8493f1a08f".to_owned(),
                incremental: None,
                input: as_argument("src/build.rs").into(),
                out_dir: as_argument(
                    "/tmp/wfrefwef__cross@0.2.5/release/build/cross-96fe5c8493f1a08f"
                )
                .into(),
                target_path: as_argument("/tmp/wfrefwef__cross@0.2.5/release").into(),
            }
        );

        #[rustfmt::skip]
        assert_eq!(as_arguments(&[
                "rustcbuildx",
                "$HOME/.rustup/toolchains/stable-x86_64-unknown-linux-gnu/bin/rustc",
                "--crate-name", "build_script_build",
                "--edition", "2021",
                "--error-format", "json",
                "--json", "diagnostic-rendered-ansi,artifacts,future-incompat",
                "--diagnostic-width", "211",
                "--crate-type", "bin",
                "--emit", "dep-info,link",
                "-C", "embed-bitcode=no",
                "-C", "debug-assertions=off",
                "--cfg", "feature=\"default\"",
                "-C", "metadata=96fe5c8493f1a08f",
                "-C", "extra-filename=-96fe5c8493f1a08f",
                "--out-dir", "/tmp/wfrefwef__cross@0.2.5/release/build/cross-96fe5c8493f1a08f",
                "-L", "dependency=/tmp/wfrefwef__cross@0.2.5/release/deps",
             ]), args);
    }

    #[test]
    fn args_when_build_script_main() {
        #[rustfmt::skip]
        // Original ordering per rustc 1.73.0
        let arguments = as_arguments(&[
            "$HOME/work/rustcbuildx/rustcbuildx/rustcbuildx",
            "$HOME/.rustup/toolchains/stable-x86_64-unknown-linux-gnu/bin/rustc",
            "--crate-name", "build_script_main",
            "--edition=2018",
            "$HOME/.cargo/registry/src/index.crates.io-6f17d22bba15001f/openssl-sys-0.9.95/build/main.rs",
            "--error-format=json",
            "--json=diagnostic-rendered-ansi,artifacts,future-incompat",
            "--crate-type", "bin",
            "--emit=dep-info,link",
            "-C", "embed-bitcode=no",
            "-C", "debug-assertions=off",
            "-C", "metadata=99f749eccead4467",
            "-C", "extra-filename=-99f749eccead4467",
            "--out-dir", "$HOME/instst/release/build/openssl-sys-99f749eccead4467",
            "-L", "dependency=$HOME/instst/release/deps",
            "--extern", "cc=$HOME/instst/release/deps/libcc-3c316ebdde73b0fe.rlib",
            "--extern", "pkg_config=%HOME/instst/release/deps/libpkg_config-a6962381fee76247.rlib",
            "--extern", "vcpkg=$HOME/instst/release/deps/libvcpkg-ebcbc23bfdf4209b.rlib",
            "--cap-lints", "warn",
        ]);

        let (st, args) = as_rustc(PWD, arguments.clone()).unwrap();

        assert_eq!(
            st,
            RustcArgs {
                crate_type: "bin".to_owned(),
                emit: "dep-info,link".to_owned(),
                externs: [
                    "libcc-3c316ebdde73b0fe.rlib".to_owned(),
                    "libpkg_config-a6962381fee76247.rlib".to_owned(),
                    "libvcpkg-ebcbc23bfdf4209b.rlib".to_owned(),
                ].into(),
                metadata: "99f749eccead4467".to_owned(),
                incremental: None,
                input: as_argument("$HOME/.cargo/registry/src/index.crates.io-6f17d22bba15001f/openssl-sys-0.9.95/build/main.rs").into(),
                out_dir: as_argument(
                    "$HOME/instst/release/build/openssl-sys-99f749eccead4467"
                )
                .into(),
                target_path: as_argument("$HOME/instst/release").into(),
            }
        );

        #[rustfmt::skip]
        assert_eq!(as_arguments(&[
                "$HOME/work/rustcbuildx/rustcbuildx/rustcbuildx",
                "$HOME/.rustup/toolchains/stable-x86_64-unknown-linux-gnu/bin/rustc",
                "--crate-name", "build_script_main",
                "--edition", "2018",
                "--error-format", "json",
                "--json", "diagnostic-rendered-ansi,artifacts,future-incompat",
                "--crate-type", "bin",
                "--emit", "dep-info,link",
                "-C", "embed-bitcode=no",
                "-C", "debug-assertions=off",
                "-C", "metadata=99f749eccead4467",
                "-C", "extra-filename=-99f749eccead4467",
                "--out-dir", "$HOME/instst/release/build/openssl-sys-99f749eccead4467",
                "-L", "dependency=$HOME/instst/release/deps",
                "--extern", "cc=$HOME/instst/release/deps/libcc-3c316ebdde73b0fe.rlib",
                "--extern", "pkg_config=%HOME/instst/release/deps/libpkg_config-a6962381fee76247.rlib",
                "--extern", "vcpkg=$HOME/instst/release/deps/libvcpkg-ebcbc23bfdf4209b.rlib",
                "--cap-lints", "warn",
             ]), args);
    }
}
