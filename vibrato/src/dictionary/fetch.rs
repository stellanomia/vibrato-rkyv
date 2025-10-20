#![cfg(feature = "download")]
use std::{fs::{self, File}, io, path::{Path, PathBuf}};

use sha2::{Digest, Sha256};

use crate::{dictionary::PresetDictionaryKind, errors::DownloadError};

pub(crate) fn download_dictionary<P: AsRef<Path>>(kind: PresetDictionaryKind, dest: P) -> Result<PathBuf, DownloadError> {
    let meta = kind.meta();
    let dest = dest.as_ref();
    const EXTRACTED_DICT_FILENAME: &str = "system.dic.zst";

    let dict_path = dest.join(EXTRACTED_DICT_FILENAME);

    if dict_path.exists()
        && let Ok(mut f) = File::open(&dict_path) {
            let mut hasher = Sha256::new();
            io::copy(&mut f, &mut hasher)?;
            let hash = hex::encode(hasher.finalize());
            if hash == meta.sha256_hash_comp_dict {
                return Ok(dict_path);
            }
        }

    fs::create_dir_all(dest)?;

    let archive_path = dest.join(format!("{}.tar", meta.name));
    let temp_path = dest.join(format!("{}.tar.part", meta.name));

    let mut response = reqwest::blocking::get(meta.download_url)?;
    if !response.status().is_success() {
        return Err(DownloadError::HttpStatus(response.status()));
    }

    let mut temp_file = File::create(&temp_path)?;
    response.copy_to(&mut temp_file)?;

    let calculated_hash = {
        let mut f = File::open(&temp_path)?;
        let mut hasher = Sha256::new();
        io::copy(&mut f, &mut hasher)?;
        hex::encode(hasher.finalize())
    };

    if calculated_hash != meta.sha256_hash_archive {
        fs::remove_file(&temp_path)?;
        return Err(DownloadError::HashMismatch);
    }

    fs::rename(&temp_path, &archive_path)?;

    let archive_file = File::open(&archive_path)?;
    let mut archive = tar::Archive::new(archive_file);
    archive.unpack(dest)?;

    fs::remove_file(&archive_path)?;

    let mut f = File::open(&dict_path)?;
    let mut hasher = Sha256::new();
    io::copy(&mut f, &mut hasher)?;
    let hash = hex::encode(hasher.finalize());

    if hash != meta.sha256_hash_comp_dict {
        return Err(DownloadError::ExtractedHashMismatch);
    }

    Ok(dict_path)
}
