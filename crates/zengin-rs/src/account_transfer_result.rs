use serde::{Deserialize, Serialize};

use crate::{Encoding, Error};

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
    pub code_division: u8,
    pub collector_code: String,
    pub collector_name: String,
    pub collection_date: String,
    pub bank_code: String,
    pub bank_name: String,
    pub branch_code: String,
    pub branch_name: String,
    pub account_type: u8,
    pub account_number: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Detail {
    pub bank_code: String,
    pub bank_name: String,
    pub branch_code: String,
    pub branch_name: String,
    pub account_type: u8,
    pub account_number: String,
    pub account_holder_name: String,
    pub amount: u64,
    pub new_code: u8,
    pub customer_number: String,
    pub result_code: u8,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Trailer {
    pub total_count: u32,
    pub total_amount: u64,
    pub success_count: u32,
    pub success_amount: u64,
    pub failure_count: u32,
    pub failure_amount: u64,
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

        let total_count = self.details.len() as u32;
        if self.trailer.total_count != total_count {
            return Err(Error::Validation(format!(
                "trailer total_count must be {total_count}, got {}",
                self.trailer.total_count
            )));
        }

        let total_amount = self
            .details
            .iter()
            .try_fold(0_u64, |sum, detail| sum.checked_add(detail.amount))
            .ok_or_else(|| Error::Validation("detail amount sum overflowed u64".to_string()))?;
        if self.trailer.total_amount != total_amount {
            return Err(Error::Validation(format!(
                "trailer total_amount must be {total_amount}, got {}",
                self.trailer.total_amount
            )));
        }

        let summary_totals_are_omitted = self.trailer.success_count == 0
            && self.trailer.success_amount == 0
            && self.trailer.failure_count == 0
            && self.trailer.failure_amount == 0;

        let success_count = self
            .details
            .iter()
            .filter(|detail| detail.result_code == 0)
            .count() as u32;
        let success_amount = self
            .details
            .iter()
            .filter(|detail| detail.result_code == 0)
            .try_fold(0_u64, |sum, detail| sum.checked_add(detail.amount))
            .ok_or_else(|| {
                Error::Validation("successful detail amount sum overflowed u64".to_string())
            })?;
        let failure_count = total_count - success_count;
        let failure_amount = total_amount - success_amount;

        if !summary_totals_are_omitted {
            if self.trailer.success_count != success_count {
                return Err(Error::Validation(format!(
                    "trailer success_count must be {success_count}, got {}",
                    self.trailer.success_count
                )));
            }

            if self.trailer.success_amount != success_amount {
                return Err(Error::Validation(format!(
                    "trailer success_amount must be {success_amount}, got {}",
                    self.trailer.success_amount
                )));
            }

            if self.trailer.failure_count != failure_count {
                return Err(Error::Validation(format!(
                    "trailer failure_count must be {failure_count}, got {}",
                    self.trailer.failure_count
                )));
            }

            if self.trailer.failure_amount != failure_amount {
                return Err(Error::Validation(format!(
                    "trailer failure_amount must be {failure_amount}, got {}",
                    self.trailer.failure_amount
                )));
            }
        }

        validate_count_width("trailer", "total_count", self.trailer.total_count)?;
        validate_amount_width("trailer", "total_amount", self.trailer.total_amount)?;
        validate_count_width("trailer", "success_count", self.trailer.success_count)?;
        validate_amount_width("trailer", "success_amount", self.trailer.success_amount)?;
        validate_count_width("trailer", "failure_count", self.trailer.failure_count)?;
        validate_amount_width("trailer", "failure_amount", self.trailer.failure_amount)?;

        Ok(())
    }
}

