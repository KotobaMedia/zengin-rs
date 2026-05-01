use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::fmt;

pub mod account_transfer;
pub mod account_transfer_result;
mod fixed;
pub mod general_transfer;
pub mod payment_notice;
pub mod payroll_transfer;
pub mod transfer_account_inquiry;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, serde::Deserialize)]
#[serde(untagged)]
pub enum ParsedFile {
    GeneralTransfer(general_transfer::File),
    PayrollTransfer(payroll_transfer::File),
    AccountTransfer(account_transfer::File),
    AccountTransferResult(account_transfer_result::File),
    TransferAccountInquiry(transfer_account_inquiry::File),
    PaymentNotice(payment_notice::File),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileType {
    Auto,
    GeneralTransfer,
    PayrollTransfer,
    AccountTransfer,
    AccountTransferResult,
    TransferAccountInquiry,
    PaymentNotice,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CodeDivision {
    #[default]
    Jis,
    Ebcdic,
}

impl CodeDivision {
    pub const fn as_u8(self) -> u8 {
        match self {
            Self::Jis => 0,
            Self::Ebcdic => 1,
        }
    }

    pub const fn from_u8(value: u8) -> Option<Self> {
        match value {
            0 => Some(Self::Jis),
            1 => Some(Self::Ebcdic),
            _ => None,
        }
    }
}

impl Serialize for CodeDivision {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_u8(self.as_u8())
    }
}

impl<'de> Deserialize<'de> for CodeDivision {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        struct Visitor;

        impl serde::de::Visitor<'_> for Visitor {
            type Value = CodeDivision;

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("0 or 1")
            }

            fn visit_u64<E>(self, value: u64) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                let value = u8::try_from(value).map_err(E::custom)?;
                CodeDivision::from_u8(value)
                    .ok_or_else(|| E::custom(format!("invalid code division {value}")))
            }

            fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                match value {
                    "0" => Ok(CodeDivision::Jis),
                    "1" => Ok(CodeDivision::Ebcdic),
                    other => Err(E::custom(format!("invalid code division {other:?}"))),
                }
            }
        }

        deserializer.deserialize_any(Visitor)
    }
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
        FileType::GeneralTransfer => parse_general_transfer(input).map(ParsedFile::GeneralTransfer),
        FileType::PayrollTransfer => parse_payroll_transfer(input).map(ParsedFile::PayrollTransfer),
        FileType::AccountTransfer => parse_account_transfer(input).map(ParsedFile::AccountTransfer),
        FileType::AccountTransferResult => {
            parse_account_transfer_result(input).map(ParsedFile::AccountTransferResult)
        }
        FileType::TransferAccountInquiry => {
            parse_transfer_account_inquiry(input).map(ParsedFile::TransferAccountInquiry)
        }
        FileType::PaymentNotice => parse_payment_notice(input).map(ParsedFile::PaymentNotice),
    }
}

pub fn parse_general_transfer(input: &[u8]) -> Result<general_transfer::File, Error> {
    general_transfer::parse(input)
}

pub fn parse_payroll_transfer(input: &[u8]) -> Result<payroll_transfer::File, Error> {
    payroll_transfer::parse(input)
}

pub fn parse_account_transfer(input: &[u8]) -> Result<account_transfer::File, Error> {
    account_transfer::parse(input)
}

pub fn parse_account_transfer_result(input: &[u8]) -> Result<account_transfer_result::File, Error> {
    account_transfer_result::parse(input)
}

pub fn parse_transfer_account_inquiry(
    input: &[u8],
) -> Result<transfer_account_inquiry::File, Error> {
    transfer_account_inquiry::parse(input)
}

pub fn parse_payment_notice(input: &[u8]) -> Result<payment_notice::File, Error> {
    payment_notice::parse(input)
}

fn parse_auto(input: &[u8]) -> Result<ParsedFile, Error> {
    let mut matches = Vec::new();
    let mut errors = Vec::new();

    match general_transfer::parse(input) {
        Ok(file) => matches.push(("general transfer", ParsedFile::GeneralTransfer(file))),
        Err(error) => errors.push(("general transfer", error)),
    }
    match payroll_transfer::parse(input) {
        Ok(file) => matches.push(("payroll transfer", ParsedFile::PayrollTransfer(file))),
        Err(error) => errors.push(("payroll transfer", error)),
    }
    match account_transfer::parse(input) {
        Ok(file) => matches.push((
            "account transfer request",
            ParsedFile::AccountTransfer(file),
        )),
        Err(error) => errors.push(("account transfer request", error)),
    }
    match account_transfer_result::parse(input) {
        Ok(file) => matches.push((
            "account transfer result",
            ParsedFile::AccountTransferResult(file),
        )),
        Err(error) => errors.push(("account transfer result", error)),
    }
    match transfer_account_inquiry::parse(input) {
        Ok(file) => matches.push((
            "transfer account inquiry",
            ParsedFile::TransferAccountInquiry(file),
        )),
        Err(error) => errors.push(("transfer account inquiry", error)),
    }
    match payment_notice::parse(input) {
        Ok(file) => matches.push(("payment notice", ParsedFile::PaymentNotice(file))),
        Err(error) => errors.push(("payment notice", error)),
    }

    match matches.len() {
        1 => Ok(matches.pop().expect("one match").1),
        0 => {
            let summary = errors
                .into_iter()
                .map(|(name, error)| format!("{name}: {error}"))
                .collect::<Vec<_>>()
                .join("; ");
            Err(Error::InvalidInput(format!(
                "unsupported zengin file: {summary}"
            )))
        }
        _ => {
            let names = matches
                .into_iter()
                .map(|(name, _)| name)
                .collect::<Vec<_>>()
                .join(", ");
            Err(Error::AmbiguousInput(format!(
                "input is valid as both or more supported file types ({names}); pass an explicit file type"
            )))
        }
    }
}

