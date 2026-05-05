use std::{
    collections::{BTreeMap, HashSet},
    fs::File,
    io::{self, Read, Write},
    path::{Path, PathBuf},
    process::ExitCode,
};

use clap::{Parser, ValueEnum};
use serde::Serialize;
use zengin_fmt::{FileType, ParsedFile, parse_as};

const MAX_INPUT_BYTES: u64 = 10 * 1024 * 1024;

#[derive(Debug, Parser)]
#[command(
    name = "zengin",
    about = "Parse Zengin fixed-width files and write JSON or CSV to stdout"
)]
struct Args {
    #[arg(value_name = "input-file")]
    path: PathBuf,

    #[arg(short = 't', long = "type", value_enum, default_value_t = CliFileType::Auto)]
    file_type: CliFileType,

    #[arg(short = 'f', long = "format", value_enum, default_value_t = CliOutputFormat::Json)]
    output_format: CliOutputFormat,

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

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum CliOutputFormat {
    Json,
    Csv,
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
    write_output(&mut stdout, &file, args.metadata_only, args.output_format)?;
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

fn write_output<W>(
    mut writer: W,
    file: &ParsedFile,
    metadata_only: bool,
    output_format: CliOutputFormat,
) -> Result<(), Box<dyn std::error::Error>>
where
    W: Write,
{
    match output_format {
        CliOutputFormat::Json => {
            write_json(&mut writer, file, metadata_only)?;
            writer.write_all(b"\n")?;
        }
        CliOutputFormat::Csv => write_csv(&mut writer, file, metadata_only)?,
    }
    Ok(())
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

struct CsvRecord {
    record_type: &'static str,
    detail_index: Option<usize>,
    fields: Vec<(String, String)>,
}

fn write_csv<W>(
    mut writer: W,
    file: &ParsedFile,
    metadata_only: bool,
) -> Result<(), Box<dyn std::error::Error>>
where
    W: Write,
{
    let (file_type, value) = parsed_file_value(file)?;
    let value = value.as_object().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            "parsed file must serialize to a JSON object",
        )
    })?;

    let file_fields = file_fields(value)?;
    let records = csv_records(value, metadata_only)?;
    let columns = csv_columns(&file_fields, &records);

    write_csv_row(&mut writer, columns.iter().map(String::as_str))?;
    for record in records {
        let mut row = BTreeMap::new();
        row.insert("file_type".to_string(), file_type.to_string());
        row.insert("record_type".to_string(), record.record_type.to_string());
        row.insert(
            "detail_index".to_string(),
            record
                .detail_index
                .map_or_else(String::new, |index| index.to_string()),
        );
        for (field, value) in &file_fields {
            row.insert(format!("file_{field}"), value.clone());
        }
        for (field, value) in record.fields {
            row.insert(field, value);
        }

        write_csv_row(
            &mut writer,
            columns
                .iter()
                .map(|column| row.get(column).map(String::as_str).unwrap_or("")),
        )?;
    }

    Ok(())
}

fn parsed_file_value(
    file: &ParsedFile,
) -> Result<(&'static str, serde_json::Value), serde_json::Error> {
    match file {
        ParsedFile::GeneralTransfer(file) => Ok(("general-transfer", serde_json::to_value(file)?)),
        ParsedFile::PayrollTransfer(file) => Ok(("payroll-transfer", serde_json::to_value(file)?)),
        ParsedFile::AccountTransfer(file) => Ok(("account-transfer", serde_json::to_value(file)?)),
        ParsedFile::AccountTransferResult(file) => {
            Ok(("account-transfer-result", serde_json::to_value(file)?))
        }
        ParsedFile::TransferAccountInquiry(file) => {
            Ok(("transfer-account-inquiry", serde_json::to_value(file)?))
        }
        ParsedFile::PaymentNotice(file) => Ok(("payment-notice", serde_json::to_value(file)?)),
    }
}

