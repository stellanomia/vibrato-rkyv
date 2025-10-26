use std::{fs::File, io};
use std::path::PathBuf;

use vibrato_rkyv::{dictionary::{DictionaryInner, SystemDictionaryBuilder}, errors::VibratoError};

use clap::Parser;

#[derive(Parser, Debug)]
#[clap(
    name = "build",
    about = "A program to build the system dictionary."
)]
pub struct Args {
    /// System lexicon file (lex.csv).
    #[clap(short = 'l', long)]
    lexicon_in: PathBuf,

    /// Matrix definition file (matrix.def).
    ///
    /// If this argument is not specified, the compiler considers `--bigram-right-in`,
    /// `--bigram-left-in`, and `--bigram-cost-in` arguments.
    #[clap(short = 'm', long)]
    matrix_in: Option<PathBuf>,

    /// Unknown word definition file (unk.def).
    #[clap(short = 'u', long)]
    unk_in: PathBuf,

    /// Character definition file (char.def).
    #[clap(short = 'c', long)]
    char_in: PathBuf,

    /// File to which the binary dictionary is output (in zstd).
    #[clap(short = 'o', long)]
    sysdic_out: PathBuf,

    /// Bi-gram information associated with right connection IDs (bigram.right).
    #[clap(long)]
    bigram_right_in: Option<PathBuf>,

    /// Bi-gram information associated with left connection IDs (bigram.left).
    #[clap(long)]
    bigram_left_in: Option<PathBuf>,

    /// Bi-gram cost file (bigram.cost).
    #[clap(long)]
    bigram_cost_in: Option<PathBuf>,

    /// Option to control trade-off between speed and memory.
    /// When setting it, the resulting model will be faster but larger.
    /// This option is enabled when bi-gram information is specified.
    #[clap(long)]
    dual_connector: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum BuildError {
    #[error(
        "Invalid argument combination: Either --matrix-in or all of \
        --bigram-{{right,left,cost}}-in must be specified."
    )]
    InvalidSourceArguments,

    #[error("I/O error: {0}")]
    Io(#[from] io::Error),

    #[error("Dictionary building failed: {0}")]
    Vibrato(#[from] VibratoError),
}

fn get_source_from_args(args: &Args) -> Result<BuildSource, BuildError> {
    if let Some(matrix_in) = &args.matrix_in {
        Ok(BuildSource::FromMatrix {
            lexicon: args.lexicon_in.clone(),
            matrix: matrix_in.clone(),
            char_def: args.char_in.clone(),
            unk_def: args.unk_in.clone(),
        })
    } else if let (Some(bigram_right_in), Some(bigram_left_in), Some(bigram_cost_in)) =
        (&args.bigram_right_in, &args.bigram_left_in, &args.bigram_cost_in)
    {
        Ok(BuildSource::FromBigram {
            lexicon: args.lexicon_in.clone(),
            bigram_right: bigram_right_in.clone(),
            bigram_left: bigram_left_in.clone(),
            bigram_cost: bigram_cost_in.clone(),
            char_def: args.char_in.clone(),
            unk_def: args.unk_in.clone(),
            dual_connector: args.dual_connector,
        })
    } else {
        Err(BuildError::InvalidSourceArguments)
    }
}

pub enum BuildSource {
    /// Build from a matrix.def file.
    FromMatrix {
        lexicon: PathBuf,
        matrix: PathBuf,
        char_def: PathBuf,
        unk_def: PathBuf,
    },
    /// Build from optimized bigram.* files.
    FromBigram {
        lexicon: PathBuf,
        bigram_right: PathBuf,
        bigram_left: PathBuf,
        bigram_cost: PathBuf,
        char_def: PathBuf,
        unk_def: PathBuf,
        dual_connector: bool,
    },
}

pub fn run(args: Args) -> Result<(), BuildError> {
    let source = get_source_from_args(&args)?;

    println!("Compiling the system dictionary...");
    let dict = build_dictionary(&source)?;

    println!("Writing the system dictionary...");
    let file = File::create(&args.sysdic_out)?;
    let mut encoder = zstd::Encoder::new(file, 19)?;
    dict.write(&mut encoder)?;
    encoder.finish()?;

    println!("Successfully built the dictionary to {}", args.sysdic_out.display());
    Ok(())
}

/// Builds a dictionary from the specified source files.
/// This is the core build logic, independent of the CLI.
pub fn build_dictionary(source: &BuildSource) -> Result<DictionaryInner, BuildError> {
    let dict = match source {
        BuildSource::FromMatrix { lexicon, matrix, char_def, unk_def } => {
            SystemDictionaryBuilder::from_readers(
                File::open(lexicon)?,
                File::open(matrix)?,
                File::open(char_def)?,
                File::open(unk_def)?,
            )?
        }
        BuildSource::FromBigram {
            lexicon,
            bigram_right,
            bigram_left,
            bigram_cost,
            char_def,
            unk_def,
            dual_connector,
        } => {
            SystemDictionaryBuilder::from_readers_with_bigram_info(
                File::open(lexicon)?,
                File::open(bigram_right)?,
                File::open(bigram_left)?,
                File::open(bigram_cost)?,
                File::open(char_def)?,
                File::open(unk_def)?,
                *dual_connector,
            )?
        }
    };
    Ok(dict)
}
