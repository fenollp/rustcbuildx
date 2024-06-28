use std::{
    collections::BTreeSet,
    mem,
    process::Stdio,
    time::{Duration, Instant},
};

use anyhow::{anyhow, Result};
use camino::Utf8Path;
use tokio::{
    io::{AsyncBufRead, AsyncBufReadExt, BufReader as TokioBufReader, Lines},
    join,
    process::Command,
    spawn,
    task::JoinHandle,
    time::timeout,
};

use crate::{
    base::BaseImage,
    envs::{base_image, cache_image, runner},
    extensions::ShowCmd,
    md::BuildContext,
    stage::Stage,
};

pub const MARK_STDOUT: &str = "::STDOUT:: ";
pub const MARK_STDERR: &str = "::STDERR:: ";

#[must_use]
pub async fn maybe_lock_image(mut img: String) -> String {
    // Lock image, as podman(4.3.1) does not respect --pull=false (fully, anyway)
    if img.starts_with("docker-image://") && !img.contains("@sha256:") {
        if let Some(line) = Command::new(runner())
            .kill_on_drop(true)
            .arg("inspect")
            .arg("--format={{index .RepoDigests 0}}")
            .arg(img.trim_start_matches("docker-image://"))
            .output()
            .await
            .ok()
            .and_then(|o| String::from_utf8(o.stdout).ok())
            .and_then(|x| x.lines().next().map(ToOwned::to_owned))
        {
            img.push_str(line.trim_start_matches(|c| c != '@'));
        }
        // FIXME: else HTTP read registry for platform
    }
    img
}

