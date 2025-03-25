use super::{Channel, Commit, D, V, Version};

#[test]
fn version_empty() {
    assert_eq!(Version::parse(""), None);
}

#[test]
fn version_no_explicit_channel() {
    assert_eq!(
        Version::parse("1.2.3"),
        Some(Version { triple: V!(1, 2, 3), channel: Channel::Stable, commit: None, tag: "" })
    );
}

#[test]
fn version_insufficent_numeric_components() {
    assert_eq!(Version::parse("1"), None);
    assert_eq!(Version::parse("1.0"), None);
}

#[test]
fn version_too_many_numeric_components() {
    assert_eq!(Version::parse("1.0.0.0"), None);
}

#[test]
fn version_numeric_components_trailing_dot() {
    assert_eq!(Version::parse("1."), None);
    assert_eq!(Version::parse("1.0."), None);
    assert_eq!(Version::parse("1.0.0."), None);
}

#[test]
fn version_channel() {
    assert_eq!(
        Version::parse("1.20.3-beta"),
        Some(Version {
            triple: V!(1, 20, 3),
            channel: Channel::Beta { prerelease: None },
            commit: None,
            tag: "",
        })
    );
    assert_eq!(
        Version::parse("1.20.3-nightly"),
        Some(Version { triple: V!(1, 20, 3), channel: Channel::Nightly, commit: None, tag: "" })
    );
    assert_eq!(
        Version::parse("1.20.3-dev"),
        Some(Version { triple: V!(1, 20, 3), channel: Channel::Dev, commit: None, tag: "" })
    );
}

#[test]
fn version_empty_channel() {
    assert_eq!(Version::parse("1.0.0-"), None);
}

#[test]
fn version_beta_channel_prerelease() {
    assert_eq!(
        Version::parse("1.2.3-beta.144"),
        Some(Version {
            triple: V!(1, 2, 3),
            channel: Channel::Beta { prerelease: Some(144) },
            commit: None,
            tag: "",
        })
    );
}

#[test]
fn version_commit_info() {
    assert_eq!(
        Version::parse("0.0.0-dev (123456789 2000-01-01)"),
        Some(Version {
            triple: V!(0, 0, 0),
            channel: Channel::Dev,
            commit: Some(Commit { short_sha: "123456789", date: D!(2000, 01, 01) }),
            tag: "",
        })
    );
}

#[test]
fn version_commit_info_no_channel() {
    assert_eq!(
        Version::parse("999.999.999 (000000000 1970-01-01)"),
        Some(Version {
            triple: V!(999, 999, 999),
            channel: Channel::Stable,
            commit: Some(Commit { short_sha: "000000000", date: D!(1970, 01, 01) }),
            tag: "",
        })
    );
}

#[test]
fn version_tag() {
    assert_eq!(
        Version::parse("0.0.0 (000000000 0000-01-01) (THIS IS A TAG)"),
        Some(Version {
            triple: V!(0, 0, 0),
            channel: Channel::Stable,
            commit: Some(Commit { short_sha: "000000000", date: D!(0000, 01, 01) }),
            tag: "THIS IS A TAG",
        })
    );
}

#[test]
fn version_trailing_whitespace() {
    assert_eq!(Version::parse(" "), None);
    assert_eq!(Version::parse("0.0.0 "), None);
    assert_eq!(Version::parse("0.0.0-dev "), None);
    assert_eq!(Version::parse("0.0.0-dev (000000000 0000-01-01) "), None);
    assert_eq!(Version::parse("0.0.0-dev (000000000 0000-01-01) (TAG) "), None);
}

#[test]
fn version_double_whitespace() {
    assert_eq!(Version::parse("0.0.0-dev  (000000000 0000-01-01)"), None);
    assert_eq!(Version::parse("0.0.0-dev (000000000  0000-01-01)"), None);
}

// Nobody says the tag can't be multiline, right?
// FIXME: Double-check bootstrap.
#[test]
fn version_multiline_tag() {
    assert_eq!(
        Version::parse("0.0.0 (abcdef 0000-01-01) (this\nis\nspanning\nacross\nlines)"),
        Some(Version {
            triple: V!(0, 0, 0),
            channel: Channel::Stable,
            commit: Some(Commit { short_sha: "abcdef", date: D!(0000, 01, 01) }),
            tag: "this\nis\nspanning\nacross\nlines"
        })
    );
}
