use serde::{Serialize, de::DeserializeOwned};

pub mod account_transfer;

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
    let file = account_transfer::parse(input)?;
    let value = serde_json::to_value(file)?;
    Ok(serde_json::from_value(value)?)
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
    doc_comment::doctest!("../README.md");
}

#[cfg(test)]
mod tests {
    use super::{
        Encoding, LineEnding, OutputFormat, account_transfer::Detail, account_transfer::End,
        account_transfer::File, account_transfer::Header, account_transfer::Trailer, from_bytes,
        to_bytes,
    };

    fn sample_file() -> File {
        File {
            header: Header {
                kind_code: 91,
                collection_date: "20260430".to_string(),
                collector_code: "1234567890".to_string(),
                collector_name: "ACME COLLECT".to_string(),
                bank_code: "0001".to_string(),
                branch_code: "123".to_string(),
                account_type: 1,
                account_number: "76543210".to_string(),
            },
            details: vec![Detail {
                payer_code: "9000000001".to_string(),
                payer_name: "TARO YAMADA".to_string(),
                bank_code: "0005".to_string(),
                branch_code: "001".to_string(),
                account_type: 1,
                account_number: "12345678".to_string(),
                amount: 1200,
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
                collection_date: "20260430".to_string(),
                collector_code: "1234567890".to_string(),
                collector_name: "ﾃｽﾄｼｭｳｷﾝ".to_string(),
                bank_code: "0001".to_string(),
                branch_code: "123".to_string(),
                account_type: 1,
                account_number: "76543210".to_string(),
            },
            details: vec![Detail {
                payer_code: "9000000001".to_string(),
                payer_name: "ﾔﾏﾀﾞﾀﾛｳ".to_string(),
                bank_code: "0005".to_string(),
                branch_code: "001".to_string(),
                account_type: 1,
                account_number: "12345678".to_string(),
                amount: 1200,
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

        let decoded: File = from_bytes(&encoded).unwrap();
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

        let decoded: File = from_bytes(&encoded).unwrap();
        assert_eq!(decoded, sample_file());
    }

    #[test]
    fn roundtrips_jis_halfwidth_text_as_unicode() {
        let file = sample_jis_file();
        let encoded = to_bytes(&file, OutputFormat::readable()).unwrap();

        assert!(encoded.iter().any(|byte| *byte >= 0xA1));

        let decoded: File = from_bytes(&encoded).unwrap();
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
}
