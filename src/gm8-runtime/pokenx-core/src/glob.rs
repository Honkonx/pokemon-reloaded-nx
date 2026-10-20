//! Directory-mask matching for GML's `file_find_first`/`file_find_next` (replaces the `glob` crate).
//! GameMaker only uses `*` and `?` in the last path component, compared without regard to case.

use std::path::PathBuf;

#[derive(Debug)]
pub struct PatternError(pub String);

impl std::fmt::Display for PatternError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// Case-insensitive `*`/`?` match.
pub fn matches(mask: &str, name: &str) -> bool {
    let mask: Vec<char> = mask.chars().flat_map(|c| c.to_lowercase()).collect();
    let name: Vec<char> = name.chars().flat_map(|c| c.to_lowercase()).collect();
    let (mut m, mut n) = (0usize, 0usize);
    let (mut star, mut resume) = (None, 0usize);
    while n < name.len() {
        if m < mask.len() && (mask[m] == '?' || mask[m] == name[n]) {
            m += 1;
            n += 1;
        } else if m < mask.len() && mask[m] == '*' {
            star = Some(m);
            resume = n;
            m += 1;
        } else if let Some(s) = star {
            m = s + 1;
            resume += 1;
            n = resume;
        } else {
            return false
        }
    }
    while m < mask.len() && mask[m] == '*' {
        m += 1;
    }
    m == mask.len()
}

/// Lists the entries of the pattern's directory whose names match its last component.
pub fn glob_case_insensitive(pattern: &str) -> Result<std::vec::IntoIter<Result<PathBuf, PatternError>>, PatternError> {
    let normalized = pattern.replace('\\', "/");
    let (dir, mask) = match normalized.rsplit_once('/') {
        Some((dir, mask)) => (if dir.is_empty() { "/" } else { dir }, mask),
        None => (".", normalized.as_str()),
    };
    if dir.contains('*') || dir.contains('?') {
        return Err(PatternError("wildcards are only supported in the last path component".into()))
    }
    let mut found = Vec::new();
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let name = entry.file_name();
            if let Some(name) = name.to_str() {
                if matches(mask, name) {
                    found.push(Ok(PathBuf::from(if normalized.contains('/') {
                        format!("{}/{}", dir.trim_end_matches('/'), name)
                    } else {
                        name.to_string()
                    })));
                }
            }
        }
    }
    found.sort_by(|a, b| match (a, b) {
        (Ok(a), Ok(b)) => a.cmp(b),
        _ => std::cmp::Ordering::Equal,
    });
    Ok(found.into_iter())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wildcard_matching() {
        assert!(matches("*.png", "Hero.PNG"));
        assert!(matches("sav?.dat", "save.dat"));
        assert!(matches("*", "anything"));
        assert!(matches("a*b*c", "aXXbYYc"));
        assert!(!matches("*.png", "hero.bmp"));
        assert!(!matches("sav?.dat", "sa.dat"));
        assert!(matches("", ""));
        assert!(!matches("", "x"));
    }
}
