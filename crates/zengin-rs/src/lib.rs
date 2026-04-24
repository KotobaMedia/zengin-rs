use serde::{Serialize, de::DeserializeOwned};

pub mod account_transfer;
pub mod account_transfer_result;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, serde::Deserialize)]
#[serde(untagged)]
pub enum ParsedFile {
    AccountTransfer(account_transfer::File),
    AccountTransferResult(account_transfer_result::File),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileType {
    Auto,
    AccountTransfer,
    AccountTransferResult,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Encoding {
    Ascii,
    Jis,
    Ebcdic,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineEnding {
    None,
    Lf,
    Crlf,
}

impl LineEnding {
    pub(crate) const fn as_bytes(self) -> &'static [u8] {
        match self {
            Self::None => b"",
            Self::Lf => b"\n",
            Self::Crlf => b"\r\n",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OutputFormat {
    pub encoding: Encoding,
    pub line_ending: LineEnding,
    pub eof: bool,
}

impl OutputFormat {
    pub const fn canonical() -> Self {
        Self {
            encoding: Encoding::Jis,
            line_ending: LineEnding::None,
            eof: false,
        }
    }

    pub const fn readable() -> Self {
        Self {
            encoding: Encoding::Jis,
            line_ending: LineEnding::Lf,
            eof: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    UnsupportedEncoding(Encoding),
    AmbiguousInput(String),
    InvalidInput(String),
    InvalidField {
        record: &'static str,
        field: &'static str,
        message: String,
    },
    Validation(String),
    Serde(String),
}

impl core::fmt::Display for Error {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::UnsupportedEncoding(encoding) => {
                write!(f, "unsupported encoding: {encoding:?}")
            }
            Self::AmbiguousInput(message) => f.write_str(message),
            Self::InvalidInput(message) => f.write_str(message),
            Self::InvalidField {
                record,
                field,
                message,
            } => {
                write!(f, "{record}.{field}: {message}")
            }
            Self::Validation(message) => f.write_str(message),
            Self::Serde(message) => f.write_str(message),
        }
    }
}

impl std::error::Error for Error {}

impl From<serde_json::Error> for Error {
    fn from(error: serde_json::Error) -> Self {
        Self::Serde(error.to_string())
    }
}

pub fn from_bytes<T>(input: &[u8]) -> Result<T, Error>
where
    T: DeserializeOwned,
{
    from_bytes_as(input, FileType::Auto)
}

pub fn from_bytes_as<T>(input: &[u8], file_type: FileType) -> Result<T, Error>
where
    T: DeserializeOwned,
{
    let file = parse_as(input, file_type)?;
    let value = serde_json::to_value(file)?;
    Ok(serde_json::from_value(value)?)
}

pub fn parse(input: &[u8]) -> Result<ParsedFile, Error> {
    parse_as(input, FileType::Auto)
}

pub fn parse_as(input: &[u8], file_type: FileType) -> Result<ParsedFile, Error> {
    match file_type {
        FileType::Auto => parse_auto(input),
        FileType::AccountTransfer => parse_account_transfer(input).map(ParsedFile::AccountTransfer),
        FileType::AccountTransferResult => {
            parse_account_transfer_result(input).map(ParsedFile::AccountTransferResult)
        }
    }
}

pub fn parse_account_transfer(input: &[u8]) -> Result<account_transfer::File, Error> {
    account_transfer::parse(input)
}

pub fn parse_account_transfer_result(input: &[u8]) -> Result<account_transfer_result::File, Error> {
    account_transfer_result::parse(input)
}

fn parse_auto(input: &[u8]) -> Result<ParsedFile, Error> {
    let account_transfer = account_transfer::parse(input);
    let account_transfer_result = account_transfer_result::parse(input);

    match (account_transfer, account_transfer_result) {
        (Ok(file), Err(_)) => Ok(ParsedFile::AccountTransfer(file)),
        (Err(_), Ok(file)) => Ok(ParsedFile::AccountTransferResult(file)),
        (Ok(_), Ok(_)) => Err(Error::AmbiguousInput(
            "input is valid as both an account transfer request and result file; pass an explicit file type".to_string(),
        )),
        (Err(account_transfer_error), Err(account_transfer_result_error)) => {
            Err(Error::InvalidInput(format!(
                "unsupported account transfer file: request parse failed with {account_transfer_error}; result parse failed with {account_transfer_result_error}"
            )))
        }
    }
}

pub fn to_bytes<T>(value: &T, format: OutputFormat) -> Result<Vec<u8>, Error>
where
    T: Serialize,
{
    let value = serde_json::to_value(value)?;
    let file: account_transfer::File = serde_json::from_value(value)?;
    account_transfer::write(&file, format)
}

#[cfg(doctest)]
mod readme_doctests {
    doc_comment::doctest!("../../../README.md");
}

#[cfg(test)]
mod tests {
    use super::{
        Encoding, Error, FileType, LineEnding, OutputFormat, account_transfer::Detail,
        account_transfer::End, account_transfer::File, account_transfer::Header,
        account_transfer::Trailer, from_bytes_as, parse_account_transfer, to_bytes,
    };

    fn sample_file() -> File {
        File {
            header: Header {
                kind_code: 91,
                code_division: "0".to_string(),
                collector_code: "1234567890".to_string(),
                collection_date: "0430".to_string(),
                collector_name: "ACME COLLECT".to_string(),
                bank_code: "0001".to_string(),
                bank_name: "BANK ALPHA".to_string(),
                branch_code: "123".to_string(),
                branch_name: "MAIN BRANCH".to_string(),
                account_type: 1,
                account_number: "7654321".to_string(),
            },
            details: vec![Detail {
                bank_code: "0005".to_string(),
                bank_name: "BANK BETA".to_string(),
                branch_code: "001".to_string(),
                branch_name: "WEST".to_string(),
                account_type: 1,
                account_number: "1234567".to_string(),
                payer_name: "TARO YAMADA".to_string(),
                amount: 1200,
                new_code: "0".to_string(),
                customer_number: "00000000001234567890".to_string(),
            }],
            trailer: Trailer {
                record_count: 1,
                total_amount: 1200,
            },
            end: End,
        }
    }

    fn sample_jis_file() -> File {
        File {
            header: Header {
                kind_code: 91,
                code_division: "0".to_string(),
                collector_code: "1234567890".to_string(),
                collection_date: "0430".to_string(),
                collector_name: "ﾃｽﾄｼｭｳｷﾝ".to_string(),
                bank_code: "0001".to_string(),
                bank_name: "ﾃｽﾄｷﾞﾝｺｳ".to_string(),
                branch_code: "123".to_string(),
                branch_name: "ﾎﾝﾃﾝ".to_string(),
                account_type: 1,
                account_number: "7654321".to_string(),
            },
            details: vec![Detail {
                bank_code: "0005".to_string(),
                bank_name: "ﾃｽﾄｷﾞﾝｺｳ".to_string(),
                branch_code: "001".to_string(),
                branch_name: "ｼﾃﾝ".to_string(),
                account_type: 1,
                account_number: "1234567".to_string(),
                payer_name: "ﾔﾏﾀﾞﾀﾛｳ".to_string(),
                amount: 1200,
                new_code: "0".to_string(),
                customer_number: "00000000001234567890".to_string(),
            }],
            trailer: Trailer {
                record_count: 1,
                total_amount: 1200,
            },
            end: End,
        }
    }

    #[test]
    fn roundtrips_readable_format() {
        let file = sample_file();
        let encoded = to_bytes(&file, OutputFormat::readable()).unwrap();

        assert!(encoded.contains(&b'\n'));

        let decoded = parse_account_transfer(&encoded).unwrap();
        assert_eq!(decoded, file);
    }

    #[test]
    fn canonical_format_has_no_line_breaks_or_eof() {
        let encoded = to_bytes(&sample_file(), OutputFormat::canonical()).unwrap();

        assert!(!encoded.contains(&b'\n'));
        assert!(!encoded.contains(&b'\r'));
        assert_ne!(encoded.last(), Some(&0x1a));
    }

    #[test]
    fn roundtrips_crlf_with_eof() {
        let encoded = to_bytes(
            &sample_file(),
            OutputFormat {
                encoding: Encoding::Ascii,
                line_ending: LineEnding::Crlf,
                eof: true,
            },
        )
        .unwrap();

        assert!(encoded.windows(2).any(|window| window == b"\r\n"));
        assert_eq!(encoded.last(), Some(&0x1a));

        let decoded = parse_account_transfer(&encoded).unwrap();
        assert_eq!(decoded, sample_file());
    }

    #[test]
    fn roundtrips_jis_halfwidth_text_as_unicode() {
        let file = sample_jis_file();
        let encoded = to_bytes(&file, OutputFormat::readable()).unwrap();

        assert!(encoded.iter().any(|byte| *byte >= 0xA1));

        let decoded = parse_account_transfer(&encoded).unwrap();
        assert_eq!(decoded, file);
        assert_eq!(decoded.header.collector_name, "ﾃｽﾄｼｭｳｷﾝ");
        assert_eq!(decoded.details[0].payer_name, "ﾔﾏﾀﾞﾀﾛｳ");
    }

    #[test]
    fn ascii_output_rejects_jis_text() {
        let error = to_bytes(
            &sample_jis_file(),
            OutputFormat {
                encoding: Encoding::Ascii,
                line_ending: LineEnding::Lf,
                eof: false,
            },
        )
        .unwrap_err();

        assert!(error.to_string().contains("must be encodable as ASCII"));
    }

    #[test]
    fn rejects_trailer_mismatch_on_write() {
        let mut file = sample_file();
        file.trailer.total_amount = 9999;

        let error = to_bytes(&file, OutputFormat::canonical()).unwrap_err();
        assert!(error.to_string().contains("trailer total_amount"));
    }

    #[test]
    fn auto_parse_rejects_ambiguous_files() {
        let encoded = to_bytes(&sample_file(), OutputFormat::readable()).unwrap();

        let error = super::parse(&encoded).unwrap_err();
        assert!(matches!(error, Error::AmbiguousInput(_)));
    }

    #[test]
    fn explicit_from_bytes_as_parses_ambiguous_files() {
        let encoded = to_bytes(&sample_file(), OutputFormat::readable()).unwrap();

        let decoded: File = from_bytes_as(&encoded, FileType::AccountTransfer).unwrap();
        assert_eq!(decoded, sample_file());
    }

    #[test]
    fn auto_parse_handles_malformed_inputs_without_panicking() {
        for len in 0..=512 {
            let input = (0..len)
                .map(|index| ((index * 37 + len) % 256) as u8)
                .collect::<Vec<_>>();

            let _ = super::parse(&input);
        }
    }
}
