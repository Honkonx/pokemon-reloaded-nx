//! Small INI reader/writer for GML's `ini_*` functions (replaces `rust-ini`, which needs `std::io`).
//! Format: optional section-less keys first, then `[Section]` blocks of `key=value` lines.

use std::{fmt, io, path::Path};

#[derive(Clone, Debug, Default)]
pub struct Properties {
    entries: Vec<(String, String)>,
}

impl Properties {
    pub fn get<K: AsRef<str>>(&self, key: K) -> Option<&str> {
        let key = key.as_ref();
        self.entries.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str())
    }

    pub fn contains_key<K: AsRef<str>>(&self, key: K) -> bool {
        self.get(key).is_some()
    }

    fn set(&mut self, key: String, value: String) {
        match self.entries.iter_mut().find(|(k, _)| *k == key) {
            Some(entry) => entry.1 = value,
            None => self.entries.push((key, value)),
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct Ini {
    /// `None` is the section-less preamble.
    sections: Vec<(Option<String>, Properties)>,
}

#[derive(Debug)]
pub struct Error(io::Error);

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for Error {}

pub struct SectionSetter<'a> {
    props: &'a mut Properties,
}

impl SectionSetter<'_> {
    pub fn set<K: Into<String>, V: Into<String>>(&mut self, key: K, value: V) -> &mut Self {
        self.props.set(key.into(), value.into());
        self
    }
}

impl Ini {
    pub fn new() -> Ini {
        Ini::default()
    }

    pub fn load_from_file<P: AsRef<Path>>(path: P) -> Result<Ini, Error> {
        let bytes = std::fs::read(path).map_err(Error)?;
        Ok(Ini::parse(&String::from_utf8_lossy(&bytes)))
    }

    pub fn parse(text: &str) -> Ini {
        let mut ini = Ini::new();
        let mut current: Option<String> = None;
        for raw in text.lines() {
            let line = raw.trim();
            if line.is_empty() || line.starts_with(';') || line.starts_with('#') {
                continue
            }
            if let Some(name) = line.strip_prefix('[').and_then(|l| l.split_once(']')).map(|(n, _)| n) {
                current = Some(name.trim().to_string());
                ini.props_mut(current.as_deref());
                continue
            }
            if let Some((key, value)) = line.split_once('=') {
                ini.props_mut(current.as_deref()).set(key.trim().to_string(), value.trim().to_string());
            }
        }
        ini
    }

    pub fn write_to_file<P: AsRef<Path>>(&self, path: P) -> Result<(), Error> {
        let path = path.as_ref().to_string_lossy().replace('\\', "/");
        crate::gml::file::write_atomic(&path, self.render().as_bytes()).map_err(Error)
    }

    pub fn render(&self) -> String {
        let mut out = String::new();
        let mut first = true;
        for (name, props) in &self.sections {
            if let Some(name) = name {
                if !first {
                    out.push_str("\r\n");
                }
                out.push_str(&format!("[{}]\r\n", name));
            }
            for (key, value) in &props.entries {
                out.push_str(&format!("{}={}\r\n", key, value));
            }
            first = false;
        }
        out
    }

    fn props_mut(&mut self, section: Option<&str>) -> &mut Properties {
        let index = match self.sections.iter().position(|(n, _)| n.as_deref() == section) {
            Some(index) => index,
            None => {
                self.sections.push((section.map(str::to_string), Properties::default()));
                self.sections.len() - 1
            },
        };
        &mut self.sections[index].1
    }

    pub fn section(&self, section: Option<&str>) -> Option<&Properties> {
        self.sections.iter().find(|(n, _)| n.as_deref() == section).map(|(_, p)| p)
    }

    pub fn with_section(&mut self, section: Option<&str>) -> SectionSetter<'_> {
        SectionSetter { props: self.props_mut(section) }
    }

    pub fn delete_from<K: AsRef<str>>(&mut self, section: Option<&str>, key: K) -> Option<String> {
        let key = key.as_ref();
        let index = self.sections.iter().position(|(n, _)| n.as_deref() == section)?;
        let entries = &mut self.sections[index].1.entries;
        let position = entries.iter().position(|(k, _)| k == key)?;
        Some(entries.remove(position).1)
    }

    pub fn delete(&mut self, section: Option<&str>) -> Option<Properties> {
        let index = self.sections.iter().position(|(n, _)| n.as_deref() == section)?;
        Some(self.sections.remove(index).1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_writes_and_edits() {
        let mut ini = Ini::parse("; c\nvolume=3\n[Video]\nscale = 2\nmode=full\n[Audio]\nmusic=on\n");
        assert_eq!(ini.section(None).unwrap().get("volume"), Some("3"));
        assert_eq!(ini.section(Some("Video")).unwrap().get("scale"), Some("2"));
        ini.with_section(Some("Video")).set("scale", "3").set("vsync", "1");
        assert_eq!(ini.section(Some("Video")).unwrap().get("scale"), Some("3"));
        assert_eq!(ini.delete_from(Some("Audio"), "music").as_deref(), Some("on"));
        let reparsed = Ini::parse(&ini.render());
        assert_eq!(reparsed.section(Some("Video")).unwrap().get("vsync"), Some("1"));
        assert!(reparsed.section(Some("Audio")).unwrap().get("music").is_none());
        assert!(reparsed.section(Some("Nope")).is_none());
    }
}
