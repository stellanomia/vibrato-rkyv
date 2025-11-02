#![cfg(feature = "download")]
use std::{fs::{self, File}, io::{self, Seek, SeekFrom}, path::{Path, PathBuf}};

use sha2::{Digest, Sha256};
use walkdir::WalkDir;
use xz2::read::XzDecoder;

use crate::{dictionary::{PresetDictionaryKind, compute_metadata_hash, config::FileType}, errors::DownloadError};

pub(crate) fn download_dictionary<P: AsRef<Path>>(kind: PresetDictionaryKind, dest: P) -> Result<PathBuf, DownloadError> {
    let meta = kind.meta();
    let dest = dest.as_ref();
    const EXTRACTED_DICT_FILENAME: &str = "system.dic.zst";

    let dict_path = dest.join(EXTRACTED_DICT_FILENAME);

    if dict_path.exists() {
        let dict = File::open(&dict_path)?;
        let dict_meta = dict.metadata()?;
        let dict_hash = compute_metadata_hash(&dict_meta);
        // `from_zstd` decompresses to the ./decompressed directory.
        let decompressed_dir = dest.join("decompressed");

        if let Ok(saved_hash) = fs::read_to_string(decompressed_dir.join("system.dic.zst.rkyv.sha256"))
            && saved_hash == dict_hash {
                return Ok(dict_path);
            }

        if let Ok(saved_hash) = fs::read_to_string(decompressed_dir.join("system.dic.zst.sha256"))
            && saved_hash == dict_hash {
                return Ok(dict_path);
            }

        if let Ok(mut f) = File::open(&dict_path) {
            let mut hasher = Sha256::new();
            io::copy(&mut f, &mut hasher)?;
            let hash = hex::encode(hasher.finalize());
            if hash == meta.sha256_hash_comp_dict {
                return Ok(dict_path);
            }
        }
    }

    fs::create_dir_all(dest)?;

    let archive_path = match meta.file_type {
        FileType::Tar => dest.join(format!("{}.tar", meta.name)),
        FileType::TarXz => dest.join(format!("{}.tar.xz", meta.name)),
    };

    let mut response = reqwest::blocking::get(meta.download_url)?;
    if !response.status().is_success() {
        return Err(DownloadError::HttpStatus(response.status()));
    }

    let mut temp_file = tempfile::NamedTempFile::new_in(dest)?;
    response.copy_to(&mut temp_file)?;

    temp_file.seek(SeekFrom::Start(0))?;
    let calculated_hash = {
        let mut hasher = Sha256::new();
        io::copy(&mut temp_file, &mut hasher)?;
        hex::encode(hasher.finalize())
    };

    if calculated_hash != meta.sha256_hash_archive {
        return Err(DownloadError::HashMismatch);
    }

    let mut archive_file = temp_file.persist(&archive_path)?;
    archive_file.seek(SeekFrom::Start(0))?;

    let mut archive: tar::Archive<Box<dyn io::Read>> = match meta.file_type {
        FileType::Tar => tar::Archive::new(Box::new(archive_file)),
        FileType::TarXz => tar::Archive::new(Box::new(XzDecoder::new(archive_file))),
    };

    archive.unpack(dest)?;

    let found_path = if dict_path.exists() {
        dict_path.clone()
    } else {
        WalkDir::new(dest)
            .into_iter()
            .filter_map(|e| e.ok())
            .find(|e| e.file_name() == EXTRACTED_DICT_FILENAME)
            .map(|e| e.into_path())
            .ok_or(DownloadError::ExtractedFileNotFound)?
    };

    if found_path != dict_path {
        fs::rename(&found_path, &dict_path)?;
    }

    for entry in fs::read_dir(dest)? {
        let path = entry?.path();
        if path.is_dir() {
            fs::remove_dir_all(path)?;
        }
    }

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
