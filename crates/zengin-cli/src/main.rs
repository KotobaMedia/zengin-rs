use std::{
    fs::File,
    io::{self, Read, Write},
    path::{Path, PathBuf},
    process::ExitCode,
};

use clap::{Parser, ValueEnum};
use serde::Serialize;
use zengin_rs::{FileType, ParsedFile, parse_as};

const MAX_INPUT_BYTES: u64 = 10 * 1024 * 1024;

#[derive(Debug, Parser)]
#[command(
    name = "zengin",
    about = "Parse Zengin fixed-width files and write JSON to stdout"
)]
struct Args {
    #[arg(value_name = "input-file")]
    path: PathBuf,

    #[arg(short = 't', long = "type", value_enum, default_value_t = CliFileType::Auto)]
    file_type: CliFileType,

    #[arg(long)]
    metadata_only: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum CliFileType {
    Auto,
    GeneralTransfer,
    PayrollTransfer,
    #[value(alias = "account-transfer")]
    Request,
    #[value(alias = "account-transfer-result")]
    Result,
    TransferAccountInquiry,
    PaymentNotice,
}

impl From<CliFileType> for FileType {
    fn from(file_type: CliFileType) -> Self {
        match file_type {
            CliFileType::Auto => Self::Auto,
            CliFileType::GeneralTransfer => Self::GeneralTransfer,
            CliFileType::PayrollTransfer => Self::PayrollTransfer,
            CliFileType::Request => Self::AccountTransfer,
            CliFileType::Result => Self::AccountTransferResult,
            CliFileType::TransferAccountInquiry => Self::TransferAccountInquiry,
            CliFileType::PaymentNotice => Self::PaymentNotice,
        }
    }
}

#[derive(Serialize)]
struct MetadataOnly<'a, H, T> {
    header: &'a H,
    trailer: &'a T,
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();

    let input = read_limited(&args.path)?;
    let file: ParsedFile = parse_as(&input, args.file_type.into())?;

    let stdout = io::stdout();
    let mut stdout = stdout.lock();
    write_json(&mut stdout, &file, args.metadata_only)?;
    stdout.write_all(b"\n")?;
    Ok(())
}

fn read_limited(path: &Path) -> Result<Vec<u8>, io::Error> {
    let file = File::open(path)?;
    let mut input = Vec::new();
    file.take(MAX_INPUT_BYTES + 1).read_to_end(&mut input)?;

    if input.len() as u64 > MAX_INPUT_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("input file exceeds the 10 MiB limit ({MAX_INPUT_BYTES} bytes)"),
        ));
    }

    Ok(input)
}

fn write_json<W>(writer: W, file: &ParsedFile, metadata_only: bool) -> Result<(), serde_json::Error>
where
    W: Write,
{
    if metadata_only {
        match file {
            ParsedFile::GeneralTransfer(file) => serde_json::to_writer_pretty(
                writer,
                &MetadataOnly {
                    header: &file.header,
                    trailer: &file.trailer,
                },
            ),
            ParsedFile::PayrollTransfer(file) => serde_json::to_writer_pretty(
                writer,
                &MetadataOnly {
                    header: &file.header,
                    trailer: &file.trailer,
                },
            ),
            ParsedFile::AccountTransfer(file) => serde_json::to_writer_pretty(
                writer,
                &MetadataOnly {
                    header: &file.header,
                    trailer: &file.trailer,
                },
            ),
            ParsedFile::AccountTransferResult(file) => serde_json::to_writer_pretty(
                writer,
                &MetadataOnly {
                    header: &file.header,
                    trailer: &file.trailer,
                },
            ),
            ParsedFile::TransferAccountInquiry(file) => serde_json::to_writer_pretty(
                writer,
                &MetadataOnly {
                    header: &file.header,
                    trailer: &file.trailer,
                },
            ),
            ParsedFile::PaymentNotice(file) => serde_json::to_writer_pretty(
                writer,
                &MetadataOnly {
                    header: &file.header,
                    trailer: &file.trailer,
                },
            ),
        }
    } else {
        serde_json::to_writer_pretty(writer, file)
    }
}
