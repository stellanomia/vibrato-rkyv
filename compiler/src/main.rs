mod build;
mod dictgen;
mod full_build;
mod train;
mod transmute_legacy;

use clap::Parser;
use thiserror::Error;

use crate::{build::BuildError, dictgen::DictgenError, full_build::FullBuildError, train::TrainError, transmute_legacy::TransmuteLegacyError};


#[derive(Parser, Debug)]
#[clap(name = "compile", version)]
struct Cli {
    #[clap(subcommand)]
    command: Command,
}

#[derive(Parser, Debug)]
enum Command {
    /// Builds a dictionary from a corpus in one step.
    FullBuild(full_build::Args),

    /// Train a model from a corpus.
    Train(train::Args),

    /// Generate dictionary files from a model.
    Dictgen(dictgen::Args),

    /// Build the binary dictionary from source files.
    Build(build::Args),

    /// Convert a legacy vibrato dictionary from bincode format to rkyv format.
    Transmute(transmute_legacy::Args),
}

#[derive(Debug, Error)]
pub enum CompileError {
    #[error(transparent)]
    FullBuildError(#[from] FullBuildError),
    #[error(transparent)]
    TrainError(#[from] TrainError),
    #[error(transparent)]
    DictgenError(#[from] DictgenError),
    #[error(transparent)]
    BuildError(#[from] BuildError),
    #[error(transparent)]
    TransmuteLegacy(#[from] TransmuteLegacyError),
}

fn main() -> Result<(), CompileError> {
    let cli = Cli::parse();
    match cli.command {
        Command::FullBuild(args) => Ok(full_build::run(args)?),
        Command::Train(args) => Ok(train::run(args)?),
        Command::Dictgen(args) => Ok(dictgen::run(args)?),
        Command::Build(args) => Ok(build::run(args)?),
        Command::Transmute(args) => Ok(transmute_legacy::run(args)?),
    }
}
