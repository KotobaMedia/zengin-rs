use std::{
    env,
    ffi::OsString,
    fs,
    io::{self, Write},
    path::PathBuf,
    process::ExitCode,
};

use zengin_rs::{ParsedFile, from_bytes};

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
    let path = parse_args(env::args_os())?;
    let input = fs::read(path)?;
    let file: ParsedFile = from_bytes(&input)?;

    let stdout = io::stdout();
    let mut stdout = stdout.lock();
    serde_json::to_writer_pretty(&mut stdout, &file)?;
    stdout.write_all(b"\n")?;
    Ok(())
}

fn parse_args(args: impl IntoIterator<Item = OsString>) -> Result<PathBuf, io::Error> {
    let mut args = args.into_iter();
    let program = args.next().unwrap_or_else(|| OsString::from("zengin"));

    match (args.next(), args.next()) {
        (Some(flag), None) if flag == "--help" || flag == "-h" => {
            print_usage(&program);
            std::process::exit(0);
        }
        (Some(path), None) => Ok(PathBuf::from(path)),
        (None, _) => Err(usage_error(&program, "missing input file")),
        _ => Err(usage_error(&program, "expected exactly one input file")),
    }
}

fn print_usage(program: &OsString) {
    println!("usage: {} <input-file>", program.to_string_lossy());
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