fn parse_header(record: &[u8]) -> Result<Header, Error> {
    ensure_record_type(record, "header", b'1')?;

    let kind_code = parse_number(record, 1..3, "header", "kind_code")? as u8;
    let code_division = parse_number(record, 3..4, "header", "code_division")? as u8;
    ensure_supported_encoding(code_division, "header", "code_division")?;
    ensure_spaces(record, 103..120, "header", "padding")?;

    Ok(Header {
        kind_code,
        code_division,
        collector_code: parse_digit_string(record, 4..14, "header", "collector_code")?,
        collector_name: parse_required_text(record, 14..54, "header", "collector_name")?,
        collection_date: parse_digit_string(record, 54..58, "header", "collection_date")?,
        bank_code: parse_digit_string(record, 58..62, "header", "bank_code")?,
        bank_name: parse_required_text(record, 62..77, "header", "bank_name")?,
        branch_code: parse_digit_string(record, 77..80, "header", "branch_code")?,
        branch_name: parse_optional_text(record, 80..95, "header", "branch_name")?,
        account_type: parse_number(record, 95..96, "header", "account_type")? as u8,
        account_number: parse_digit_string(record, 96..103, "header", "account_number")?,
    })
}

fn parse_detail(record: &[u8]) -> Result<Detail, Error> {
    ensure_record_type(record, "detail", b'2')?;
    ensure_spaces(record, 38..42, "detail", "bank_padding")?;
    ensure_spaces(record, 112..120, "detail", "padding")?;

    Ok(Detail {
        bank_code: parse_digit_string(record, 1..5, "detail", "bank_code")?,
        bank_name: parse_required_text(record, 5..20, "detail", "bank_name")?,
        branch_code: parse_digit_string(record, 20..23, "detail", "branch_code")?,
        branch_name: parse_optional_text(record, 23..38, "detail", "branch_name")?,
        account_type: parse_number(record, 42..43, "detail", "account_type")? as u8,
        account_number: parse_digit_string(record, 43..50, "detail", "account_number")?,
        account_holder_name: parse_required_text(record, 50..80, "detail", "account_holder_name")?,
        amount: parse_number(record, 80..90, "detail", "amount")?,
        new_code: parse_number(record, 90..91, "detail", "new_code")? as u8,
        customer_number: parse_optional_text(record, 91..111, "detail", "customer_number")?,
        result_code: parse_number(record, 111..112, "detail", "result_code")? as u8,
    })
}

fn parse_trailer(record: &[u8]) -> Result<Trailer, Error> {
    ensure_record_type(record, "trailer", b'8')?;
    ensure_spaces(record, 55..120, "trailer", "padding")?;

    Ok(Trailer {
        total_count: parse_number(record, 1..7, "trailer", "total_count")? as u32,
        total_amount: parse_number(record, 7..19, "trailer", "total_amount")?,
        success_count: parse_number(record, 19..25, "trailer", "success_count")? as u32,
        success_amount: parse_number(record, 25..37, "trailer", "success_amount")?,
        failure_count: parse_number(record, 37..43, "trailer", "failure_count")? as u32,
        failure_amount: parse_number(record, 43..55, "trailer", "failure_amount")?,
    })
}

fn parse_end(record: &[u8]) -> Result<End, Error> {
    ensure_record_type(record, "end", b'9')?;
    ensure_spaces(record, 1..120, "end", "padding")?;
    Ok(End)
}

fn validate_header(header: &Header) -> Result<(), Error> {
    if header.kind_code != 91 {
        return Err(Error::Validation(format!(
            "header kind_code must be 91, got {}",
            header.kind_code
        )));
    }

    ensure_supported_encoding(header.code_division, "header", "code_division")?;
    validate_digit_str("header", "collector_code", &header.collector_code, 10)?;
    validate_text_value("header", "collector_name", &header.collector_name)?;
    validate_digit_str("header", "collection_date", &header.collection_date, 4)?;
    validate_digit_str("header", "bank_code", &header.bank_code, 4)?;
    validate_text_value("header", "bank_name", &header.bank_name)?;
    validate_digit_str("header", "branch_code", &header.branch_code, 3)?;
    validate_text_value_allow_empty("header", "branch_name", &header.branch_name)?;
    validate_numeric_width("header", "account_type", header.account_type.into(), 1)?;
    validate_digit_str("header", "account_number", &header.account_number, 7)?;
    Ok(())
}

