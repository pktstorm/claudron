use std::process::Command;

/// Split a dotted version into numeric segments.
///
/// A segment that does not parse counts as 0 rather than panicking -- version
/// strings come from transcripts and are not guaranteed well-formed.
pub fn parse(v: &str) -> Vec<u32> {
    v.split('.')
        .map(|seg| seg.parse::<u32>().unwrap_or(0))
        .collect()
}

/// True when `a` is an earlier version than `b`.
pub fn is_older(a: &str, b: &str) -> bool {
    let (a, b) = (parse(a), parse(b));
    let len = a.len().max(b.len());
    for i in 0..len {
        // A missing segment is 0, so "2.1" precedes "2.1.1".
        let (x, y) = (a.get(i).copied().unwrap_or(0), b.get(i).copied().unwrap_or(0));
        if x != y {
            return x < y;
        }
    }
    false
}

/// The version of the `claude` binary on PATH, if it can be determined.
pub fn installed() -> Option<String> {
    let out = Command::new("claude").arg("--version").output().ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&out.stdout);
    // Output looks like "2.1.220 (Claude Code)" -- take the leading token.
    text.split_whitespace().next().map(str::to_string)
}

/// The version everything else is compared against.
pub fn baseline(installed: Option<String>, observed: &[String]) -> Option<String> {
    // Taking the max of both is what makes this self-correcting: Claude Code
    // auto-updates, so a value read once at startup can go stale while the app
    // runs. When a session appears carrying a newer version, that session is
    // itself proof a newer version exists.
    let mut best = installed;
    for v in observed {
        best = match best {
            None => Some(v.clone()),
            Some(cur) if is_older(&cur, v) => Some(v.clone()),
            Some(cur) => Some(cur),
        };
    }
    best
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_dotted_segments() {
        assert_eq!(parse("2.1.220"), vec![2, 1, 220]);
        assert_eq!(parse("2.1"), vec![2, 1]);
    }

    #[test]
    fn an_unparseable_segment_counts_as_zero() {
        assert_eq!(parse("2.1.beta"), vec![2, 1, 0]);
        assert_eq!(parse(""), vec![0]);
    }

    #[test]
    fn compares_numerically_not_as_strings() {
        // The whole feature inverts if this is a string comparison:
        // "2.1.99" > "2.1.220" lexically, but 99 < 220 numerically.
        assert!(is_older("2.1.99", "2.1.220"));
        assert!(!is_older("2.1.220", "2.1.99"));
    }

    #[test]
    fn equal_versions_are_not_older() {
        assert!(!is_older("2.1.220", "2.1.220"));
    }

    #[test]
    fn compares_across_segment_counts() {
        assert!(is_older("2.1", "2.1.1"));
        assert!(!is_older("2.2", "2.1.9"));
    }

    #[test]
    fn baseline_prefers_whichever_is_newer() {
        // The binary can be newer than anything observed yet.
        assert_eq!(
            baseline(Some("2.1.220".into()), &["2.1.205".into()]),
            Some("2.1.220".into())
        );
        // And an observed session can be newer than a binary read at startup,
        // because Claude Code auto-updates while Claudron runs.
        assert_eq!(
            baseline(Some("2.1.220".into()), &["2.1.221".into()]),
            Some("2.1.221".into())
        );
    }

    #[test]
    fn baseline_falls_back_to_observed_when_the_binary_is_unknown() {
        assert_eq!(
            baseline(None, &["2.1.205".into(), "2.1.220".into()]),
            Some("2.1.220".into())
        );
    }

    #[test]
    fn baseline_is_none_when_there_is_nothing_to_compare() {
        assert_eq!(baseline(None, &[]), None);
    }

    #[test]
    #[ignore]
    fn reads_the_real_installed_version() {
        match installed() {
            Some(v) => {
                println!("installed claude version: {v}");
                assert!(!parse(&v).is_empty());
                assert!(v.chars().next().is_some_and(|c| c.is_ascii_digit()));
            }
            None => println!("claude binary not found or not runnable -- fallback path"),
        }
    }
}
