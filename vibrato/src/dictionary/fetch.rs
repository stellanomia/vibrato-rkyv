#![cfg(feature = "download")]
use std::{fs::{self, File}, io::{self, Seek, SeekFrom}, path::{Path, PathBuf}};

use sha2::{Digest, Sha256};
use tempfile::tempdir_in;
use walkdir::WalkDir;
use xz2::read::XzDecoder;

use crate::{dictionary::{PresetDictionaryKind, compute_metadata_hash, config::FileType}, errors::DownloadError};

pub(crate) fn download_dictionary<P: AsRef<Path>>(kind: PresetDictionaryKind, dest_dir: P) -> Result<PathBuf, DownloadError> {
    let preset_meta = kind.meta();
    let dest_dir = dest_dir.as_ref();

    let dict_path = dest_dir
        .join(format!("{}.dic.zst", preset_meta.sha256_hash_comp_dict));

    if dict_path.exists()
        && fs::exists(Path::new(&dest_dir.join(format!("{}.sha256", compute_metadata_hash(&fs::metadata(&dict_path)?)))))? {
            return Ok(dict_path);
        }

    fs::create_dir_all(dest_dir)?;

    let archive_path = match preset_meta.file_type {
        FileType::Tar => dest_dir.join(format!("{}.tar", preset_meta.name)),
        FileType::TarXz => dest_dir.join(format!("{}.tar.xz", preset_meta.name)),
    };

    let mut response = reqwest::blocking::get(preset_meta.download_url)?;
    if !response.status().is_success() {
        return Err(DownloadError::HttpStatus(response.status()));
    }

    let mut temp_file = tempfile::NamedTempFile::new_in(dest_dir)?;
    response.copy_to(&mut temp_file)?;

    temp_file.seek(SeekFrom::Start(0))?;
    let calculated_hash = {
        let mut hasher = Sha256::new();
        io::copy(&mut temp_file, &mut hasher)?;
        hex::encode(hasher.finalize())
    };

    if calculated_hash != preset_meta.sha256_hash_archive {
        return Err(DownloadError::HashMismatch);
    }

    let mut archive_file = temp_file.persist(&archive_path)?;
    archive_file.seek(SeekFrom::Start(0))?;

    let mut archive: tar::Archive<Box<dyn io::Read>> = match preset_meta.file_type {
        FileType::Tar => tar::Archive::new(Box::new(archive_file)),
        FileType::TarXz => tar::Archive::new(Box::new(XzDecoder::new(archive_file))),
    };

    let temp_unpack_dir = tempdir_in(dest_dir)?;

    archive.unpack(&temp_unpack_dir)?;

    let found_path = WalkDir::new(&temp_unpack_dir)
        .into_iter()
        .filter_map(|e| e.ok())
        .find(|e| e.file_name() == "system.dic.zst")
        .map(|e| e.into_path())
        .ok_or(DownloadError::ExtractedFileNotFound)?;

    fs::rename(&found_path, &dict_path)?;

    fs::remove_file(&archive_path)?;

    let mut f = File::open(&dict_path)?;
    let metadata = f.metadata()?;
    let mut hasher = Sha256::new();
    io::copy(&mut f, &mut hasher)?;
    let hash = hex::encode(hasher.finalize());

    if hash != preset_meta.sha256_hash_comp_dict {
        return Err(DownloadError::ExtractedHashMismatch);
    }

    if let Ok(entries) = fs::read_dir(dest_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_file()
                && path.extension().is_some_and(|ext| ext == "sha256")
            {
                let _ = fs::remove_file(path);
            }
        }
    }

    let metadata_hash = compute_metadata_hash(&metadata);
    let metadata_hash_path = dest_dir.join(format!("{metadata_hash}.sha256"));
    File::create(metadata_hash_path)?;

    Ok(dict_path)
}
