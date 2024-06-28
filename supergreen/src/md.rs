// Our own MetaData utils

use std::{
    collections::BTreeSet,
    fs::{self},
};

use anyhow::{anyhow, bail, Result};
use camino::{Utf8Path, Utf8PathBuf};
use serde::{Deserialize, Serialize};

use crate::{base::RUST, cratesio::CRATESIO_STAGE_PREFIX, stage::Stage};

#[cfg_attr(test, derive(Default))]
#[derive(Clone, Deserialize, Serialize)]
pub struct Md {
    pub this: String,
    pub deps: Vec<String>,

    pub contexts: BTreeSet<BuildContext>,

    pub stages: BTreeSet<DockerfileStage>,
}
impl Md {
    #[inline]
    #[must_use]
    pub fn new(this: &str) -> Self {
        Self { this: this.to_owned(), deps: vec![], contexts: [].into(), stages: [].into() }
    }

    pub fn from_file(md_path: &Utf8Path) -> Result<Self> {
        // TODO: deser from stream
        let md_raw =
            fs::read_to_string(md_path).map_err(|e| anyhow!("Failed reading Md {md_path}: {e}"))?;
        toml::de::from_str(&md_raw).map_err(|e| anyhow!("Failed deserializing Md {md_path}: {e}"))
    }

    pub fn to_string(&self) -> Result<String> {
        toml::to_string_pretty(self).map_err(|e| anyhow!("Failed serializing Md: {e}"))
    }

    pub fn rust_stage(&self) -> Option<DockerfileStage> {
        self.stages.iter().find(|DockerfileStage { name, .. }| name == RUST).cloned()
    }

    pub fn push_block(&mut self, name: &Stage, script: String) {
        self.stages.insert(DockerfileStage { name: name.to_string(), script });
    }

    pub fn append_blocks(
        &self,
        dockerfile: &mut String,
        visited: &mut BTreeSet<String>,
    ) -> Result<()> {
        let mut stages = self.stages.iter().filter(|DockerfileStage { name, .. }| name != RUST);

        let Some(DockerfileStage { name, script }) = stages.next() else {
            bail!("BUG: has to have at least one stage")
        };

        let mut filter = ""; // not an actual stage name
        if name.starts_with(CRATESIO_STAGE_PREFIX) {
            filter = name;
            if visited.insert(name.to_owned()) {
                dockerfile.push_str(script);
            }
        } else {
            // Otherwise, write it back in
            dockerfile.push_str(script);
        }

        for DockerfileStage { name, script } in stages {
            if name == filter {
                continue;
            }
            dockerfile.push_str(script);
        }

        Ok(())
    }

    pub fn extend_from_externs(
        &mut self,
        mds: Vec<(Utf8PathBuf, Self)>,
    ) -> Result<Vec<Utf8PathBuf>> {
        use szyk::{sort, Node};

        let mut dag: Vec<_> = mds
            .into_iter()
            .map(|(md_path, md)| {
                let this = dec(&md.this);
                self.deps.push(md.this);
                self.contexts
                    .extend(md.contexts.into_iter().filter(BuildContext::is_readonly_mount));
                Node::new(this, decs(&md.deps), md_path)
            })
            .collect();
        let this = dec(&self.this);
        dag.push(Node::new(this, decs(&self.deps), "".into()));

        let mut md_paths = match sort(&dag, this) {
            Ok(ordering) => ordering,
            Err(e) => bail!("Failed topolosorting {}: {e:?}", self.this),
        };
        md_paths.truncate(md_paths.len() - 1); // pop last (it's self.this's empty path)

        Ok(md_paths)
    }
}

#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub struct DockerfileStage {
    pub name: String,
    pub script: String,
}

// pub(crate) const HDR: &str = "# ";

// # syntax = ..
// # this = "x"
// # mnt = ["y", "z"]
// # contexts = [
// #   { name = "a", uri = "b" },
// # ]
// FROM ..

#[inline]
#[must_use]
fn dec(#[allow(clippy::ptr_arg)] x: &String) -> u64 {
    u64::from_str_radix(x, 16).expect("16-digit hex str")
}

#[inline]
#[must_use]
fn decs(xs: &[String]) -> Vec<u64> {
    xs.iter().map(dec).collect()
}

#[test]
fn dec_decs() {
    #[inline]
    fn enc(metadata: u64) -> String {
        format!("{metadata:#x}").trim_start_matches("0x").to_owned()
    }

    let as_hex = "dab737da4696ee62".to_owned();
    let as_dec = 15760126831633034850;
    assert_eq!(dec(&as_hex), as_dec);
    assert_eq!(enc(as_dec), format!("{as_hex}"));
}

