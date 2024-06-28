use nutype::nutype;

#[nutype(sanitize(with=oci_name), validate(predicate=tag_name), derive(Debug, Display, PartialEq))]
pub struct Stage(String);

#[inline]
#[must_use]
fn oci_name(name: String) -> String {
    name.to_lowercase()
        .replace(|c: char| c != '.' && !c.is_alphanumeric(), "-")
        .replace("---", "-")
        .to_owned()
}

#[inline]
#[must_use]
fn tag_name(name: &str) -> bool {
    !name.starts_with(['-', '.']) && name.len() <= 128
}

#[test]
fn safe_stages() {
    let mk = |x| Stage::new(x).unwrap().to_string();

    pretty_assertions::assert_eq!(
        mk("libgit2-sys-0.14.2+1.5.1-index.crates.io-6f17d22bba15001f"),
        "libgit2-sys-0.14.2-1.5.1-index.crates.io-6f17d22bba15001f".to_owned()
    );

    assert!(Stage::new("-libgit2-sys-0.14.2+1.5.1-index.crates.io-6f17d22bba15001f").is_err());
    assert!(Stage::new(".libgit2-sys-0.14.2+1.5.1-index.crates.io-6f17d22bba15001f").is_err());
    assert!(Stage::new(".libgit2-".to_owned() + &"b".repeat(128)).is_err());
}
