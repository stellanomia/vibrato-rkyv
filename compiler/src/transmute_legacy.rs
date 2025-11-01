use std::path::{Path, PathBuf};
use std::fs::File;
use std::io::{self, BufReader, BufWriter, Read, Write};

use clap::Parser;
use tempfile::NamedTempFile;
use vibrato_rkyv::Dictionary;
use xz2::bufread::XzDecoder;

use crate::{build::BuildError, dictgen::DictgenError, train::TrainError};


#[derive(Parser, Debug)]
#[clap(
    name = "transmute-lagacy",
    about = "Convert a legacy vibrato dictionary from bincode format to rkyv format."
)]
pub struct Args {
    /// Path to the source legacy (bincode) dictionary file.
    #[clap(value_name = "INPUT")]
    pub input: PathBuf,

    /// Directory to which the dictionary files are output.
    #[clap(short = 'o', long)]
    out_dir: PathBuf,
}

#[derive(Debug, thiserror::Error)]
pub enum TransmuteLegacyError {
    #[error(transparent)]
    Train(#[from] TrainError),
    #[error(transparent)]
    Dictgen(#[from] DictgenError),
    #[error(transparent)]
    Build(#[from] BuildError),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    VibratoRkyv(#[from] vibrato_rkyv::errors::VibratoError),

    #[error("Unsupported file extension: {0:?}. Only '.dic', '.dic.zst', 'tar.xz', 'tar.gz' are supported.")]
    UnsupportedExtension(Option<String>),

    #[error("Dictionary file not found in the tar archive")]
    DictNotFoundInTar,

    #[error("Output path is not a directory: {0}")]
    PathNotDirectory(PathBuf),
}


pub fn run(args: Args) -> Result<(), TransmuteLegacyError> {
    let bincode_path = args.input;
    if !args.out_dir.exists() {
        println!("Creating output directory: {}", args.out_dir.display());
        std::fs::create_dir_all(&args.out_dir)?;
    }
    if !args.out_dir.is_dir() {
        return Err(TransmuteLegacyError::PathNotDirectory(args.out_dir));
    }

    let reader = get_reader(&bincode_path)?;
    let dictionary = unsafe { Dictionary::from_legacy_reader(reader)? };

    let out_path = args.out_dir.join("system.dic");
    println!("Writing rkyv dictionary to: {}", out_path.display());

    let mut writer = BufWriter::new(File::create(&out_path)?);
    dictionary.write(&mut writer)?;

    writer.flush()?;

    let compressed_out_path = args.out_dir.join("system.dic.zst");
    println!("Compressing dictionary with zstd to: {}", compressed_out_path.display());

    let dict_file = File::open(&out_path)?;
    let mut reader = BufReader::new(dict_file);

    let compressed_file = File::create(&compressed_out_path)?;
    let mut encoder = zstd::Encoder::new(compressed_file, 19)?;

    io::copy(&mut reader, &mut encoder)?;
    encoder.finish()?;

    println!("\nSuccessfully converted and created dictionaries at:");
    println!("{}", out_path.display());

    Ok(())
}

fn get_reader(path: &Path) -> Result<Box<dyn Read>, TransmuteLegacyError> {
    let file = File::open(path)?;

    let extension = path.extension()
        .and_then(|s| s.to_str())
        .map(|s| s.to_lowercase());

    match extension.as_deref() {
        Some("zst") => {
            println!("Detected .zst extension, decompressing...");
            let decoder = zstd::Decoder::new(file)?;
            Ok(Box::new(BufReader::new(decoder)))
        }
        Some("dic") => {
            println!("Detected .dic extension.");
            Ok(Box::new(BufReader::new(file)))
        }
        Some("gz") => {
            println!("Detected .tar.gz extension, extracting dictionary to a temporary file...");
            let file = File::open(path)?;
            let tar_gz_reader = BufReader::new(file);
            let tar_reader = flate2::read::GzDecoder::new(tar_gz_reader);
            let mut archive = tar::Archive::new(tar_reader);

            for entry_result in archive.entries()? {
                let mut entry = entry_result?;
                let entry_path = entry.path()?;

                if let Some(name) = entry_path.file_name().map(|s| s.to_string_lossy().to_string())
                    && (name.ends_with(".dic") || name.ends_with(".dic.zst")) {
                        let mut temp_file = NamedTempFile::new()?;
                        println!("Found {} in archive, extracting to {}", name, temp_file.path().display());

                        io::copy(&mut entry, temp_file.as_file_mut())?;

                        let reopened_file = temp_file.reopen()?;

                        if name.ends_with(".dic.zst") {
                             let decoder = zstd::Decoder::new(reopened_file)?;
                             return Ok(Box::new(BufReader::new(decoder)));
                        } else {
                             return Ok(Box::new(BufReader::new(reopened_file)));
                        }
                    }
            }
            Err(TransmuteLegacyError::DictNotFoundInTar)
        }
        Some("xz") => {
            println!("Detected .tar.xz extension, extracting dictionary to a temporary file...");
            let file = File::open(path)?;
            let tar_gz_reader = BufReader::new(file);
            let tar_reader = XzDecoder::new(tar_gz_reader);
            let mut archive = tar::Archive::new(tar_reader);

            for entry_result in archive.entries()? {
                let mut entry = entry_result?;
                let entry_path = entry.path()?;

                if let Some(name) = entry_path.file_name().map(|s| s.to_string_lossy().to_string())
                    && (name.ends_with(".dic") || name.ends_with(".dic.zst")) {
                        let mut temp_file = NamedTempFile::new()?;
                        println!("Found {} in archive, extracting to {}", name, temp_file.path().display());

                        io::copy(&mut entry, temp_file.as_file_mut())?;

                        let reopened_file = temp_file.reopen()?;

                        if name.ends_with(".dic.zst") {
                             let decoder = zstd::Decoder::new(reopened_file)?;
                             return Ok(Box::new(BufReader::new(decoder)));
                        } else {
                             return Ok(Box::new(BufReader::new(reopened_file)));
                        }
                    }
            }
            Err(TransmuteLegacyError::DictNotFoundInTar)
        }
        _ => {
            let ext_str = extension.unwrap_or_else(|| "None".to_string());
            Err(TransmuteLegacyError::UnsupportedExtension(Some(ext_str)))
        }
    }
}