fn file_fields(
    file: &serde_json::Map<String, serde_json::Value>,
) -> Result<Vec<(String, String)>, serde_json::Error> {
    file.iter()
        .filter(|(field, _)| !matches!(field.as_str(), "header" | "details" | "trailer" | "end"))
        .map(|(field, value)| Ok((field.clone(), csv_value(value)?)))
        .collect()
}

fn csv_records(
    file: &serde_json::Map<String, serde_json::Value>,
    metadata_only: bool,
) -> Result<Vec<CsvRecord>, serde_json::Error> {
    let mut records = Vec::new();

    if let Some(header) = file.get("header") {
        records.push(CsvRecord {
            record_type: "header",
            detail_index: None,
            fields: record_fields(header)?,
        });
    }

    if !metadata_only {
        if let Some(serde_json::Value::Array(details)) = file.get("details") {
            for (index, detail) in details.iter().enumerate() {
                records.push(CsvRecord {
                    record_type: "detail",
                    detail_index: Some(index + 1),
                    fields: record_fields(detail)?,
                });
            }
        }
    }

    if let Some(trailer) = file.get("trailer") {
        records.push(CsvRecord {
            record_type: "trailer",
            detail_index: None,
            fields: record_fields(trailer)?,
        });
    }

    if !metadata_only && file.contains_key("end") {
        records.push(CsvRecord {
            record_type: "end",
            detail_index: None,
            fields: Vec::new(),
        });
    }

    Ok(records)
}

fn record_fields(value: &serde_json::Value) -> Result<Vec<(String, String)>, serde_json::Error> {
    match value {
        serde_json::Value::Object(fields) => fields
            .iter()
            .map(|(field, value)| Ok((field.clone(), csv_value(value)?)))
            .collect(),
        value => Ok(vec![("value".to_string(), csv_value(value)?)]),
    }
}

fn csv_value(value: &serde_json::Value) -> Result<String, serde_json::Error> {
    match value {
        serde_json::Value::Null => Ok(String::new()),
        serde_json::Value::Bool(value) => Ok(value.to_string()),
        serde_json::Value::Number(value) => Ok(value.to_string()),
        serde_json::Value::String(value) => Ok(value.clone()),
        serde_json::Value::Array(_) | serde_json::Value::Object(_) => serde_json::to_string(value),
    }
}

fn csv_columns(file_fields: &[(String, String)], records: &[CsvRecord]) -> Vec<String> {
    let mut columns = Vec::from([
        "file_type".to_string(),
        "record_type".to_string(),
        "detail_index".to_string(),
    ]);
    let mut seen = columns.iter().cloned().collect::<HashSet<_>>();

    for (field, _) in file_fields {
        push_column(&mut columns, &mut seen, format!("file_{field}"));
    }
    for record in records {
        for (field, _) in &record.fields {
            push_column(&mut columns, &mut seen, field.clone());
        }
    }

    columns
}

fn push_column(columns: &mut Vec<String>, seen: &mut HashSet<String>, column: String) {
    if seen.insert(column.clone()) {
        columns.push(column);
    }
}

fn write_csv_row<'a, W, I>(writer: &mut W, cells: I) -> io::Result<()>
where
    W: Write,
    I: IntoIterator<Item = &'a str>,
{
    let mut first = true;
    for cell in cells {
        if first {
            first = false;
        } else {
            writer.write_all(b",")?;
        }
        write_csv_cell(writer, cell)?;
    }
    writer.write_all(b"\n")
}

fn write_csv_cell<W>(writer: &mut W, cell: &str) -> io::Result<()>
where
    W: Write,
{
    let needs_quotes = cell.contains([',', '"', '\r', '\n']);
    if !needs_quotes {
        writer.write_all(cell.as_bytes())?;
        return Ok(());
    }

    writer.write_all(b"\"")?;
    for byte in cell.bytes() {
        if byte == b'"' {
            writer.write_all(b"\"\"")?;
        } else {
            writer.write_all(&[byte])?;
        }
    }
    writer.write_all(b"\"")
}
