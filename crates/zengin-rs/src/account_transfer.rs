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
    pub collection_date: String,
    pub collector_code: String,
    pub collector_name: String,
    pub bank_code: String,
    pub branch_code: String,
    pub account_type: u8,
    pub account_number: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Detail {
    pub payer_code: String,
    pub payer_name: String,
    pub bank_code: String,
    pub branch_code: String,
    pub account_type: u8,
    pub account_number: String,
    pub amount: u64,
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
    ensure_spaces(record, 77..120, "header", "padding")?;

    Ok(Header {
        kind_code: parse_number(record, 1..3, "header", "kind_code")? as u8,
        collection_date: parse_digit_string(record, 3..11, "header", "collection_date")?,
        collector_code: parse_digit_string(record, 11..21, "header", "collector_code")?,
        collector_name: parse_text(record, 21..61, "header", "collector_name")?,
        bank_code: parse_digit_string(record, 61..65, "header", "bank_code")?,
        branch_code: parse_digit_string(record, 65..68, "header", "branch_code")?,
        account_type: parse_number(record, 68..69, "header", "account_type")? as u8,
        account_number: parse_digit_string(record, 69..77, "header", "account_number")?,
    })
}

fn parse_detail(record: &[u8]) -> Result<Detail, Error> {
    ensure_record_type(record, "detail", b'2')?;
    ensure_spaces(record, 77..120, "detail", "padding")?;

    Ok(Detail {
        payer_code: parse_digit_string(record, 1..11, "detail", "payer_code")?,
        payer_name: parse_text(record, 11..51, "detail", "payer_name")?,
        bank_code: parse_digit_string(record, 51..55, "detail", "bank_code")?,
        branch_code: parse_digit_string(record, 55..58, "detail", "branch_code")?,
        account_type: parse_number(record, 58..59, "detail", "account_type")? as u8,
        account_number: parse_digit_string(record, 59..67, "detail", "account_number")?,
        amount: parse_number(record, 67..77, "detail", "amount")?,
    })
}

fn parse_trailer(record: &[u8]) -> Result<Trailer, Error> {
    ensure_record_type(record, "trailer", b'8')?;
    ensure_spaces(record, 19..120, "trailer", "padding")?;

    Ok(Trailer {
        record_count: parse_number(record, 1..7, "trailer", "record_count")? as u32,
        total_amount: parse_number(record, 7..19, "trailer", "total_amount")?,
    })
}

fn parse_end(record: &[u8]) -> Result<End, Error> {
    ensure_record_type(record, "end", b'9')?;
    ensure_spaces(record, 1..120, "end", "padding")?;
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
    put_digits(
        &mut record,
        3..11,
        &header.collection_date,
        "header",
        "collection_date",
    )?;
    put_digits(
        &mut record,
        11..21,
        &header.collector_code,
        "header",
        "collector_code",
    )?;
    put_text(
        &mut record,
        21..61,
        &header.collector_name,
        "header",
        "collector_name",
        encoding,
    )?;
    put_digits(
        &mut record,
        61..65,
        &header.bank_code,
        "header",
        "bank_code",
    )?;
    put_digits(
        &mut record,
        65..68,
        &header.branch_code,
        "header",
        "branch_code",
    )?;
    put_number(
        &mut record,
        68..69,
        header.account_type.into(),
        "header",
        "account_type",
    )?;
    put_digits(
        &mut record,
        69..77,
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
    put_digits(
        &mut record,
        1..11,
        &detail.payer_code,
        "detail",
        "payer_code",
    )?;
    put_text(
        &mut record,
        11..51,
        &detail.payer_name,
        "detail",
        "payer_name",
        encoding,
    )?;
    put_digits(
        &mut record,
        51..55,
        &detail.bank_code,
        "detail",
        "bank_code",
    )?;
    put_digits(
        &mut record,
        55..58,
        &detail.branch_code,
        "detail",
        "branch_code",
    )?;
    put_number(
        &mut record,
        58..59,
        detail.account_type.into(),
        "detail",
        "account_type",
    )?;
    put_digits(
        &mut record,
        59..67,
        &detail.account_number,
        "detail",
        "account_number",
    )?;
    put_number(&mut record, 67..77, detail.amount, "detail", "amount")?;
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

    validate_digit_str("header", "collection_date", &header.collection_date, 8)?;
    validate_digit_str("header", "collector_code", &header.collector_code, 10)?;
    validate_text_value("header", "collector_name", &header.collector_name)?;
    validate_digit_str("header", "bank_code", &header.bank_code, 4)?;
    validate_digit_str("header", "branch_code", &header.branch_code, 3)?;
    validate_numeric_width("header", "account_type", header.account_type.into(), 1)?;
    validate_digit_str("header", "account_number", &header.account_number, 8)?;
    Ok(())
}

fn validate_detail(detail: &Detail) -> Result<(), Error> {
    validate_digit_str("detail", "payer_code", &detail.payer_code, 10)?;
    validate_text_value("detail", "payer_name", &detail.payer_name)?;
    validate_digit_str("detail", "bank_code", &detail.bank_code, 4)?;
    validate_digit_str("detail", "branch_code", &detail.branch_code, 3)?;
    validate_numeric_width("detail", "account_type", detail.account_type.into(), 1)?;
    validate_digit_str("detail", "account_number", &detail.account_number, 8)?;
    validate_numeric_width("detail", "amount", detail.amount, 10)?;
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
) -> Result<Vec<u8>, Error> {
    validate_text_value(record, field, value)?;

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

fn put_text(
    record: &mut [u8; RECORD_LEN],
    range: core::ops::Range<usize>,
    value: &str,
    record_name: &'static str,
    field: &'static str,
    encoding: Encoding,
) -> Result<(), Error> {
    let encoded = encode_text(value, encoding, record_name, field, range.len())?;
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

fn ensure_spaces(
    record: &[u8],
    range: core::ops::Range<usize>,
    record_name: &'static str,
    field: &'static str,
) -> Result<(), Error> {
    if !record[range.clone()].iter().all(|byte| *byte == b' ') {
        return Err(Error::InvalidField {
            record: record_name,
            field,
            message: "must be space padded".to_string(),
        });
    }

    Ok(())
}

fn parse_text(
    record: &[u8],
    range: core::ops::Range<usize>,
    record_name: &'static str,
    field: &'static str,
) -> Result<String, Error> {
    let value = decode_jis_text(&record[range], record_name, field)?;
    let value = value.trim_end_matches(' ').to_string();
    validate_text_value(record_name, field, &value)?;
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
