use serde::{Deserialize, Serialize};

use crate::{Encoding, Error, OutputFormat};

const RECORD_LEN: usize = 120;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct File {
    pub header: Header,
    pub details: Vec<Detail>,
    pub trailer: Trailer,
    pub end: End,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Header {
    pub kind_code: u8,
    #[serde(default)]
    pub code_division: String,
    pub collector_code: String,
    pub collector_name: String,
    pub collection_date: String,
    #[serde(default)]
    pub bank_code: String,
    #[serde(default)]
    pub bank_name: String,
    pub branch_code: String,
    #[serde(default)]
    pub branch_name: String,
    pub account_type: u8,
    pub account_number: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Detail {
    #[serde(default)]
    pub bank_code: String,
    #[serde(default)]
    pub bank_name: String,
    pub branch_code: String,
    #[serde(default)]
    pub branch_name: String,
    pub account_type: u8,
    pub account_number: String,
    pub payer_name: String,
    pub amount: u64,
    #[serde(default)]
    pub new_code: String,
    /// Bank-specific 20-byte customer-code payload carried in bytes 92..111 of the request record.
    ///
    /// This crate intentionally does not normalize this field across banks. Callers must provide
    /// and interpret the wire representation themselves.
    ///
    /// Current wire behavior:
    /// - If the value is shorter than 20 bytes, it is left-aligned and the remaining bytes are
    ///   padded with ASCII spaces (`0x20`).
    /// - On parse, trailing ASCII spaces are trimmed.
    ///
    /// This matters in particular for Yucho (`ゆうちょBizダイレクト`), whose spec defines this
    /// 20-byte area as `支払人コード1` (10 bytes) + `支払人コード2` (10 bytes). This type does not
    /// split or join those subfields for you. If you need Yucho's 10+10 layout, encode and decode
    /// it on the caller side before passing it to this field.
    ///
    /// It also matters for banks whose specs require zero-filled numeric customer codes rather than
    /// space padding. In those cases, callers should pass the exact 20-byte wire value expected by
    /// the bank instead of relying on this crate to apply bank-specific padding rules.
    #[serde(default)]
    pub customer_number: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Trailer {
    pub record_count: u32,
    pub total_amount: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct End;

pub(crate) fn parse(input: &[u8]) -> Result<File, Error> {
    let records = split_records(input)?;
    if records.len() < 4 {
        return Err(Error::InvalidInput(
            "expected header, detail, trailer, and end records".to_string(),
        ));
    }

    let header = parse_header(records[0])?;
    let trailer = parse_trailer(records[records.len() - 2])?;
    let end = parse_end(records[records.len() - 1])?;
    let details = records[1..records.len() - 2]
        .iter()
        .map(|record| parse_detail(record))
        .collect::<Result<Vec<_>, _>>()?;

    let file = File {
        header,
        details,
        trailer,
        end,
    };
    file.validate()?;
    Ok(file)
}

pub(crate) fn write(file: &File, format: OutputFormat) -> Result<Vec<u8>, Error> {
    file.validate()?;

    match format.encoding {
        Encoding::Ascii | Encoding::Jis => {}
        other => return Err(Error::UnsupportedEncoding(other)),
    }

    let mut records = Vec::with_capacity(file.details.len() + 3);
    records.push(render_header(&file.header, format.encoding)?);
    records.extend(render_details(&file.details, format.encoding)?);
    records.push(render_trailer(&file.trailer)?);
    records.push(render_end(&file.end));

    let line_ending = format.line_ending.as_bytes();
    let mut output = Vec::with_capacity(
        records.len() * (RECORD_LEN + line_ending.len()) + usize::from(format.eof),
    );

    for record in records {
        output.extend_from_slice(&record);
        output.extend_from_slice(line_ending);
    }

    if format.eof {
        output.push(0x1a);
    }

    Ok(output)
}

impl File {
    fn validate(&self) -> Result<(), Error> {
        validate_header(&self.header)?;

        if self.details.is_empty() {
            return Err(Error::Validation(
                "at least one detail record is required".to_string(),
            ));
        }

        for detail in &self.details {
            validate_detail(detail)?;
        }

        let expected_count = self.details.len() as u32;
        if self.trailer.record_count != expected_count {
            return Err(Error::Validation(format!(
                "trailer record_count must be {expected_count}, got {}",
                self.trailer.record_count
            )));
        }

        let expected_total = self
            .details
            .iter()
            .try_fold(0_u64, |sum, detail| sum.checked_add(detail.amount))
            .ok_or_else(|| Error::Validation("detail amount sum overflowed u64".to_string()))?;

        if self.trailer.total_amount != expected_total {
            return Err(Error::Validation(format!(
                "trailer total_amount must be {expected_total}, got {}",
                self.trailer.total_amount
            )));
        }

        if self.trailer.record_count > 999_999 {
            return Err(Error::Validation(
                "trailer record_count exceeds 6 digits".to_string(),
            ));
        }

        if self.trailer.total_amount > 999_999_999_999 {
            return Err(Error::Validation(
                "trailer total_amount exceeds 12 digits".to_string(),
            ));
        }

        Ok(())
    }
}

fn split_records(input: &[u8]) -> Result<Vec<&[u8]>, Error> {
    let input = strip_optional_eof(input);
    if input.is_empty() {
        return Err(Error::InvalidInput("input is empty".to_string()));
    }

    if input.contains(&b'\n') {
        let mut lines = input.split(|byte| *byte == b'\n').collect::<Vec<_>>();
        if lines.last().is_some_and(|line| line.is_empty()) {
            lines.pop();
        }

        if lines.is_empty() {
            return Err(Error::InvalidInput(
                "input does not contain any records".to_string(),
            ));
        }

        let mut records = Vec::with_capacity(lines.len());
        for (index, line) in lines.into_iter().enumerate() {
            let line = line.strip_suffix(b"\r").unwrap_or(line);
            if line.contains(&b'\r') {
                return Err(Error::InvalidInput(format!(
                    "record {} contains an unexpected CR byte",
                    index + 1
                )));
            }
            ensure_record_len(line, index + 1)?;
            records.push(line);
        }
        return Ok(records);
    }

    if input.contains(&b'\r') {
        return Err(Error::InvalidInput(
            "bare CR line endings are not supported".to_string(),
        ));
    }

    if input.len() % RECORD_LEN != 0 {
        return Err(Error::InvalidInput(format!(
            "canonical input length must be a multiple of {RECORD_LEN}, got {}",
            input.len()
        )));
    }

    let mut records = Vec::with_capacity(input.len() / RECORD_LEN);
    for (index, record) in input.chunks(RECORD_LEN).enumerate() {
        ensure_record_len(record, index + 1)?;
        records.push(record);
    }
    Ok(records)
}

fn strip_optional_eof(input: &[u8]) -> &[u8] {
    input.strip_suffix(&[0x1a]).unwrap_or(input)
}

fn ensure_record_len(record: &[u8], index: usize) -> Result<(), Error> {
    if record.len() != RECORD_LEN {
        return Err(Error::InvalidInput(format!(
            "record {index} must be {RECORD_LEN} bytes, got {}",
            record.len()
        )));
    }

    Ok(())
}

fn parse_header(record: &[u8]) -> Result<Header, Error> {
    ensure_record_type(record, "header", b'1')?;

    Ok(Header {
        kind_code: parse_number(record, 1..3, "header", "kind_code")? as u8,
        code_division: parse_optional_text(record, 3..4, "header", "code_division")?,
        collector_code: parse_digit_string(record, 4..14, "header", "collector_code")?,
        collector_name: parse_required_text(record, 14..54, "header", "collector_name")?,
        collection_date: parse_digit_string(record, 54..58, "header", "collection_date")?,
        bank_code: parse_optional_text(record, 58..62, "header", "bank_code")?,
        bank_name: parse_optional_text(record, 62..77, "header", "bank_name")?,
        branch_code: parse_digit_string(record, 77..80, "header", "branch_code")?,
        branch_name: parse_optional_text(record, 80..95, "header", "branch_name")?,
        account_type: parse_number(record, 95..96, "header", "account_type")? as u8,
        account_number: parse_digit_string(record, 96..103, "header", "account_number")?,
    })
}

fn parse_detail(record: &[u8]) -> Result<Detail, Error> {
    ensure_record_type(record, "detail", b'2')?;

    let result_code = parse_number(record, 111..112, "detail", "result_code")? as u8;
    if result_code != 0 {
        return Err(Error::InvalidField {
            record: "detail",
            field: "result_code",
            message: format!("must be 0 for request files, got {result_code}"),
        });
    }

    Ok(Detail {
        bank_code: parse_optional_text(record, 1..5, "detail", "bank_code")?,
        bank_name: parse_optional_text(record, 5..20, "detail", "bank_name")?,
        branch_code: parse_digit_string(record, 20..23, "detail", "branch_code")?,
        branch_name: parse_optional_text(record, 23..38, "detail", "branch_name")?,
        account_type: parse_number(record, 42..43, "detail", "account_type")? as u8,
        account_number: parse_digit_string(record, 43..50, "detail", "account_number")?,
        payer_name: parse_required_text(record, 50..80, "detail", "payer_name")?,
        amount: parse_number(record, 80..90, "detail", "amount")?,
        new_code: parse_optional_text(record, 90..91, "detail", "new_code")?,
        customer_number: parse_optional_text(record, 91..111, "detail", "customer_number")?,
    })
}

fn parse_trailer(record: &[u8]) -> Result<Trailer, Error> {
    ensure_record_type(record, "trailer", b'8')?;

    let success_count = parse_number(record, 19..25, "trailer", "success_count")?;
    let success_amount = parse_number(record, 25..37, "trailer", "success_amount")?;
    let failure_count = parse_number(record, 37..43, "trailer", "failure_count")?;
    let failure_amount = parse_number(record, 43..55, "trailer", "failure_amount")?;

    for (field, value) in [
        ("success_count", success_count),
        ("success_amount", success_amount),
        ("failure_count", failure_count),
        ("failure_amount", failure_amount),
    ] {
        if value != 0 {
            return Err(Error::InvalidField {
                record: "trailer",
                field,
                message: format!("must be 0 for request files, got {value}"),
            });
        }
    }

    Ok(Trailer {
        record_count: parse_number(record, 1..7, "trailer", "record_count")? as u32,
        total_amount: parse_number(record, 7..19, "trailer", "total_amount")?,
    })
}

fn parse_end(record: &[u8]) -> Result<End, Error> {
    ensure_record_type(record, "end", b'9')?;
    Ok(End)
}

fn render_header(header: &Header, encoding: Encoding) -> Result<[u8; RECORD_LEN], Error> {
    validate_header(header)?;

    let mut record = blank_record(b'1');
    put_number(
        &mut record,
        1..3,
        header.kind_code.into(),
        "header",
        "kind_code",
    )?;
    put_optional_text(
        &mut record,
        3..4,
        &header.code_division,
        "header",
        "code_division",
        encoding,
    )?;
    put_digits(
        &mut record,
        4..14,
        &header.collector_code,
        "header",
        "collector_code",
    )?;
    put_required_text(
        &mut record,
        14..54,
        &header.collector_name,
        "header",
        "collector_name",
        encoding,
    )?;
    put_digits(
        &mut record,
        54..58,
        &header.collection_date,
        "header",
        "collection_date",
    )?;
    put_optional_text(
        &mut record,
        58..62,
        &header.bank_code,
        "header",
        "bank_code",
        encoding,
    )?;
    put_optional_text(
        &mut record,
        62..77,
        &header.bank_name,
        "header",
        "bank_name",
        encoding,
    )?;
    put_digits(
        &mut record,
        77..80,
        &header.branch_code,
        "header",
        "branch_code",
    )?;
    put_optional_text(
        &mut record,
        80..95,
        &header.branch_name,
        "header",
        "branch_name",
        encoding,
    )?;
    put_number(
        &mut record,
        95..96,
        header.account_type.into(),
        "header",
        "account_type",
    )?;
    put_digits(
        &mut record,
        96..103,
        &header.account_number,
        "header",
        "account_number",
    )?;
    Ok(record)
}

fn render_details(details: &[Detail], encoding: Encoding) -> Result<Vec<[u8; RECORD_LEN]>, Error> {
    details
        .iter()
        .map(|detail| render_detail(detail, encoding))
        .collect()
}

fn render_detail(detail: &Detail, encoding: Encoding) -> Result<[u8; RECORD_LEN], Error> {
    validate_detail(detail)?;

    let mut record = blank_record(b'2');
    put_optional_text(
        &mut record,
        1..5,
        &detail.bank_code,
        "detail",
        "bank_code",
        encoding,
    )?;
    put_optional_text(
        &mut record,
        5..20,
        &detail.bank_name,
        "detail",
        "bank_name",
        encoding,
    )?;
    put_digits(
        &mut record,
        20..23,
        &detail.branch_code,
        "detail",
        "branch_code",
    )?;
    put_optional_text(
        &mut record,
        23..38,
        &detail.branch_name,
        "detail",
        "branch_name",
        encoding,
    )?;
    put_number(
        &mut record,
        42..43,
        detail.account_type.into(),
        "detail",
        "account_type",
    )?;
    put_digits(
        &mut record,
        43..50,
        &detail.account_number,
        "detail",
        "account_number",
    )?;
    put_required_text(
        &mut record,
        50..80,
        &detail.payer_name,
        "detail",
        "payer_name",
        encoding,
    )?;
    put_number(&mut record, 80..90, detail.amount, "detail", "amount")?;
    put_optional_text(
        &mut record,
        90..91,
        &detail.new_code,
        "detail",
        "new_code",
        encoding,
    )?;
    put_optional_text(
        &mut record,
        91..111,
        &detail.customer_number,
        "detail",
        "customer_number",
        encoding,
    )?;
    record[111] = b'0';
    Ok(record)
}

fn render_trailer(trailer: &Trailer) -> Result<[u8; RECORD_LEN], Error> {
    let mut record = blank_record(b'8');
    put_number(
        &mut record,
        1..7,
        trailer.record_count.into(),
        "trailer",
        "record_count",
    )?;
    put_number(
        &mut record,
        7..19,
        trailer.total_amount,
        "trailer",
        "total_amount",
    )?;
    put_number(&mut record, 19..25, 0, "trailer", "success_count")?;
    put_number(&mut record, 25..37, 0, "trailer", "success_amount")?;
    put_number(&mut record, 37..43, 0, "trailer", "failure_count")?;
    put_number(&mut record, 43..55, 0, "trailer", "failure_amount")?;
    Ok(record)
}

fn render_end(_end: &End) -> [u8; RECORD_LEN] {
    blank_record(b'9')
}

fn validate_header(header: &Header) -> Result<(), Error> {
    if header.kind_code != 91 {
        return Err(Error::Validation(format!(
            "header kind_code must be 91, got {}",
            header.kind_code
        )));
    }

    validate_text_value_allow_empty("header", "code_division", &header.code_division)?;
    validate_digit_str("header", "collector_code", &header.collector_code, 10)?;
    validate_text_value("header", "collector_name", &header.collector_name)?;
    validate_digit_str("header", "collection_date", &header.collection_date, 4)?;
    validate_text_value_allow_empty("header", "bank_code", &header.bank_code)?;
    validate_text_value_allow_empty("header", "bank_name", &header.bank_name)?;
    validate_digit_str("header", "branch_code", &header.branch_code, 3)?;
    validate_text_value_allow_empty("header", "branch_name", &header.branch_name)?;
    validate_numeric_width("header", "account_type", header.account_type.into(), 1)?;
    validate_digit_str("header", "account_number", &header.account_number, 7)?;
    Ok(())
}

fn validate_detail(detail: &Detail) -> Result<(), Error> {
    validate_text_value_allow_empty("detail", "bank_code", &detail.bank_code)?;
    validate_text_value_allow_empty("detail", "bank_name", &detail.bank_name)?;
    validate_digit_str("detail", "branch_code", &detail.branch_code, 3)?;
    validate_text_value_allow_empty("detail", "branch_name", &detail.branch_name)?;
    validate_numeric_width("detail", "account_type", detail.account_type.into(), 1)?;
    validate_digit_str("detail", "account_number", &detail.account_number, 7)?;
    validate_text_value("detail", "payer_name", &detail.payer_name)?;
    validate_numeric_width("detail", "amount", detail.amount, 10)?;
    validate_text_value_allow_empty("detail", "new_code", &detail.new_code)?;
    validate_text_value_allow_empty("detail", "customer_number", &detail.customer_number)?;
    Ok(())
}

fn validate_digit_str(
    record: &'static str,
    field: &'static str,
    value: &str,
    width: usize,
) -> Result<(), Error> {
    if value.len() != width {
        return Err(Error::InvalidField {
            record,
            field,
            message: format!("must be exactly {width} digits, got {}", value.len()),
        });
    }

    if !value.as_bytes().iter().all(u8::is_ascii_digit) {
        return Err(Error::InvalidField {
            record,
            field,
            message: "must contain only ASCII digits".to_string(),
        });
    }

    Ok(())
}

fn validate_text_value(
    record: &'static str,
    field: &'static str,
    value: &str,
) -> Result<(), Error> {
    if value.is_empty() {
        return Err(Error::InvalidField {
            record,
            field,
            message: "must not be empty".to_string(),
        });
    }

    validate_text_value_allow_empty(record, field, value)
}

fn validate_text_value_allow_empty(
    record: &'static str,
    field: &'static str,
    value: &str,
) -> Result<(), Error> {
    if value.chars().any(char::is_control) {
        return Err(Error::InvalidField {
            record,
            field,
            message: "must not contain control characters".to_string(),
        });
    }

    Ok(())
}

fn encode_text(
    value: &str,
    encoding: Encoding,
    record: &'static str,
    field: &'static str,
    width: usize,
    required: bool,
) -> Result<Vec<u8>, Error> {
    if required {
        validate_text_value(record, field, value)?;
    } else {
        validate_text_value_allow_empty(record, field, value)?;
    }

    let mut encoded = Vec::with_capacity(value.chars().count());
    for ch in value.chars() {
        let byte = match encoding {
            Encoding::Ascii => encode_ascii_char(ch).ok_or_else(|| Error::InvalidField {
                record,
                field,
                message: format!("must be encodable as ASCII, got {:?}", ch),
            })?,
            Encoding::Jis => encode_jis_char(ch).ok_or_else(|| Error::InvalidField {
                record,
                field,
                message: format!("must be encodable as JIS X 0201, got {:?}", ch),
            })?,
            Encoding::Ebcdic => {
                return Err(Error::UnsupportedEncoding(Encoding::Ebcdic));
            }
        };
        encoded.push(byte);
    }

    if encoded.len() > width {
        return Err(Error::InvalidField {
            record,
            field,
            message: format!(
                "must be at most {width} bytes when encoded, got {}",
                encoded.len()
            ),
        });
    }

    Ok(encoded)
}

fn decode_jis_text(
    bytes: &[u8],
    record: &'static str,
    field: &'static str,
) -> Result<String, Error> {
    let mut value = String::with_capacity(bytes.len());
    for (index, byte) in bytes.iter().copied().enumerate() {
        let ch = decode_jis_char(byte).ok_or_else(|| Error::InvalidField {
            record,
            field,
            message: format!("invalid JIS X 0201 byte 0x{byte:02X} at offset {index}"),
        })?;
        value.push(ch);
    }
    Ok(value)
}

fn encode_ascii_char(ch: char) -> Option<u8> {
    match ch {
        ' '..='~' => Some(ch as u8),
        _ => None,
    }
}

fn encode_jis_char(ch: char) -> Option<u8> {
    match ch {
        ' '..='~' => Some(ch as u8),
        '¥' => Some(0x5C),
        '‾' => Some(0x7E),
        '\u{FF61}'..='\u{FF9F}' => Some((u32::from(ch) - 0xFF61 + 0xA1) as u8),
        _ => None,
    }
}

fn decode_jis_char(byte: u8) -> Option<char> {
    match byte {
        0x20..=0x7E => Some(byte as char),
        0xA1..=0xDF => char::from_u32(u32::from(byte) - 0xA1 + 0xFF61),
        _ => None,
    }
}

fn validate_numeric_width(
    record: &'static str,
    field: &'static str,
    value: u64,
    width: usize,
) -> Result<(), Error> {
    let formatted = format!("{value}");
    if formatted.len() > width {
        return Err(Error::InvalidField {
            record,
            field,
            message: format!("must fit within {width} digits, got {}", formatted.len()),
        });
    }

    Ok(())
}

fn blank_record(record_type: u8) -> [u8; RECORD_LEN] {
    let mut record = [b' '; RECORD_LEN];
    record[0] = record_type;
    record
}

fn put_required_text(
    record: &mut [u8; RECORD_LEN],
    range: core::ops::Range<usize>,
    value: &str,
    record_name: &'static str,
    field: &'static str,
    encoding: Encoding,
) -> Result<(), Error> {
    let encoded = encode_text(value, encoding, record_name, field, range.len(), true)?;
    record[range.start..range.start + encoded.len()].copy_from_slice(&encoded);
    Ok(())
}

fn put_optional_text(
    record: &mut [u8; RECORD_LEN],
    range: core::ops::Range<usize>,
    value: &str,
    record_name: &'static str,
    field: &'static str,
    encoding: Encoding,
) -> Result<(), Error> {
    let encoded = encode_text(value, encoding, record_name, field, range.len(), false)?;
    record[range.start..range.start + encoded.len()].copy_from_slice(&encoded);
    Ok(())
}

fn put_digits(
    record: &mut [u8; RECORD_LEN],
    range: core::ops::Range<usize>,
    value: &str,
    record_name: &'static str,
    field: &'static str,
) -> Result<(), Error> {
    validate_digit_str(record_name, field, value, range.len())?;
    record[range].copy_from_slice(value.as_bytes());
    Ok(())
}

fn put_number(
    record: &mut [u8; RECORD_LEN],
    range: core::ops::Range<usize>,
    value: u64,
    record_name: &'static str,
    field: &'static str,
) -> Result<(), Error> {
    validate_numeric_width(record_name, field, value, range.len())?;
    let formatted = format!("{value:0width$}", width = range.len());
    record[range].copy_from_slice(formatted.as_bytes());
    Ok(())
}

fn ensure_record_type(record: &[u8], record_name: &'static str, expected: u8) -> Result<(), Error> {
    if record[0] != expected {
        return Err(Error::InvalidInput(format!(
            "{record_name} record must start with {}, got {}",
            expected as char, record[0] as char
        )));
    }

    Ok(())
}

fn parse_required_text(
    record: &[u8],
    range: core::ops::Range<usize>,
    record_name: &'static str,
    field: &'static str,
) -> Result<String, Error> {
    let value = parse_optional_text(record, range, record_name, field)?;
    validate_text_value(record_name, field, &value)?;
    Ok(value)
}

fn parse_optional_text(
    record: &[u8],
    range: core::ops::Range<usize>,
    record_name: &'static str,
    field: &'static str,
) -> Result<String, Error> {
    let value = decode_jis_text(&record[range], record_name, field)?;
    let value = value.trim_end_matches(' ').to_string();
    validate_text_value_allow_empty(record_name, field, &value)?;
    Ok(value)
}

fn parse_digit_string(
    record: &[u8],
    range: core::ops::Range<usize>,
    record_name: &'static str,
    field: &'static str,
) -> Result<String, Error> {
    let value =
        core::str::from_utf8(&record[range.clone()]).map_err(|error| Error::InvalidField {
            record: record_name,
            field,
            message: error.to_string(),
        })?;
    validate_digit_str(record_name, field, value, range.len())?;
    Ok(value.to_string())
}

fn parse_number(
    record: &[u8],
    range: core::ops::Range<usize>,
    record_name: &'static str,
    field: &'static str,
) -> Result<u64, Error> {
    let value = parse_digit_string(record, range, record_name, field)?;
    value.parse::<u64>().map_err(|error| Error::InvalidField {
        record: record_name,
        field,
        message: error.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::{Detail, End, File, Header, RECORD_LEN, Trailer, parse};
    use crate::{Encoding, Error, LineEnding, OutputFormat, to_bytes};

    fn sample_file() -> File {
        File {
            header: Header {
                kind_code: 91,
                code_division: "0".to_string(),
                collector_code: "1234567890".to_string(),
                collector_name: "ACME COLLECTOR".to_string(),
                collection_date: "0422".to_string(),
                bank_code: "0185".to_string(),
                bank_name: "BANK ALPHA".to_string(),
                branch_code: "040".to_string(),
                branch_name: "MAIN BRANCH".to_string(),
                account_type: 1,
                account_number: "5000001".to_string(),
            },
            details: vec![
                Detail {
                    bank_code: "0185".to_string(),
                    bank_name: "BANK ALPHA".to_string(),
                    branch_code: "040".to_string(),
                    branch_name: "WEST".to_string(),
                    account_type: 1,
                    account_number: "6000001".to_string(),
                    payer_name: "ALPHA INC".to_string(),
                    amount: 1000,
                    new_code: "0".to_string(),
                    customer_number: "01234567890123450001".to_string(),
                },
                Detail {
                    bank_code: "0185".to_string(),
                    bank_name: "BANK ALPHA".to_string(),
                    branch_code: "041".to_string(),
                    branch_name: "EAST".to_string(),
                    account_type: 2,
                    account_number: "6000002".to_string(),
                    payer_name: "BETA LLC".to_string(),
                    amount: 2000,
                    new_code: "1".to_string(),
                    customer_number: "01234567890123450002".to_string(),
                },
            ],
            trailer: Trailer {
                record_count: 2,
                total_amount: 3000,
            },
            end: End,
        }
    }

    fn pad_text(value: &str, width: usize) -> String {
        format!("{value:<width$}")
    }

    fn pad_number<T>(value: T, width: usize) -> String
    where
        T: core::fmt::Display,
    {
        format!("{value:0width$}")
    }

    fn sample_bytes() -> Vec<u8> {
        let file = sample_file();
        let mut lines = Vec::new();

        lines.push(
            format!(
                "1{:02}{}{}{}{}{}{}{}{}{}{}",
                file.header.kind_code,
                file.header.code_division,
                file.header.collector_code,
                pad_text(&file.header.collector_name, 40),
                file.header.collection_date,
                pad_text(&file.header.bank_code, 4),
                pad_text(&file.header.bank_name, 15),
                file.header.branch_code,
                pad_text(&file.header.branch_name, 15),
                file.header.account_type,
                file.header.account_number,
            ) + &" ".repeat(17),
        );

        for detail in &file.details {
            lines.push(
                format!(
                    "2{}{}{}{}{}{}{}{}{}{}{}",
                    pad_text(&detail.bank_code, 4),
                    pad_text(&detail.bank_name, 15),
                    detail.branch_code,
                    pad_text(&detail.branch_name, 15),
                    " ".repeat(4),
                    detail.account_type,
                    detail.account_number,
                    pad_text(&detail.payer_name, 30),
                    pad_number(detail.amount, 10),
                    pad_text(&detail.new_code, 1),
                    pad_text(&detail.customer_number, 20),
                ) + &format!("{}{}", 0, " ".repeat(8)),
            );
        }

        lines.push(
            format!(
                "8{}{}{}{}{}{}",
                pad_number(file.trailer.record_count, 6),
                pad_number(file.trailer.total_amount, 12),
                pad_number(0, 6),
                pad_number(0, 12),
                pad_number(0, 6),
                pad_number(0, 12),
            ) + &" ".repeat(65),
        );
        lines.push(format!("9{}", " ".repeat(119)));

        for line in &lines {
            assert_eq!(line.len(), 120);
        }

        lines.join("\r\n").into_bytes()
    }

    fn sample_canonical_bytes() -> Vec<u8> {
        sample_bytes()
            .into_iter()
            .filter(|byte| *byte != b'\r' && *byte != b'\n')
            .collect()
    }

    #[test]
    fn parses_request_file_layout() {
        let decoded = parse(&sample_bytes()).unwrap();
        assert_eq!(decoded, sample_file());
    }

    #[test]
    fn writes_documented_request_layout() {
        let encoded = to_bytes(
            &sample_file(),
            OutputFormat {
                encoding: Encoding::Jis,
                line_ending: LineEnding::None,
                eof: false,
            },
        )
        .unwrap();

        assert_eq!(encoded, sample_canonical_bytes());
    }

    #[test]
    fn short_customer_number_is_space_padded_on_write() {
        let mut file = sample_file();
        file.details[0].customer_number = "1043".to_string();

        let encoded = to_bytes(
            &file,
            OutputFormat {
                encoding: Encoding::Ascii,
                line_ending: LineEnding::None,
                eof: false,
            },
        )
        .unwrap();

        let first_detail_offset = RECORD_LEN;
        assert_eq!(
            &encoded[first_detail_offset + 91..first_detail_offset + 111],
            b"1043                "
        );
    }

    #[test]
    fn rejects_non_zero_result_codes() {
        let mut bytes = sample_bytes();
        let detail_offset = 122;
        bytes[detail_offset + 111] = b'1';

        let error = parse(&bytes).unwrap_err();
        assert!(error.to_string().contains("detail.result_code"));
    }

    #[test]
    fn rejects_non_zero_request_summary_fields() {
        let mut bytes = sample_bytes();
        let trailer_offset = 122 * 3;
        bytes[trailer_offset + 24] = b'1';

        let error = parse(&bytes).unwrap_err();
        assert!(error.to_string().contains("trailer.success_count"));
    }

    #[test]
    fn accepts_blank_optional_fields_from_yucho_style_layout() {
        let mut bytes = sample_bytes();

        bytes[3] = b' ';
        for offset in 58..77 {
            bytes[offset] = b' ';
        }
        for offset in 80..95 {
            bytes[offset] = b' ';
        }

        let detail_offset = 122;
        for offset in detail_offset + 1..detail_offset + 20 {
            bytes[offset] = b' ';
        }
        for offset in detail_offset + 23..detail_offset + 38 {
            bytes[offset] = b' ';
        }
        bytes[detail_offset + 90] = b' ';
        for offset in detail_offset + 91..detail_offset + 111 {
            bytes[offset] = b' ';
        }

        let decoded = parse(&bytes).unwrap();
        assert_eq!(decoded.header.code_division, "");
        assert_eq!(decoded.header.bank_code, "");
        assert_eq!(decoded.header.bank_name, "");
        assert_eq!(decoded.header.branch_name, "");
        assert_eq!(decoded.details[0].bank_code, "");
        assert_eq!(decoded.details[0].bank_name, "");
        assert_eq!(decoded.details[0].branch_name, "");
        assert_eq!(decoded.details[0].new_code, "");
        assert_eq!(decoded.details[0].customer_number, "");
    }

    #[test]
    fn rejects_ebcdic_request_files() {
        let error = crate::to_bytes(
            &sample_file(),
            crate::OutputFormat {
                encoding: Encoding::Ebcdic,
                line_ending: crate::LineEnding::None,
                eof: false,
            },
        )
        .unwrap_err();

        assert!(matches!(
            error,
            Error::UnsupportedEncoding(Encoding::Ebcdic)
        ));
    }
}