fn validate_detail(detail: &Detail) -> Result<(), Error> {
    validate_digit_str("detail", "bank_code", &detail.bank_code, 4)?;
    validate_text_value("detail", "bank_name", &detail.bank_name)?;
    validate_digit_str("detail", "branch_code", &detail.branch_code, 3)?;
    validate_text_value_allow_empty("detail", "branch_name", &detail.branch_name)?;
    validate_numeric_width("detail", "account_type", detail.account_type.into(), 1)?;
    validate_digit_str("detail", "account_number", &detail.account_number, 7)?;
    validate_text_value("detail", "account_holder_name", &detail.account_holder_name)?;
    validate_numeric_width("detail", "amount", detail.amount, 10)?;
    validate_numeric_width("detail", "new_code", detail.new_code.into(), 1)?;
    validate_text_value_allow_empty("detail", "customer_number", &detail.customer_number)?;
    validate_numeric_width("detail", "result_code", detail.result_code.into(), 1)?;
    Ok(())
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

fn ensure_supported_encoding(
    code_division: u8,
    record: &'static str,
    field: &'static str,
) -> Result<(), Error> {
    match code_division {
        0 => Ok(()),
        1 => Err(Error::UnsupportedEncoding(Encoding::Ebcdic)),
        other => Err(Error::InvalidField {
            record,
            field,
            message: format!("must be 0 or 1, got {other}"),
        }),
    }
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

fn decode_jis_char(byte: u8) -> Option<char> {
    match byte {
        0x20..=0x7E => Some(byte as char),
        0xA1..=0xDF => char::from_u32(u32::from(byte) - 0xA1 + 0xFF61),
        _ => None,
    }
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

fn validate_count_width(
    record: &'static str,
    field: &'static str,
    value: u32,
) -> Result<(), Error> {
    validate_numeric_width(record, field, value.into(), 6)
}

fn validate_amount_width(
    record: &'static str,
    field: &'static str,
    value: u64,
) -> Result<(), Error> {
    validate_numeric_width(record, field, value, 12)
}

#[cfg(test)]
mod tests {
    use super::{Detail, End, File, Header, Trailer, parse};
    use crate::{Encoding, Error};

    fn sample_file() -> File {
        File {
            header: Header {
                kind_code: 91,
                code_division: 0,
                collector_code: "1234567890".to_string(),
                collector_name: "ACME COLLECTOR".to_string(),
                collection_date: "0422".to_string(),
                bank_code: "0288".to_string(),
                bank_name: "BANK ALPHA".to_string(),
                branch_code: "220".to_string(),
                branch_name: "MAIN BRANCH".to_string(),
                account_type: 1,
                account_number: "5000001".to_string(),
            },
            details: vec![
                Detail {
                    bank_code: "0288".to_string(),
                    bank_name: "BANK ALPHA".to_string(),
                    branch_code: "110".to_string(),
                    branch_name: "WEST".to_string(),
                    account_type: 1,
                    account_number: "6000001".to_string(),
                    account_holder_name: "ALPHA INC".to_string(),
                    amount: 1000,
                    new_code: 0,
                    customer_number: "01234567890123450001".to_string(),
                    result_code: 0,
                },
                Detail {
                    bank_code: "0288".to_string(),
                    bank_name: "BANK ALPHA".to_string(),
                    branch_code: "650".to_string(),
                    branch_name: "EAST".to_string(),
                    account_type: 2,
                    account_number: "6000002".to_string(),
                    account_holder_name: "BETA LLC".to_string(),
                    amount: 2000,
                    new_code: 1,
                    customer_number: "01234567890123450002".to_string(),
                    result_code: 1,
                },
            ],
            trailer: Trailer {
                total_count: 2,
                total_amount: 3000,
                success_count: 1,
                success_amount: 1000,
                failure_count: 1,
                failure_amount: 2000,
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
                file.header.bank_code,
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
                    detail.bank_code,
                    pad_text(&detail.bank_name, 15),
                    detail.branch_code,
                    pad_text(&detail.branch_name, 15),
                    " ".repeat(4),
                    detail.account_type,
                    detail.account_number,
                    pad_text(&detail.account_holder_name, 30),
                    pad_number(detail.amount, 10),
                    detail.new_code,
                    detail.customer_number,
                ) + &format!("{}{}", detail.result_code, " ".repeat(8)),
            );
        }

        lines.push(
            format!(
                "8{}{}{}{}{}{}",
                pad_number(file.trailer.total_count, 6),
                pad_number(file.trailer.total_amount, 12),
                pad_number(file.trailer.success_count, 6),
                pad_number(file.trailer.success_amount, 12),
                pad_number(file.trailer.failure_count, 6),
                pad_number(file.trailer.failure_amount, 12),
            ) + &" ".repeat(65),
        );
        lines.push(format!("9{}", " ".repeat(119)));

        for line in &lines {
            assert_eq!(line.len(), 120);
        }

        lines.join("\r\n").into_bytes()
    }

    #[test]
    fn parses_result_file_layout() {
        let decoded = parse(&sample_bytes()).unwrap();
        assert_eq!(decoded, sample_file());
    }

    #[test]
    fn validates_result_summary_fields() {
        let mut bytes = sample_bytes();
        let trailer_offset = 122 * 3;
        bytes[trailer_offset + 24] = b'2';

        let error = parse(&bytes).unwrap_err();
        assert!(error.to_string().contains("trailer success_count"));
    }

    #[test]
    fn rejects_ebcdic_result_files() {
        let mut bytes = sample_bytes();
        bytes[3] = b'1';

        let error = parse(&bytes).unwrap_err();
        assert!(matches!(
            error,
            Error::UnsupportedEncoding(Encoding::Ebcdic)
        ));
    }

    #[test]
    fn accepts_blank_branch_names() {
        let mut bytes = sample_bytes();

        for offset in 80..95 {
            bytes[offset] = b' ';
        }

        let detail_offset = 122;
        for offset in detail_offset + 23..detail_offset + 38 {
            bytes[offset] = b' ';
        }

        let decoded = parse(&bytes).unwrap();
        assert_eq!(decoded.header.branch_name, "");
        assert_eq!(decoded.details[0].branch_name, "");
    }

    #[test]
    fn accepts_blank_and_space_padded_customer_numbers() {
        let mut bytes = sample_bytes();

        let first_detail_offset = 122;
        for offset in first_detail_offset + 91..first_detail_offset + 111 {
            bytes[offset] = b' ';
        }

        let second_detail_offset = 244;
        let customer_number = format!("{:<20}", "1043");
        bytes[second_detail_offset + 91..second_detail_offset + 111]
            .copy_from_slice(customer_number.as_bytes());

        let decoded = parse(&bytes).unwrap();
        assert_eq!(decoded.details[0].customer_number, "");
        assert_eq!(decoded.details[1].customer_number, "1043");
    }

    #[test]
    fn accepts_omitted_success_and_failure_summaries() {
        let mut bytes = sample_bytes();
        let trailer_offset = 122 * 3;

        for offset in trailer_offset + 19..trailer_offset + 55 {
            bytes[offset] = b'0';
        }

        let decoded = parse(&bytes).unwrap();
        assert_eq!(decoded.trailer.success_count, 0);
        assert_eq!(decoded.trailer.success_amount, 0);
        assert_eq!(decoded.trailer.failure_count, 0);
        assert_eq!(decoded.trailer.failure_amount, 0);
    }
}
