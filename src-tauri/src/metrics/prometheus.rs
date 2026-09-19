//! A small parser for the Prometheus text exposition format, enough for an exporter's
//! `/metrics` page: `name{label="value",...} value [timestamp]`. Comments and blank lines are
//! skipped; escaped quotes and backslashes inside label values are honoured.

use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq)]
pub struct Sample {
    pub name: String,
    pub labels: BTreeMap<String, String>,
    pub value: f64,
}

impl Sample {
    pub fn label(&self, key: &str) -> Option<&str> {
        self.labels.get(key).map(String::as_str)
    }
}

/// Every sample line in `text`; anything unreadable is skipped rather than failing the page.
pub fn parse(text: &str) -> Vec<Sample> {
    text.lines().filter_map(parse_line).collect()
}

fn parse_line(line: &str) -> Option<Sample> {
    let line = line.trim();
    if line.is_empty() || line.starts_with('#') {
        return None;
    }
    let (name, labels, tail) = match line.find('{') {
        Some(open) => {
            let close = closing_brace(line, open)?;
            (
                &line[..open],
                parse_labels(&line[open + 1..close]),
                &line[close + 1..],
            )
        }
        None => {
            let end = line.find(char::is_whitespace).unwrap_or(line.len());
            (&line[..end], BTreeMap::new(), &line[end..])
        }
    };
    let name = name.trim();
    if name.is_empty() {
        return None;
    }
    let value: f64 = tail.split_whitespace().next()?.parse().ok()?;
    Some(Sample {
        name: name.to_string(),
        labels,
        value,
    })
}

/// The index of the `}` that closes the label set opened at `open`, skipping quoted text.
fn closing_brace(line: &str, open: usize) -> Option<usize> {
    let mut in_quotes = false;
    let mut escaped = false;
    for (index, byte) in line.bytes().enumerate().skip(open + 1) {
        if escaped {
            escaped = false;
            continue;
        }
        match byte {
            b'\\' if in_quotes => escaped = true,
            b'"' => in_quotes = !in_quotes,
            b'}' if !in_quotes => return Some(index),
            _ => {}
        }
    }
    None
}

fn parse_labels(text: &str) -> BTreeMap<String, String> {
    let mut labels = BTreeMap::new();
    let mut rest = text;
    loop {
        rest = rest.trim_start_matches([',', ' ', '\t']);
        if rest.is_empty() {
            break;
        }
        let Some(equals) = rest.find('=') else {
            break;
        };
        let key = rest[..equals].trim().to_string();
        let Some(after) = rest[equals + 1..].trim_start().strip_prefix('"') else {
            break;
        };
        let mut value = String::new();
        let mut end = None;
        let mut chars = after.char_indices();
        while let Some((index, character)) = chars.next() {
            match character {
                '\\' => {
                    if let Some((_, next)) = chars.next() {
                        value.push(match next {
                            'n' => '\n',
                            other => other,
                        });
                    }
                }
                '"' => {
                    end = Some(index);
                    break;
                }
                other => value.push(other),
            }
        }
        let Some(end) = end else {
            break;
        };
        labels.insert(key, value);
        rest = &after[end + 1..];
    }
    labels
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_dcgm_style_page() {
        let page = "# HELP DCGM_FI_DEV_GPU_TEMP GPU temperature (in C).\n\
                    # TYPE DCGM_FI_DEV_GPU_TEMP gauge\n\
                    DCGM_FI_DEV_GPU_TEMP{gpu=\"0\",UUID=\"GPU-1\",modelName=\"Card A\",Hostname=\"node-a\"} 41\n\
                    DCGM_FI_DEV_POWER_USAGE{gpu=\"0\",UUID=\"GPU-1\"} 15.650000 1700000000000\n\
                    \n\
                    process_start_time_seconds 1.7e9\n";
        let samples = parse(page);
        assert_eq!(samples.len(), 3);
        assert_eq!(samples[0].name, "DCGM_FI_DEV_GPU_TEMP");
        assert_eq!(samples[0].label("modelName"), Some("Card A"));
        assert_eq!(samples[0].label("Hostname"), Some("node-a"));
        assert_eq!(samples[0].value, 41.0);
        assert_eq!(samples[1].value, 15.65);
        assert!(samples[2].labels.is_empty());
        assert_eq!(samples[2].value, 1.7e9);
    }

    #[test]
    fn honours_escapes_and_braces_inside_values() {
        let samples = parse(r#"m{a="x\"y",b="with } brace",c="back\\slash"} 1"#);
        assert_eq!(samples.len(), 1);
        assert_eq!(samples[0].label("a"), Some("x\"y"));
        assert_eq!(samples[0].label("b"), Some("with } brace"));
        assert_eq!(samples[0].label("c"), Some("back\\slash"));
    }

    #[test]
    fn skips_unreadable_lines() {
        let samples = parse("good 1\nbad{unterminated=\"x 2\nalso_bad notanumber\n{} 3\n");
        assert_eq!(samples.len(), 1);
        assert_eq!(samples[0].name, "good");
    }

    #[test]
    fn reads_nan_and_infinity() {
        let samples = parse("a NaN\nb +Inf\n");
        assert!(samples[0].value.is_nan());
        assert!(samples[1].value.is_infinite());
    }
}