pub fn to_bytes<T>(value: &T, format: OutputFormat) -> Result<Vec<u8>, Error>
where
    T: Serialize,
{
    to_bytes_as(value, FileType::Auto, format)
}

pub fn to_bytes_as<T>(
    value: &T,
    file_type: FileType,
    format: OutputFormat,
) -> Result<Vec<u8>, Error>
where
    T: Serialize,
{
    let value = serde_json::to_value(value)?;
    match file_type {
        FileType::Auto => write_auto_value(&value, format),
        FileType::GeneralTransfer => {
            write_value_as::<general_transfer::File>(&value, format, general_transfer::write)
        }
        FileType::PayrollTransfer => {
            write_value_as::<payroll_transfer::File>(&value, format, payroll_transfer::write)
        }
        FileType::AccountTransfer => {
            write_value_as::<account_transfer::File>(&value, format, account_transfer::write)
        }
        FileType::AccountTransferResult => write_value_as::<account_transfer_result::File>(
            &value,
            format,
            account_transfer_result::write,
        ),
        FileType::TransferAccountInquiry => write_value_as::<transfer_account_inquiry::File>(
            &value,
            format,
            transfer_account_inquiry::write,
        ),
        FileType::PaymentNotice => {
            write_value_as::<payment_notice::File>(&value, format, payment_notice::write)
        }
    }
}

fn write_value_as<T>(
    value: &serde_json::Value,
    format: OutputFormat,
    write: fn(&T, OutputFormat) -> Result<Vec<u8>, Error>,
) -> Result<Vec<u8>, Error>
where
    T: DeserializeOwned,
{
    let file = serde_json::from_value(value.clone())?;
    write(&file, format)
}

fn write_auto_value(value: &serde_json::Value, format: OutputFormat) -> Result<Vec<u8>, Error> {
    let mut matches = Vec::new();

    if let Ok(file) = serde_json::from_value::<general_transfer::File>(value.clone()) {
        matches.push(("general transfer", general_transfer::write(&file, format)?));
    }
    if let Ok(file) = serde_json::from_value::<payroll_transfer::File>(value.clone()) {
        matches.push(("payroll transfer", payroll_transfer::write(&file, format)?));
    }
    if let Ok(file) = serde_json::from_value::<account_transfer::File>(value.clone()) {
        matches.push((
            "account transfer request",
            account_transfer::write(&file, format)?,
        ));
    }
    if let Ok(file) = serde_json::from_value::<account_transfer_result::File>(value.clone()) {
        matches.push((
            "account transfer result",
            account_transfer_result::write(&file, format)?,
        ));
    }
    if let Ok(file) = serde_json::from_value::<transfer_account_inquiry::File>(value.clone()) {
        matches.push((
            "transfer account inquiry",
            transfer_account_inquiry::write(&file, format)?,
        ));
    }
    if let Ok(file) = serde_json::from_value::<payment_notice::File>(value.clone()) {
        matches.push(("payment notice", payment_notice::write(&file, format)?));
    }

    match matches.len() {
        1 => Ok(matches.pop().expect("one match").1),
        0 => Err(Error::InvalidInput(
            "unsupported zengin output value; pass a supported file type".to_string(),
        )),
        _ => {
            let names = matches
                .into_iter()
                .map(|(name, _)| name)
                .collect::<Vec<_>>()
                .join(", ");
            Err(Error::AmbiguousInput(format!(
                "value can be written as multiple supported file types ({names}); pass an explicit file type"
            )))
        }
    }
}

#[cfg(doctest)]
mod readme_doctests {
    doc_comment::doctest!("../../../README.md");
}

#[cfg(test)]
mod tests {
    use super::{
        CodeDivision, Encoding, Error, FileType, LineEnding, OutputFormat,
        account_transfer::Detail, account_transfer::End, account_transfer::File,
        account_transfer::Header, account_transfer::Trailer, from_bytes_as, parse_account_transfer,
        to_bytes,
    };

    fn sample_file() -> File {
        File {
            header: Header {
                kind_code: 91,
                code_division: CodeDivision::Jis,
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
                code_division: CodeDivision::Jis,
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
