//! Streaming checksums — O(1) memory.

use crate::errors::{DownloadError, Result};
use md5::{Digest as Md5Digest, Md5};
use sha1::{Digest as Sha1Digest, Sha1};
use sha2::{Digest, Sha256, Sha512};
use std::fs::File;
use std::io::{BufReader, Read};
use std::path::Path;

#[derive(Debug, Clone, Copy)]
pub enum HashAlgo {
    Sha256,
    Sha512,
    Sha1,
    Md5,
}

impl HashAlgo {
    pub fn from_name(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "sha256" | "sha-256" => Some(HashAlgo::Sha256),
            "sha512" | "sha-512" => Some(HashAlgo::Sha512),
            "sha1" | "sha-1" => Some(HashAlgo::Sha1),
            "md5" => Some(HashAlgo::Md5),
            _ => None,
        }
    }
}

pub fn hash_file(path: &Path, algo: HashAlgo) -> Result<String> {
    let file = File::open(path)?;
    let mut reader = BufReader::with_capacity(1024 * 1024, file);
    let mut buf = vec![0u8; 1024 * 1024];

    match algo {
        HashAlgo::Sha256 => {
            let mut h = Sha256::new();
            loop {
                let n = reader.read(&mut buf)?;
                if n == 0 {
                    break;
                }
                h.update(&buf[..n]);
            }
            Ok(hex::encode(h.finalize()))
        }
        HashAlgo::Sha512 => {
            let mut h = Sha512::new();
            loop {
                let n = reader.read(&mut buf)?;
                if n == 0 {
                    break;
                }
                h.update(&buf[..n]);
            }
            Ok(hex::encode(h.finalize()))
        }
        HashAlgo::Sha1 => {
            let mut h = Sha1::new();
            loop {
                let n = reader.read(&mut buf)?;
                if n == 0 {
                    break;
                }
                h.update(&buf[..n]);
            }
            Ok(hex::encode(h.finalize()))
        }
        HashAlgo::Md5 => {
            let mut h = Md5::new();
            loop {
                let n = reader.read(&mut buf)?;
                if n == 0 {
                    break;
                }
                h.update(&buf[..n]);
            }
            Ok(hex::encode(h.finalize()))
        }
    }
}

pub fn verify_file(path: &Path, algo: HashAlgo, expected: &str) -> Result<()> {
    let actual = hash_file(path, algo)?;
    let expected = expected.trim().to_lowercase();
    let actual_l = actual.to_lowercase();
    if actual_l != expected {
        return Err(DownloadError::ChecksumMismatch {
            expected: expected.to_string(),
            actual: actual_l,
        });
    }
    Ok(())
}
