//! Whether the Oxford dictionary in a folder can be used, for the settings window
//! (`docs/specs/m1.5-oxford.md` 2.3).

use std::path::Path;

use super::reader::ExtDictionary;
use super::{OXFORD_FILE_NAME, OXFORD_SOURCE};
use crate::DictError;

/// What an extended dictionary's `meta` says about it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtInfo {
    /// `meta.source_title`, e.g. "牛津高阶英汉双解词典（第 10 版）".
    pub title: String,
    pub entries: u64,
    /// `meta.built_at`, `YYYY-MM-DDTHH:MM:SSZ`.
    pub built_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExtStatus {
    FolderMissing,
    /// The folder has no `oalecd10.db`.
    FileMissing,
    /// The file cannot be used; the reason in one Chinese sentence.
    Unusable(String),
    Ready(ExtInfo),
}

impl ExtDictionary {
    /// Opens the Oxford dictionary: an extended dictionary whose `meta.source` is
    /// `oalecd10`.
    pub fn open_oxford(path: &Path) -> Result<Self, DictError> {
        let dict = Self::open(path)?;
        if dict.source() != OXFORD_SOURCE {
            return Err(DictError::UnsupportedFormat {
                found: Some(format!("source {}", dict.source())),
            });
        }
        Ok(dict)
    }

    pub fn info(&self) -> Result<ExtInfo, DictError> {
        Ok(ExtInfo {
            title: self.meta("source_title")?.unwrap_or_default(),
            entries: self
                .meta("entry_count")?
                .and_then(|n| n.parse().ok())
                .unwrap_or(0),
            built_at: self.meta("built_at")?.unwrap_or_default(),
        })
    }
}

/// The Oxford dictionary's status in `folder`. Opens the file to check it and closes it
/// again.
pub fn oxford_status(folder: &Path) -> ExtStatus {
    if !folder.is_dir() {
        return ExtStatus::FolderMissing;
    }
    let path = folder.join(OXFORD_FILE_NAME);
    if !path.is_file() {
        return ExtStatus::FileMissing;
    }
    match ExtDictionary::open_oxford(&path).and_then(|d| d.info()) {
        Ok(info) => ExtStatus::Ready(info),
        Err(e) => ExtStatus::Unusable(unusable_reason(&e)),
    }
}

/// Why a file that is there cannot be used, in one sentence for the settings window.
pub fn unusable_reason(error: &DictError) -> String {
    match error {
        DictError::NotFound(_) => "文件不存在".to_owned(),
        DictError::UnsupportedFormat { .. } => {
            "不是 LexTrail 的牛津词典文件，或由这个版本不认识的转换工具生成".to_owned()
        }
        DictError::Sqlite(_) | DictError::Data(_) => format!("文件无法读取（{error}）"),
    }
}

#[cfg(test)]
mod tests {
    use rusqlite::Connection;

    use super::*;
    use crate::ext::schema::{FORMAT, FORMAT_VERSION, SCHEMA};

    fn write(path: &Path, source: &str) {
        let conn = Connection::open(path).unwrap();
        conn.execute_batch(SCHEMA).unwrap();
        conn.execute_batch(&format!(
            "INSERT INTO meta VALUES ('format', '{FORMAT}'), ('format_version', '{FORMAT_VERSION}'),
             ('source', '{source}'), ('source_title', '牛津测试词典'), ('entry_count', '3'),
             ('built_at', '2026-10-22T08:33:03Z');"
        ))
        .unwrap();
    }

    #[test]
    fn each_status() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(
            oxford_status(&dir.path().join("none")),
            ExtStatus::FolderMissing
        );
        assert_eq!(oxford_status(dir.path()), ExtStatus::FileMissing);

        let path = dir.path().join(OXFORD_FILE_NAME);
        std::fs::write(&path, b"not a database").unwrap();
        let ExtStatus::Unusable(why) = oxford_status(dir.path()) else {
            panic!("unusable");
        };
        assert!(why.contains("不是"), "{why}");

        std::fs::remove_file(&path).unwrap();
        write(&path, "other");
        let ExtStatus::Unusable(why) = oxford_status(dir.path()) else {
            panic!("unusable");
        };
        assert!(why.contains("不是"), "{why}");

        std::fs::remove_file(&path).unwrap();
        write(&path, OXFORD_SOURCE);
        assert_eq!(
            oxford_status(dir.path()),
            ExtStatus::Ready(ExtInfo {
                title: "牛津测试词典".into(),
                entries: 3,
                built_at: "2026-10-22T08:33:03Z".into(),
            })
        );
    }
}
