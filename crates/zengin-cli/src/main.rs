use std::{
    env,
    ffi::{OsStr, OsString},
    fs::File,
    io::{self, Read, Write},
    path::{Path, PathBuf},
    process::ExitCode,
};

use serde::Serialize;
use zengin_rs::{FileType, ParsedFile, parse_as};

const MAX_INPUT_BYTES: u64 = 10 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
struct Args {
    path: PathBuf,
    file_type: FileType,
    metadata_only: bool,
}

#[derive(Serialize)]
struct MetadataOnly<'a, H, T> {
    header: &'a H,
    trailer: &'a T,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ParsedArgs {
    Help { program: OsString },
    Run(Args),
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
    let args = match parse_args(env::args_os())? {
        ParsedArgs::Help { program } => {
            print_usage(&program);
            return Ok(());
        }
        ParsedArgs::Run(args) => args,
    };

    let input = read_limited(&args.path)?;
    let file: ParsedFile = parse_as(&input, args.file_type)?;

    let stdout = io::stdout();
    let mut stdout = stdout.lock();
    write_json(&mut stdout, &file, args.metadata_only)?;
    stdout.write_all(b"\n")?;
    Ok(())
}

fn parse_args(args: impl IntoIterator<Item = OsString>) -> Result<ParsedArgs, io::Error> {
    let mut args = args.into_iter();
    let program = args.next().unwrap_or_else(|| OsString::from("zengin"));
    let mut file_type = FileType::Auto;
    let mut metadata_only = false;
    let mut path = None;

    while let Some(arg) = args.next() {
        if arg == "--help" || arg == "-h" {
            return Ok(ParsedArgs::Help { program });
        }

        if arg == "--metadata-only" {
            metadata_only = true;
            continue;
        }

        if arg == "--type" || arg == "-t" {
            let value = args
                .next()
                .ok_or_else(|| usage_error(&program, "--type requires auto, request, or result"))?;
            file_type = parse_file_type(&program, &value)?;
            continue;
        }

        if let Some(value) = arg.to_str().and_then(|arg| arg.strip_prefix("--type=")) {
            file_type = parse_file_type_str(&program, value)?;
            continue;
        }

        if path.replace(PathBuf::from(arg)).is_some() {
            return Err(usage_error(&program, "expected exactly one input file"));
        }
    }

    let path = path.ok_or_else(|| usage_error(&program, "missing input file"))?;
    Ok(ParsedArgs::Run(Args {
        path,
        file_type,
        metadata_only,
    }))
}

fn parse_file_type(program: &OsString, value: &OsStr) -> Result<FileType, io::Error> {
    let value = value
        .to_str()
        .ok_or_else(|| usage_error(program, "file type must be valid UTF-8"))?;
    parse_file_type_str(program, value)
}

fn parse_file_type_str(program: &OsString, value: &str) -> Result<FileType, io::Error> {
    match value {
        "auto" => Ok(FileType::Auto),
        "request" => Ok(FileType::AccountTransfer),
        "result" => Ok(FileType::AccountTransferResult),
        other => Err(usage_error(
            program,
            &format!("unsupported file type {other:?}; expected auto, request, or result"),
        )),
    }
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
        }
    } else {
        serde_json::to_writer_pretty(writer, file)
    }
}

fn print_usage(program: &OsString) {
    println!(
        "usage: {} [--metadata-only] [--type auto|request|result] <input-file>",
        program.to_string_lossy()
    );
}

fn usage_error(program: &OsString, message: &str) -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidInput,
        format!(
            "{message}\nusage: {} [--metadata-only] [--type auto|request|result] <input-file>",
            program.to_string_lossy()
        ),
    )
}
