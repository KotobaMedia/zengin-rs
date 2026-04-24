use std::{
    env,
    ffi::{OsStr, OsString},
    fs::File,
    io::{self, Read, Write},
    path::{Path, PathBuf},
    process::ExitCode,
};

use zengin_rs::{FileType, ParsedFile, parse_as};

const MAX_INPUT_BYTES: u64 = 10 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
struct Args {
    path: PathBuf,
    file_type: FileType,
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
    serde_json::to_writer_pretty(&mut stdout, &file)?;
    stdout.write_all(b"\n")?;
    Ok(())
}

fn parse_args(args: impl IntoIterator<Item = OsString>) -> Result<ParsedArgs, io::Error> {
    let mut args = args.into_iter();
    let program = args.next().unwrap_or_else(|| OsString::from("zengin"));
    let mut file_type = FileType::Auto;
    let mut path = None;

    while let Some(arg) = args.next() {
        if arg == "--help" || arg == "-h" {
            return Ok(ParsedArgs::Help { program });
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
    Ok(ParsedArgs::Run(Args { path, file_type }))
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

fn print_usage(program: &OsString) {
    println!(
        "usage: {} [--type auto|request|result] <input-file>",
        program.to_string_lossy()
    );
}

fn usage_error(program: &OsString, message: &str) -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidInput,
        format!(
            "{message}\nusage: {} <input-file>",
            program.to_string_lossy()
        ),
    )
}
