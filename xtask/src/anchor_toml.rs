// Pure text helpers for the Anchor.toml managed guardian-set block.

use eyre::{bail, OptionExt, Result};

pub const BEGIN_MARKER: &str =
    "# BEGIN guardian-sets (managed by xtask; run `just rotate-guardian-set`)";
pub const END_MARKER: &str = "# END guardian-sets";

/// Read `test.validator.url` from an Anchor.toml string.
pub fn validator_url(contents: &str) -> Option<String> {
    let value: toml::Value = contents.parse().ok()?;
    value
        .get("test")?
        .get("validator")?
        .get("url")?
        .as_str()
        .map(str::to_owned)
}

/// Replace the text strictly between the BEGIN and END marker lines.
pub fn splice_managed_block(contents: &str, new_interior: &str) -> Result<String> {
    let begin = match contents.find(BEGIN_MARKER) {
        Some(i) => i,
        None => bail!(
            "managed guardian-set block not found (missing `{BEGIN_MARKER}`); \
             run `just rotate-guardian-set` after adding the markers"
        ),
    };
    // End of the BEGIN marker line (include its trailing newline).
    let after_begin = contents[begin..]
        .find('\n')
        .map(|n| begin + n + 1)
        .unwrap_or(contents.len());
    let end = match contents[after_begin..].find(END_MARKER) {
        Some(i) => after_begin + i,
        None => bail!("managed guardian-set block malformed (missing `{END_MARKER}`)"),
    };
    let mut out = String::with_capacity(contents.len());
    out.push_str(&contents[..after_begin]);
    out.push_str(new_interior);
    if !new_interior.ends_with('\n') {
        out.push('\n');
    }
    out.push_str(&contents[end..]);
    Ok(out)
}

/// Collect addresses from uncommented `address = "..."` lines inside the managed block.
pub fn uncommented_addresses(contents: &str) -> Result<Vec<String>> {
    let begin = contents
        .find(BEGIN_MARKER)
        .ok_or_eyre("managed guardian-set block not found")?;
    let end = contents[begin..]
        .find(END_MARKER)
        .map(|i| begin + i)
        .ok_or_eyre("managed guardian-set block malformed")?;
    let mut addrs = Vec::new();
    for line in contents[begin..end].lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with('#') {
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix("address") {
            if let Some(q1) = rest.find('"') {
                if let Some(q2) = rest[q1 + 1..].find('"') {
                    addrs.push(rest[q1 + 1..q1 + 1 + q2].to_string());
                }
            }
        }
    }
    Ok(addrs)
}

/// Render the interior of the managed block: one entry per existing index (ascending),
/// uncommented iff the index is `active` or `active - 1`, otherwise commented out.
pub fn render_interior(existing: &[u32], active: u32, addr_of: impl Fn(u32) -> String) -> String {
    let previous = active.saturating_sub(1);
    let mut indices: Vec<u32> = existing.to_vec();
    indices.sort_unstable();
    let mut out = String::new();
    for (n, &i) in indices.iter().enumerate() {
        if n > 0 {
            out.push('\n');
        }
        let addr = addr_of(i);
        out.push_str(&format!("# Wormhole Guardian Set account (index = {i})\n"));
        if i == active || i == previous {
            out.push_str(&format!("[[test.validator.clone]]\naddress = \"{addr}\"\n"));
        } else {
            out.push_str(&format!(
                "# [[test.validator.clone]]\n# address = \"{addr}\"\n"
            ));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "\
prefix line
# BEGIN guardian-sets (managed by xtask; run `just rotate-guardian-set`)
# Wormhole Guardian Set account (index = 5)
# [[test.validator.clone]]
# address = \"AAA5\"

# Wormhole Guardian Set account (index = 6)
[[test.validator.clone]]
address = \"AAA6\"
# END guardian-sets
suffix line
";

    #[test]
    fn parses_validator_url() {
        let toml = "[test.validator]\nurl = \"https://api.devnet.solana.com\"\n";
        assert_eq!(
            validator_url(toml).as_deref(),
            Some("https://api.devnet.solana.com")
        );
        assert_eq!(validator_url("nothing = 1\n"), None);
    }

    #[test]
    fn lists_only_uncommented_addresses_in_block() {
        assert_eq!(
            uncommented_addresses(SAMPLE).unwrap(),
            vec!["AAA6".to_string()]
        );
    }

    #[test]
    fn splice_replaces_only_interior() {
        let out = splice_managed_block(SAMPLE, "NEW INTERIOR").unwrap();
        assert!(out.starts_with("prefix line\n"));
        assert!(out.ends_with("suffix line\n"));
        assert!(out.contains(&format!("{BEGIN_MARKER}\nNEW INTERIOR\n{END_MARKER}")));
        assert!(!out.contains("AAA6"));
    }

    #[test]
    fn splice_errors_without_markers() {
        assert!(splice_managed_block("no markers here\n", "x").is_err());
    }

    #[test]
    fn splice_does_not_double_newline_when_interior_already_ends_with_one() {
        let out = splice_managed_block(SAMPLE, "NEW INTERIOR\n").unwrap();
        assert!(out.contains(&format!("NEW INTERIOR\n{END_MARKER}")));
        assert!(!out.contains(&format!("NEW INTERIOR\n\n{END_MARKER}")));
    }

    #[test]
    fn renders_active_and_previous_uncommented() {
        let out = render_interior(&[5, 6, 7], 7, |i| format!("ADDR{i}"));
        // index 7 (active) uncommented
        assert!(out.contains("# Wormhole Guardian Set account (index = 7)\n[[test.validator.clone]]\naddress = \"ADDR7\""));
        // index 6 (previous) uncommented
        assert!(out.contains("# Wormhole Guardian Set account (index = 6)\n[[test.validator.clone]]\naddress = \"ADDR6\""));
        // index 5 (older) commented
        assert!(out.contains("# Wormhole Guardian Set account (index = 5)\n# [[test.validator.clone]]\n# address = \"ADDR5\""));
    }

    #[test]
    fn render_interior_sorts_ascending_regardless_of_input_order() {
        let out = render_interior(&[7, 5, 6], 6, |i| format!("ADDR{i}"));
        let pos5 = out
            .find("# Wormhole Guardian Set account (index = 5)")
            .unwrap();
        let pos6 = out
            .find("# Wormhole Guardian Set account (index = 6)")
            .unwrap();
        let pos7 = out
            .find("# Wormhole Guardian Set account (index = 7)")
            .unwrap();
        assert!(pos5 < pos6, "expected index 5 to appear before index 6");
        assert!(pos6 < pos7, "expected index 6 to appear before index 7");
    }
}