pub async fn build(
    krate: &str,
    command: &str,
    dockerfile_path: &Utf8Path,
    target: Stage,
    contexts: &BTreeSet<BuildContext>,
    out_dir: impl AsRef<Utf8Path>,
) -> Result<Option<i32>> {
    let mut cmd = Command::new(command);
    cmd.arg("--debug");
    cmd.arg("build");

    cmd.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());

    // Makes sure the underlying OS process dies with us
    cmd.kill_on_drop(true);

    // Makes sure that the BuildKit builder is used by either runner
    cmd.env("DOCKER_BUILDKIT", "1");

    //TODO: (use if set) cmd.env("SOURCE_DATE_EPOCH", "0"); // https://reproducible-builds.org/docs/source-date-epoch

    cmd.arg("--builder=supergreen");

    if false {
        cmd.arg("--no-cache");
    }

    //     cmd.arg(format!("--cache-to=type=registry,ref={img},mode=max,compression=zstd,force-compression=true,oci-mediatypes=true"));
    // // [2024-04-09T07:55:39Z DEBUG lib-autocfg-72217d8ded4d7ec7@177912] ✖ ERROR: Cache export is not supported for the docker driver.
    // // [2024-04-09T07:55:39Z DEBUG lib-autocfg-72217d8ded4d7ec7@177912] ✖ Switch to a different driver, or turn on the containerd image store, and try again.
    // // [2024-04-09T07:55:39Z DEBUG lib-autocfg-72217d8ded4d7ec7@177912] ✖ Learn more at https://docs.docker.com/go/build-cache-backends/

    if let Some(img) = cache_image() {
        let img = img.trim_start_matches("docker-image://");
        cmd.arg(format!("--cache-from=type=registry,ref={img},mode=max"));

        let tag = Stage::new(krate.to_owned())?; // TODO: include enough info for repro
        if tag.to_string().starts_with("bin-") {
            // FIXME: re-tag some tag to latest when pushing only?
            cmd.arg(format!("--tag={img}:latest"));
        } else {
            cmd.arg(format!("--tag={img}:{tag}"));
        }
        cmd.arg("--build-arg=BUILDKIT_INLINE_CACHE=1"); // https://docs.docker.com/build/cache/backends/inline
        cmd.arg("--load");
    }

    if false {
        // TODO: https://docs.docker.com/build/attestations/
        cmd.arg("--provenance=mode=max");
        cmd.arg("--sbom=true");
    }
    //cmd.arg("--metadata-file=/tmp/meta.json"); => {"buildx.build.ref": "default/default/o5c4435yz6o6xxxhdvekx5lmn"}

    if matches!(base_image().await, BaseImage::Image(_)) {
        // FIXME: pre-build rust stage with network then, never activate network ever.
        cmd.arg("--network=none");
    }

    cmd.arg("--platform=local");
    cmd.arg("--pull=false");
    cmd.arg(format!("--target={target}"));
    cmd.arg(format!("--output=type=local,dest={out_dir}", out_dir = out_dir.as_ref()));
    // cmd.arg("--build-arg=BUILDKIT_MULTI_PLATFORM=1"); // "deterministic output"? adds /linux_amd64/ to extracted cratesio

    // TODO: do without local Docker-compatible CLI
    // https://github.com/pyaillet/doggy
    // https://lib.rs/crates/bollard

    for BuildContext { name, uri } in contexts {
        cmd.arg(format!("--build-context={name}={uri}"));
    }

    cmd.arg(format!("--file={dockerfile_path}"));

    cmd.arg(dockerfile_path.parent().unwrap_or(dockerfile_path));

    let call = cmd.show();
    let envs: Vec<_> = cmd.as_std().get_envs().map(|(k, v)| format!("{k:?}={v:?}")).collect();
    let envs = envs.join(" ");

    log::info!(target: &krate, "Starting {call} (env: {envs:?})`");
    let errf = |e| anyhow!("Failed starting {call}: {e}");
    let mut child = cmd.spawn().map_err(errf)?;

    let pid = child.id().unwrap_or_default();
    log::info!(target: &krate, "Started {call} as pid={pid}`");
    let krate = format!("{krate}@{pid}");

    let out = TokioBufReader::new(child.stdout.take().expect("started")).lines();
    let err = TokioBufReader::new(child.stderr.take().expect("started")).lines();

    // TODO: try https://github.com/docker/buildx/pull/2500/files + find podman equivalent?
    let out_task = fwd(krate.clone(), out, "stdout", "➤", MARK_STDOUT);
    let err_task = fwd(krate.clone(), err, "stderr", "✖", MARK_STDERR);

    let (secs, code) = {
        let start = Instant::now();
        let res = child.wait().await;
        let elapsed = start.elapsed();
        (elapsed, res.map_err(|e| anyhow!("Failed calling {call}: {e}"))?.code())
    };
    log::info!(target: &krate, "command `{command} build` ran in {secs:?}: {code:?}");

    let longish = Duration::from_secs(2);
    match join!(timeout(longish, out_task), timeout(longish, err_task)) {
        (Err(e), _) | (_, Err(e)) => panic!(">>> {krate} ({longish:?}): {e}"),
        (_, _) => {}
    }
    drop(child);

    if !(0..=1).contains(&code.unwrap_or(-1)) {
        // Something is very wrong here. Try to be helpful by logging some info about runner config:
        let mut cmd = Command::new(command);
        let cmd = cmd.kill_on_drop(true).arg("info");
        let check =
            cmd.output().await.map_err(|e| anyhow!("Failed starting {}: {e}", cmd.show()))?;
        let stdout = String::from_utf8(check.stdout)
            .map_err(|e| anyhow!("Failed parsing check STDOUT: {e}"))?;
        let stderr = String::from_utf8(check.stderr)
            .map_err(|e| anyhow!("Failed parsing check STDERR: {e}"))?;
        log::warn!(
            target: &krate,
            "Runner info: [code: {}] [STDOUT {stdout}] [STDERR {stderr}]",
            check.status
        );
    }

    Ok(code)
}

#[inline]
fn fwd<R>(
    krate: String,
    mut stdio: Lines<R>,
    name: &'static str,
    badge: &'static str,
    mark: &'static str,
) -> JoinHandle<()>
where
    R: AsyncBufRead + Unpin + Send + 'static,
{
    let fwder = if mark == MARK_STDOUT { fwd_stdout } else { fwd_stderr };
    spawn(async move {
        log::debug!(target: &krate, "Starting {name} task");
        let mut buf = String::new();
        loop {
            match stdio.next_line().await {
                Ok(None) => break,
                Ok(Some(line)) => {
                    log::debug!(target: &krate, "{badge} {line}");
                    if let Some(msg) = lift_stdio(&line, mark) {
                        fwder(&krate, msg, &mut buf);
                    }
                }
                Err(e) => {
                    log::warn!("Failed during piping of {name}: {e:?}");
                    break;
                }
            }
        }
        log::debug!(target: &krate, "Terminating {name} task");
        drop(stdio);
    })
}

