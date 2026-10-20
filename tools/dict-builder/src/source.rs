//! The pinned ECDICT source file: download and verification (D038).

use std::fs::{self, File};
use std::io::{self, BufReader, Read};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use sha2::{Digest, Sha256};

/// The pinned source. Changing any of these is a dictionary update: rebuild, compare and
/// note it in `docs/specs/m1-dict.md`.
pub struct Source {
    pub commit: &'static str,
    pub url: &'static str,
    pub size: u64,
    pub sha256: &'static str,
}

pub const ECDICT: Source = Source {
    commit: "82c9872576b23118d7c42e920c11beb77f510ae2",
    url: "https://raw.githubusercontent.com/skywind3000/ECDICT/82c9872576b23118d7c42e920c11beb77f510ae2/ecdict.csv",
    size: 65_933_428,
    sha256: "1a6947e04785db63613a92e14903cdae7954f7e84860b10e68e5c7cbb3f9c3cf",
};

impl Source {
    /// Local file name, unique per pinned commit.
    pub fn file_name(&self) -> String {
        format!("ecdict-{}.csv", &self.commit[..7])
    }
}

/// Returns the path of the verified source file in `raw_dir`, downloading it first unless
/// `offline`. A file that fails verification is deleted.
pub fn ensure(source: &Source, raw_dir: &Path, offline: bool) -> Result<PathBuf> {
    let path = raw_dir.join(source.file_name());
    if path.exists() {
        match verify(source, &path) {
            Ok(()) => return Ok(path),
            Err(e) => {
                eprintln!(
                    "existing {} is invalid ({e:#}); removing it",
                    path.display()
                );
                fs::remove_file(&path)?;
            }
        }
    }
    if offline {
        bail!("{} is missing and --offline was given", path.display());
    }
    fs::create_dir_all(raw_dir).with_context(|| format!("creating {}", raw_dir.display()))?;
    eprintln!("downloading {}", source.url);
    let tmp = path.with_extension("csv.part");
    let result = download(source.url, &tmp).and_then(|()| verify(source, &tmp));
    if let Err(e) = result {
        let _ = fs::remove_file(&tmp);
        return Err(e);
    }
    fs::rename(&tmp, &path)?;
    Ok(path)
}

fn download(url: &str, to: &Path) -> Result<()> {
    let response = ureq::get(url)
        .call()
        .with_context(|| format!("GET {url}"))?;
    let mut body = response.into_body().into_reader();
    let mut file = File::create(to).with_context(|| format!("creating {}", to.display()))?;
    io::copy(&mut body, &mut file).context("downloading")?;
    file.sync_all()?;
    Ok(())
}

/// Checks the file's size and SHA-256 against the pinned values.
pub fn verify(source: &Source, path: &Path) -> Result<()> {
    let size = fs::metadata(path)?.len();
    if size != source.size {
        bail!("size is {size} bytes, expected {}", source.size);
    }
    let hash = sha256_file(path)?;
    if hash != source.sha256 {
        bail!("SHA-256 is {hash}, expected {}", source.sha256);
    }
    Ok(())
}

pub fn sha256_file(path: &Path) -> Result<String> {
    let mut reader = BufReader::new(File::open(path)?);
    let mut hasher = Sha256::new();
    let mut buf = vec![0; 1 << 16];
    loop {
        let n = reader.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    // SHA-256 of "abc".
    const ABC: &str = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";

    fn source(size: u64, sha256: &'static str) -> Source {
        Source {
            commit: "0123456789abcdef",
            url: "http://unused.invalid/",
            size,
            sha256,
        }
    }

    fn write_abc(dir: &Path, name: &str) -> PathBuf {
        let path = dir.join(name);
        File::create(&path).unwrap().write_all(b"abc").unwrap();
        path
    }

    #[test]
    fn hashes_files() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(sha256_file(&write_abc(dir.path(), "a")).unwrap(), ABC);
    }

    #[test]
    fn verify_checks_size_and_hash() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_abc(dir.path(), "a");
        assert!(verify(&source(3, ABC), &path).is_ok());
        assert!(verify(&source(4, ABC), &path).is_err());
        let wrong = "0000000000000000000000000000000000000000000000000000000000000000";
        assert!(verify(&source(3, wrong), &path).is_err());
    }

    #[test]
    fn valid_local_file_is_used_offline() {
        let dir = tempfile::tempdir().unwrap();
        let src = source(3, ABC);
        let path = write_abc(dir.path(), &src.file_name());
        assert_eq!(ensure(&src, dir.path(), true).unwrap(), path);
    }

    #[test]
    fn invalid_local_file_is_removed() {
        let dir = tempfile::tempdir().unwrap();
        let src = source(4, ABC);
        let path = write_abc(dir.path(), &src.file_name());
        assert!(ensure(&src, dir.path(), true).is_err());
        assert!(!path.exists());
    }

    #[test]
    fn pinned_source_file_name() {
        assert_eq!(ECDICT.file_name(), "ecdict-82c9872.csv");
        assert!(ECDICT.url.contains(ECDICT.commit));
    }
}
