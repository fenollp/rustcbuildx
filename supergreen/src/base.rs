use crate::runner::maybe_lock_image;

pub const RUST: &str = "rust-base";

#[derive(Clone, Debug, PartialEq)]
pub struct RustcV {
    pub version: String,
    pub commit: String,
    pub date: String,
    pub channel: String,

    pub base: String,
}

#[derive(Clone, Debug, PartialEq)]
pub enum BaseImage {
    Image(String),
    RustcV(RustcV),
}

#[test]
fn test_from_rustc_v() {
    assert_eq!(
        // # rustc -V
        BaseImage::from_rustc_v("rustc 1.79.0 (129f3b996 2024-06-10)\n".to_owned()),
        Some(BaseImage::Image("docker-image://docker.io/library/rust:1.79.0-slim".to_owned()))
    );

    assert_eq!(
        // # rustc +nightly -V
        BaseImage::from_rustc_v("rustc 1.81.0-nightly (d7f6ebace 2024-06-16)\n".to_owned()),
        Some(BaseImage::RustcV(RustcV {
            version: "1.81.0".to_owned(),
            commit: "d7f6ebace".to_owned(),
            date: "2024-06-16".to_owned(),
            channel: "nightly".to_owned(),
            base: "".to_owned(),
        }))
    );
}

impl BaseImage {
    #[inline]
    #[must_use]
    pub fn base(&self) -> String {
        match self {
            Self::Image(img) => img.clone(),
            Self::RustcV(RustcV { base, .. }) if base.is_empty() => {
                "docker.io/library/debian:12-slim".to_owned()
            }
            Self::RustcV(RustcV { base, .. }) => base.clone(),
        }
    }

    #[inline]
    #[must_use]
    pub fn from_rustc_v(rustc_v: String) -> Option<Self> {
        let x = rustc_v.trim_start_matches("rustc ").replace(['(', ')', '\n'], "");

        let cut: Vec<&str> = x.splitn(3, ' ').collect();
        let [version, commit, date] = cut[..] else {
            return None;
        };

        for channel in ["nightly", "stable", "beta"] {
            let suffix = format!("-{channel}");
            if version.ends_with(&suffix) {
                return Some(Self::RustcV(RustcV {
                    version: version.trim_end_matches(&suffix).to_owned(),
                    commit: commit.to_owned(),
                    date: date.to_owned(),
                    channel: channel.to_owned(),
                    base: "".to_owned(),
                }));
            }
        }

        Some(BaseImage::Image(format!("docker-image://docker.io/library/rust:{version}-slim")))
    }

    pub async fn maybe_lock_base(self) -> Self {
        let base = maybe_lock_image(self.base()).await;
        match self {
            Self::Image(_) => Self::Image(base),
            Self::RustcV(rustcv @ RustcV { .. }) => Self::RustcV(RustcV { base, ..rustcv }),
        }
    }

    pub fn block(&self) -> String {
        let base = self.base();
        let base = base.trim_start_matches("docker-image://");

        let mut block = String::new();
        if let BaseImage::RustcV(RustcV { date, channel, .. }) = self {
            // TODO? maybe use commit & version as selector too?

            // FIXME: multiplatformify (using auto ARG.s)

            // TODO: use https://github.com/reproducible-containers/repro-sources-list.sh

            if channel == "nightly" {
                block.push_str(&format!(r#"
FROM scratch AS rustup
ADD --chmod=0755 --checksum=sha256:6aeece6993e902708983b209d04c0d1dbb14ebb405ddb87def578d41f920f56d \
  https://static.rust-lang.org/rustup/archive/1.27.1/x86_64-unknown-linux-gnu/rustup-init /rustup-init
FROM {base} AS {RUST}
ENV RUSTUP_HOME=/usr/local/rustup \
     CARGO_HOME=/usr/local/cargo \
           PATH=/usr/local/cargo/bin:$PATH
RUN \
    set -ux \
 && apt-get update \
 && apt-get install -y --no-install-recommends \
      ca-certificates \
      gcc \
      libc6-dev
RUN \
  --mount=from=rustup,source=/rustup-init,target=/rustup-init \
    set -ux \
 && /rustup-init -y --no-modify-path --profile minimal --default-toolchain nightly-{date} \
 && chmod -R a+w $RUSTUP_HOME $CARGO_HOME \
 && rustup --version \
 && cargo --version \
 && rustc --version \
 && apt-get remove -y --auto-remove \
 && rm -rf /var/lib/apt/lists/*
"#
                    )[1..],
                );
            }
        }

        if block.is_empty() {
            format!("FROM {base} AS {RUST}\n")
        } else {
            block
        }
    }
}