#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub struct BuildContext {
    pub name: String, // TODO: constrain with Docker stage name pattern
    pub uri: String,  // TODO: constrain with Docker build-context URIs
}
impl BuildContext {
    #[inline]
    #[must_use]
    pub fn is_readonly_mount(&self) -> bool {
        self.name.starts_with(CRATESIO_STAGE_PREFIX) ||
        // TODO: create a stage from sources where able (public repos) use --secret mounts for private deps (and secret direct artifacts)
        self.name.starts_with("input_") ||
         // TODO: link this to the build script it's coming from
         self.name.starts_with("crate_out-")
    }
}

impl From<(String, String)> for BuildContext {
    fn from((name, uri): (String, String)) -> Self {
        Self { name, uri }
    }
}

#[test]
fn md_ser() {
    let md = Md {
        this: "711ba64e1183a234".to_owned(),
        deps: vec!["81529f4c2380d9ec".to_owned(), "88a4324b2aff6db9".to_owned()],
        contexts: [BuildContext { name: "rust".to_owned(), uri: "docker-image://docker.io/library/rust:1.77.2-slim@sha256:090d8d4e37850b349b59912647cc7a35c6a64dba8168f6998562f02483fa37d7".to_owned() }].into(),
        ..Default::default()
    };

    let ser = md.to_string().unwrap();
    pretty_assertions::assert_eq!(
        r#"
this = "711ba64e1183a234"
deps = [
    "81529f4c2380d9ec",
    "88a4324b2aff6db9",
]
stages = []

[[contexts]]
name = "rust"
uri = "docker-image://docker.io/library/rust:1.77.2-slim@sha256:090d8d4e37850b349b59912647cc7a35c6a64dba8168f6998562f02483fa37d7"
"#[1..],
        ser
    );
}

#[test]
fn md_utils() {
    use std::fs;

    use mktemp::Temp;

    use crate::base::RUST;

    const LONG:&str= "docker-image://docker.io/library/rust:1.69.0-slim@sha256:8b85a8a6bf7ed968e24bab2eae6f390d2c9c8dbed791d3547fef584000f48f9e";

    let tmp = Temp::new_file().unwrap();
    fs::write(&tmp, format!(r#"this = "9494aa6093cd94c9"
deps = ["0dc1fe2644e3176a"]
contexts = [
  {{ name = "rust-base", uri = {LONG:?} }},
  {{ name = "input_src_lib_rs--rustversion-1.0.9", uri = "/home/maison/.cargo/registry/src/github.com-1ecc6299db9ec823/rustversion-1.0.9" }},
  {{ name = "crate_out-...", uri = "/home/maison/code/thing.git/target/debug/build/rustversion-ae69baa7face5565/out" }},
]
stages = []
"#)).unwrap();

    let this = "9494aa6093cd94c9".to_owned();
    let deps = vec!["0dc1fe2644e3176a".to_owned()];
    let contexts = [
        BuildContext { name: RUST.to_owned(), uri: LONG.to_owned() },
        BuildContext {
            name: "input_src_lib_rs--rustversion-1.0.9".to_owned(),
            uri: "/home/maison/.cargo/registry/src/github.com-1ecc6299db9ec823/rustversion-1.0.9"
                .to_owned(),
        },
        BuildContext {
            name: "crate_out-...".to_owned(),
            uri: "/home/maison/code/thing.git/target/debug/build/rustversion-ae69baa7face5565/out"
                .to_owned(),
        },
    ];
    let md = Md::from_file(tmp.as_path().try_into().unwrap()).unwrap();
    assert_eq!(md.this, this);
    assert_eq!(md.deps, deps);
    assert_eq!(md.contexts, contexts.clone().into());

    let used: Vec<_> = contexts.into_iter().filter(BuildContext::is_readonly_mount).collect();
    assert!(used[0].name.starts_with("input_"));
    assert!(used[1].name.starts_with("crate_out-"));
    assert_eq!(used.len(), 2);
}

#[test]
fn md_parsing_failure() {
    use std::fs;

    use mktemp::Temp;

    let tmp = Temp::new_file().unwrap();
    fs::write(&tmp, r#"this = "81529f4c2380d9ec"
deps = [[]]
contexts = [
  { name = "rust", uri = "docker-image://docker.io/library/rust:1.77.2-slim@sha256:090d8d4e37850b349b59912647cc7a35c6a64dba8168f6998562f02483fa37d7" },
]
"#).unwrap();

    let err = Md::from_file(tmp.as_path().try_into().unwrap())
        .err()
        .map(|x| x.to_string())
        .unwrap_or_default();
    dbg!(&err);
    assert!(err.contains("\n2 | deps = [[]]\n"));
    assert!(err.contains("\ninvalid type: sequence, expected a string\n"));
}