#[test]
#[allow(clippy::str_to_string)] // assertx
fn support_long_broken_json_lines() {
    let logs = assertx::setup_logging_test();
    let lines = [
        r#"#42 1.312 ::STDERR:: {"$message_type":"artifact","artifact":"/tmp/thing","emit":"link""#,
        r#"#42 1.313 ::STDERR:: }"#,
    ];
    let mut buf = String::new();

    let msg = lift_stdio(lines[0], MARK_STDERR);
    assert_eq!(msg, Some(r#"{"$message_type":"artifact","artifact":"/tmp/thing","emit":"link""#));
    fwd_stderr("krate", msg.unwrap(), &mut buf);
    assert_eq!(buf, r#"{"$message_type":"artifact","artifact":"/tmp/thing","emit":"link""#);

    let msg = lift_stdio(lines[1], MARK_STDERR);
    assert_eq!(msg, Some("}"));
    fwd_stderr("krate", msg.unwrap(), &mut buf);
    assert_eq!(buf, "");

    // Then fwd_stderr
    // calls artifact_written(r#"{"$message_type":"artifact","artifact":"/tmp/thing","emit":"link"}"#)
    // which returns Some("/tmp/thing")
    assertx::assert_logs_contain_in_order!(
        logs,
        log::Level::Info => "rustc wrote /tmp/thing"
    );
}

#[inline]
fn fwd_stderr(krate: &str, msg: &str, buf: &mut String) {
    let show = |msg: &str| {
        eprintln!("{msg}");
        if let Some(file) = artifact_written(msg) {
            log::info!(target: &krate, "rustc wrote {file}")
        }
    };
    match (buf.is_empty(), msg.starts_with('{'), msg.ends_with('}')) {
        (true, true, true) => show(msg), // json
        (true, true, false) => buf.push_str(msg),
        (true, false, true) => show(msg),  // ?
        (true, false, false) => show(msg), // text
        (false, true, true) => {
            show(&mem::take(buf));
            show(msg) // json
        }
        (false, true, false) => {
            show(&mem::take(buf));
            buf.push_str(msg)
        }
        (false, false, true) => {
            buf.push_str(msg);
            show(&mem::take(buf));
        }
        (false, false, false) => {
            show(&mem::take(buf));
            show(msg) // text
        }
    }
}

#[inline]
fn fwd_stdout(_krate: &str, msg: &str, #[allow(clippy::ptr_arg)] _buf: &mut String) {
    println!("{msg}");
}

#[test]
fn stdio_passthrough_from_runner() {
    assert_eq!(lift_stdio("#47 1.714 ::STDOUT:: hi!", MARK_STDOUT), Some("hi!"));
    let lines = [
        r#"#47 1.714 ::STDERR:: {"$message_type":"artifact","artifact":"/tmp/clis-vixargs_0-1-0/release/deps/libclap_derive-fcea659dae5440c4.so","emit":"link"}"#,
        r#"#47 1.714 ::STDERR:: {"$message_type":"diagnostic","message":"2 warnings emitted","code":null,"level":"warning","spans":[],"children":[],"rendered":"warning: 2 warnings emitted\n\n"}"#,
        r#"#47 1.714 ::STDOUT:: hi!"#,
    ].into_iter().map(|line| lift_stdio(line, MARK_STDERR));
    assert_eq!(
        lines.collect::<Vec<_>>(),
        vec![
            Some(
                r#"{"$message_type":"artifact","artifact":"/tmp/clis-vixargs_0-1-0/release/deps/libclap_derive-fcea659dae5440c4.so","emit":"link"}"#
            ),
            Some(
                r#"{"$message_type":"diagnostic","message":"2 warnings emitted","code":null,"level":"warning","spans":[],"children":[],"rendered":"warning: 2 warnings emitted\n\n"}"#
            ),
            None,
        ]
    );
}

// Maybe replace with actual JSON deserialization
#[inline]
fn artifact_written(msg: &str) -> Option<&str> {
    let mut z = msg.split('"');
    let mut a = z.next();
    let mut b = z.next();
    let mut c = z.next();
    loop {
        match (a, b, c) {
            (Some("artifact"), Some(":"), Some(file)) => return Some(file),
            (_, _, Some(_)) => {}
            (_, _, None) => return None,
        }
        (a, b, c) = (b, c, z.next());
    }
}

#[inline]
fn lift_stdio<'a>(line: &'a str, mark: &'static str) -> Option<&'a str> {
    // Docker builds running shell code usually start like: #47 0.057
    let line = line.trim_start_matches(|c| ['#', '.', ' '].contains(&c) || c.is_ascii_digit());
    let msg = line.trim_start_matches(mark);
    let cut = msg.len() != line.len();
    let msg = msg.trim();
    (cut && !msg.is_empty()).then_some(msg)
}
