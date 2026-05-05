use crate::{CodeDivision, Encoding, Error, OutputFormat};

pub(crate) fn split_records(input: &[u8], record_len: usize) -> Result<Vec<&[u8]>, Error> {
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
            ensure_record_len(line, record_len, index + 1)?;
            records.push(line);
        }
        return Ok(records);
    }

    if input.contains(&b'\r') {
        return Err(Error::InvalidInput(
            "bare CR line endings are not supported".to_string(),
        ));
    }

    if !input.len().is_multiple_of(record_len) {
        return Err(Error::InvalidInput(format!(
            "canonical input length must be a multiple of {record_len}, got {}",
            input.len()
        )));
    }

    let mut records = Vec::with_capacity(input.len() / record_len);
    for (index, record) in input.chunks(record_len).enumerate() {
        ensure_record_len(record, record_len, index + 1)?;
        records.push(record);
    }
    Ok(records)
}

fn strip_optional_eof(input: &[u8]) -> &[u8] {
    input.strip_suffix(&[0x1a]).unwrap_or(input)
}

fn ensure_record_len(record: &[u8], record_len: usize, index: usize) -> Result<(), Error> {
    if record.len() != record_len {
        return Err(Error::InvalidInput(format!(
            "record {index} must be {record_len} bytes, got {}",
            record.len()
        )));
    }

    Ok(())
}

pub(crate) fn write_records<const N: usize>(
    records: Vec<[u8; N]>,
    format: OutputFormat,
) -> Vec<u8> {
    let line_ending = format.line_ending.as_bytes();
    let mut output =
        Vec::with_capacity(records.len() * (N + line_ending.len()) + usize::from(format.eof));

    for record in records {
        output.extend_from_slice(&record);
        output.extend_from_slice(line_ending);
    }

    if format.eof {
        output.push(0x1a);
    }

    output
}

pub(crate) fn ensure_supported_output_encoding(encoding: Encoding) -> Result<(), Error> {
    match encoding {
        Encoding::Ascii | Encoding::Jis => Ok(()),
        other => Err(Error::UnsupportedEncoding(other)),
    }
}

pub(crate) fn parse_code_division(
    record: &[u8],
    range: core::ops::Range<usize>,
    record_name: &'static str,
    field: &'static str,
) -> Result<CodeDivision, Error> {
    let value = parse_number(record, range, record_name, field)?;
    let value = u8::try_from(value).map_err(|error| Error::InvalidField {
        record: record_name,
        field,
        message: error.to_string(),
    })?;
    CodeDivision::from_u8(value).ok_or_else(|| Error::InvalidField {
        record: record_name,
        field,
        message: format!("must be 0 or 1, got {value}"),
    })
}

pub(crate) fn ensure_supported_code_division(
    code_division: CodeDivision,
    _record: &'static str,
    _field: &'static str,
) -> Result<(), Error> {
    match code_division {
        CodeDivision::Jis => Ok(()),
        CodeDivision::Ebcdic => Err(Error::UnsupportedEncoding(Encoding::Ebcdic)),
    }
}

pub(crate) fn ensure_record_type(
    record: &[u8],
    record_name: &'static str,
    expected: u8,
) -> Result<(), Error> {
    if record[0] != expected {
        return Err(Error::InvalidInput(format!(
            "{record_name} record must start with {}, got {}",
            expected as char, record[0] as char
        )));
    }

    Ok(())
}

pub(crate) fn ensure_spaces(
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

pub(crate) fn parse_required_text(
    record: &[u8],
    range: core::ops::Range<usize>,
    record_name: &'static str,
    field: &'static str,
) -> Result<String, Error> {
    let value = parse_optional_text(record, range, record_name, field)?;
    validate_text_value(record_name, field, &value)?;
    Ok(value)
}

pub(crate) fn parse_optional_text(
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

pub(crate) fn parse_digit_string(
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

pub(crate) fn parse_number(
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

pub(crate) fn validate_digit_str(
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

pub(crate) fn validate_optional_digit_str(
    record: &'static str,
    field: &'static str,
    value: &str,
    width: usize,
) -> Result<(), Error> {
    if value.is_empty() {
        return Ok(());
    }

    validate_digit_str(record, field, value, width)
}

pub(crate) fn validate_text_value(
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

pub(crate) fn validate_text_value_allow_empty(
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

pub(crate) fn validate_numeric_width(
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

pub(crate) fn validate_count_width(
    record: &'static str,
    field: &'static str,
    value: u32,
) -> Result<(), Error> {
    validate_numeric_width(record, field, value.into(), 6)
}

pub(crate) fn validate_amount_width(
    record: &'static str,
    field: &'static str,
    value: u64,
) -> Result<(), Error> {
    validate_numeric_width(record, field, value, 12)
}

pub(crate) fn blank_record<const N: usize>(record_type: u8) -> [u8; N] {
    let mut record = [b' '; N];
    record[0] = record_type;
    record
}

pub(crate) fn put_required_text<const N: usize>(
    record: &mut [u8; N],
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

pub(crate) fn put_optional_text<const N: usize>(
    record: &mut [u8; N],
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

pub(crate) fn put_digits<const N: usize>(
    record: &mut [u8; N],
    range: core::ops::Range<usize>,
    value: &str,
    record_name: &'static str,
    field: &'static str,
) -> Result<(), Error> {
    validate_digit_str(record_name, field, value, range.len())?;
    record[range].copy_from_slice(value.as_bytes());
    Ok(())
}

pub(crate) fn put_code_division<const N: usize>(
    record: &mut [u8; N],
    range: core::ops::Range<usize>,
    value: CodeDivision,
    record_name: &'static str,
    field: &'static str,
) -> Result<(), Error> {
    put_number(record, range, value.as_u8().into(), record_name, field)
}

pub(crate) fn put_optional_digits<const N: usize>(
    record: &mut [u8; N],
    range: core::ops::Range<usize>,
    value: &str,
    record_name: &'static str,
    field: &'static str,
) -> Result<(), Error> {
    if value.is_empty() {
        return Ok(());
    }

    put_digits(record, range, value, record_name, field)
}

pub(crate) fn put_number<const N: usize>(
    record: &mut [u8; N],
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
