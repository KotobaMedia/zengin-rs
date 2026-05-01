use serde::{Deserialize, Serialize};

use crate::{CodeDivision, Encoding, Error, OutputFormat, fixed};

const RECORD_LEN: usize = 200;
const TEN_DIGIT_MAX: u64 = 9_999_999_999;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Format {
    #[default]
    A,
    B,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct File {
    #[serde(default)]
    pub format: Format,
    pub header: Header,
    pub details: Vec<Detail>,
    pub trailer: Trailer,
    pub end: End,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Header {
    pub kind_code: u8,
    #[serde(default)]
    pub code_division: CodeDivision,
    pub creation_date: String,
    pub account_date_from: String,
    pub account_date_to: String,
    pub bank_code: String,
    #[serde(default)]
    pub bank_name: String,
    pub branch_code: String,
    #[serde(default)]
    pub branch_name: String,
    pub account_type: u8,
    pub account_number: String,
    pub account_name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Detail {
    pub inquiry_number: String,
    pub account_date: String,
    pub value_date: String,
    pub amount: u64,
    pub other_bank_check_amount: u64,
    pub remitter_code: String,
    pub remitter_name: String,
    #[serde(default)]
    pub remitting_bank_name: String,
    #[serde(default)]
    pub remitting_branch_name: String,
    #[serde(default)]
    pub cancellation_type: String,
    #[serde(default)]
    pub edi_info: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Trailer {
    pub total_count: u32,
    pub total_amount: u64,
    pub cancellation_count: u32,
    pub cancellation_amount: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct End;

pub(crate) fn parse(input: &[u8]) -> Result<File, Error> {
    let records = fixed::split_records(input, RECORD_LEN)?;
    if records.len() < 4 {
        return Err(Error::InvalidInput(
            "expected header, detail, trailer, and end records".to_string(),
        ));
    }

    let header = parse_header(records[0])?;
    let trailer = parse_trailer(records[records.len() - 2])?;
    let end = parse_end(records[records.len() - 1])?;
    let parsed_details = records[1..records.len() - 2]
        .iter()
        .map(|record| parse_detail(record))
        .collect::<Result<Vec<_>, _>>()?;

    let format = parsed_details[0].0;
    if parsed_details
        .iter()
        .any(|(detail_format, _)| *detail_format != format)
    {
        return Err(Error::InvalidInput(
            "payment notice details mix format A and format B records".to_string(),
        ));
    }

    let details = parsed_details
        .into_iter()
        .map(|(_, detail)| detail)
        .collect::<Vec<_>>();

    let file = File {
        format,
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
    fixed::ensure_supported_output_encoding(format.encoding)?;

    let mut records = Vec::with_capacity(file.details.len() + 3);
    records.push(render_header(&file.header, format.encoding)?);
    records.extend(render_details(&file.details, file.format, format.encoding)?);
    records.push(render_trailer(&file.trailer)?);
    records.push(render_end(&file.end));

    Ok(fixed::write_records(records, format))
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
            validate_detail(detail, self.format)?;
        }

        let expected_count = self.details.len() as u32;
        if self.trailer.total_count != expected_count {
            return Err(Error::Validation(format!(
                "trailer total_count must be {expected_count}, got {}",
                self.trailer.total_count
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

        let cancellation_totals_are_omitted =
            self.trailer.cancellation_count == 0 && self.trailer.cancellation_amount == 0;
        let cancellation_count = self
            .details
            .iter()
            .filter(|detail| detail.cancellation_type == "1")
            .count() as u32;
        let cancellation_amount = self
            .details
            .iter()
            .filter(|detail| detail.cancellation_type == "1")
            .try_fold(0_u64, |sum, detail| sum.checked_add(detail.amount))
            .ok_or_else(|| {
                Error::Validation("cancellation detail amount sum overflowed u64".to_string())
            })?;

        if !cancellation_totals_are_omitted {
            if self.trailer.cancellation_count != cancellation_count {
                return Err(Error::Validation(format!(
                    "trailer cancellation_count must be {cancellation_count}, got {}",
                    self.trailer.cancellation_count
                )));
            }

            if self.trailer.cancellation_amount != cancellation_amount {
                return Err(Error::Validation(format!(
                    "trailer cancellation_amount must be {cancellation_amount}, got {}",
                    self.trailer.cancellation_amount
                )));
            }
        }

        fixed::validate_count_width("trailer", "total_count", self.trailer.total_count)?;
        fixed::validate_amount_width("trailer", "total_amount", self.trailer.total_amount)?;
        fixed::validate_count_width(
            "trailer",
            "cancellation_count",
            self.trailer.cancellation_count,
        )?;
        fixed::validate_amount_width(
            "trailer",
            "cancellation_amount",
            self.trailer.cancellation_amount,
        )?;
        Ok(())
    }
}

fn parse_header(record: &[u8]) -> Result<Header, Error> {
    fixed::ensure_record_type(record, "header", b'1')?;

    let code_division = fixed::parse_code_division(record, 3..4, "header", "code_division")?;
    fixed::ensure_supported_code_division(code_division, "header", "code_division")?;
    fixed::ensure_spaces(record, 107..200, "header", "padding")?;

    Ok(Header {
        kind_code: fixed::parse_number(record, 1..3, "header", "kind_code")? as u8,
        code_division,
        creation_date: fixed::parse_digit_string(record, 4..10, "header", "creation_date")?,
        account_date_from: fixed::parse_digit_string(
            record,
            10..16,
            "header",
            "account_date_from",
        )?,
        account_date_to: fixed::parse_digit_string(record, 16..22, "header", "account_date_to")?,
        bank_code: fixed::parse_digit_string(record, 22..26, "header", "bank_code")?,
        bank_name: fixed::parse_optional_text(record, 26..41, "header", "bank_name")?,
        branch_code: fixed::parse_digit_string(record, 41..44, "header", "branch_code")?,
        branch_name: fixed::parse_optional_text(record, 44..59, "header", "branch_name")?,
        account_type: fixed::parse_number(record, 59..60, "header", "account_type")? as u8,
        account_number: fixed::parse_digit_string(record, 60..67, "header", "account_number")?,
        account_name: fixed::parse_required_text(record, 67..107, "header", "account_name")?,
    })
}

fn parse_detail(record: &[u8]) -> Result<(Format, Detail), Error> {
    fixed::ensure_record_type(record, "detail", b'2')?;

    let format = if record[148..172].iter().any(|byte| *byte != b' ') {
        Format::B
    } else {
        Format::A
    };

    match format {
        Format::A => fixed::ensure_spaces(record, 148..200, "detail", "padding")?,
        Format::B => fixed::ensure_spaces(record, 172..200, "detail", "padding")?,
    }

    let amount_1 = fixed::parse_number(record, 19..29, "detail", "amount_1")?;
    let other_bank_check_amount_1 =
        fixed::parse_number(record, 29..39, "detail", "other_bank_check_amount_1")?;

    let (amount, other_bank_check_amount, edi_info) = match format {
        Format::A => (
            amount_1,
            other_bank_check_amount_1,
            fixed::parse_optional_text(record, 128..148, "detail", "edi_info")?,
        ),
        Format::B => {
            let amount_2 = fixed::parse_number(record, 128..140, "detail", "amount_2")?;
            let other_bank_check_amount_2 =
                fixed::parse_number(record, 140..152, "detail", "other_bank_check_amount_2")?;
            let uses_large_amount = amount_2 != 0 || other_bank_check_amount_2 != 0;
            (
                if uses_large_amount {
                    amount_2
                } else {
                    amount_1
                },
                if uses_large_amount {
                    other_bank_check_amount_2
                } else {
                    other_bank_check_amount_1
                },
                fixed::parse_optional_text(record, 152..172, "detail", "edi_info")?,
            )
        }
    };

    Ok((
        format,
        Detail {
            inquiry_number: fixed::parse_digit_string(record, 1..7, "detail", "inquiry_number")?,
            account_date: fixed::parse_digit_string(record, 7..13, "detail", "account_date")?,
            value_date: fixed::parse_digit_string(record, 13..19, "detail", "value_date")?,
            amount,
            other_bank_check_amount,
            remitter_code: fixed::parse_digit_string(record, 39..49, "detail", "remitter_code")?,
            remitter_name: fixed::parse_required_text(record, 49..97, "detail", "remitter_name")?,
            remitting_bank_name: fixed::parse_optional_text(
                record,
                97..112,
                "detail",
                "remitting_bank_name",
            )?,
            remitting_branch_name: fixed::parse_optional_text(
                record,
                112..127,
                "detail",
                "remitting_branch_name",
            )?,
            cancellation_type: fixed::parse_optional_text(
                record,
                127..128,
                "detail",
                "cancellation_type",
            )?,
            edi_info,
        },
    ))
}

fn parse_trailer(record: &[u8]) -> Result<Trailer, Error> {
    fixed::ensure_record_type(record, "trailer", b'8')?;
    fixed::ensure_spaces(record, 37..200, "trailer", "padding")?;

    Ok(Trailer {
        total_count: fixed::parse_number(record, 1..7, "trailer", "total_count")? as u32,
        total_amount: fixed::parse_number(record, 7..19, "trailer", "total_amount")?,
        cancellation_count: fixed::parse_number(record, 19..25, "trailer", "cancellation_count")?
            as u32,
        cancellation_amount: fixed::parse_number(record, 25..37, "trailer", "cancellation_amount")?,
    })
}

fn parse_end(record: &[u8]) -> Result<End, Error> {
    fixed::ensure_record_type(record, "end", b'9')?;
    fixed::ensure_spaces(record, 1..200, "end", "padding")?;
    Ok(End)
}

fn render_header(header: &Header, encoding: Encoding) -> Result<[u8; RECORD_LEN], Error> {
    validate_header(header)?;

    let mut record = fixed::blank_record(b'1');
    fixed::put_number(
        &mut record,
        1..3,
        header.kind_code.into(),
        "header",
        "kind_code",
    )?;
    fixed::put_code_division(
        &mut record,
        3..4,
        header.code_division,
        "header",
        "code_division",
    )?;
    fixed::put_digits(
        &mut record,
        4..10,
        &header.creation_date,
        "header",
        "creation_date",
    )?;
    fixed::put_digits(
        &mut record,
        10..16,
        &header.account_date_from,
        "header",
        "account_date_from",
    )?;
    fixed::put_digits(
        &mut record,
        16..22,
        &header.account_date_to,
        "header",
        "account_date_to",
    )?;
    fixed::put_digits(
        &mut record,
        22..26,
        &header.bank_code,
        "header",
        "bank_code",
    )?;
    fixed::put_optional_text(
        &mut record,
        26..41,
        &header.bank_name,
        "header",
        "bank_name",
        encoding,
    )?;
    fixed::put_digits(
        &mut record,
        41..44,
        &header.branch_code,
        "header",
        "branch_code",
    )?;
    fixed::put_optional_text(
        &mut record,
        44..59,
        &header.branch_name,
        "header",
        "branch_name",
        encoding,
    )?;
    fixed::put_number(
        &mut record,
        59..60,
        header.account_type.into(),
        "header",
        "account_type",
    )?;
    fixed::put_digits(
        &mut record,
        60..67,
        &header.account_number,
        "header",
        "account_number",
    )?;
    fixed::put_required_text(
        &mut record,
        67..107,
        &header.account_name,
        "header",
        "account_name",
        encoding,
    )?;
    Ok(record)
}

fn render_details(
    details: &[Detail],
    format: Format,
    encoding: Encoding,
) -> Result<Vec<[u8; RECORD_LEN]>, Error> {
    details
        .iter()
        .map(|detail| render_detail(detail, format, encoding))
        .collect()
}

fn render_detail(
    detail: &Detail,
    format: Format,
    encoding: Encoding,
) -> Result<[u8; RECORD_LEN], Error> {
    validate_detail(detail, format)?;

    let mut record = fixed::blank_record(b'2');
    fixed::put_digits(
        &mut record,
        1..7,
        &detail.inquiry_number,
        "detail",
        "inquiry_number",
    )?;
    fixed::put_digits(
        &mut record,
        7..13,
        &detail.account_date,
        "detail",
        "account_date",
    )?;
    fixed::put_digits(
        &mut record,
        13..19,
        &detail.value_date,
        "detail",
        "value_date",
    )?;

    let use_wide_amount = format == Format::B
        && (detail.amount > TEN_DIGIT_MAX || detail.other_bank_check_amount > TEN_DIGIT_MAX);
    let amount_1 = if use_wide_amount { 0 } else { detail.amount };
    let other_bank_check_amount_1 = if use_wide_amount {
        0
    } else {
        detail.other_bank_check_amount
    };

    fixed::put_number(&mut record, 19..29, amount_1, "detail", "amount_1")?;
    fixed::put_number(
        &mut record,
        29..39,
        other_bank_check_amount_1,
        "detail",
        "other_bank_check_amount_1",
    )?;
    fixed::put_digits(
        &mut record,
        39..49,
        &detail.remitter_code,
        "detail",
        "remitter_code",
    )?;
    fixed::put_required_text(
        &mut record,
        49..97,
        &detail.remitter_name,
        "detail",
        "remitter_name",
        encoding,
    )?;
    fixed::put_optional_text(
        &mut record,
        97..112,
        &detail.remitting_bank_name,
        "detail",
        "remitting_bank_name",
        encoding,
    )?;
    fixed::put_optional_text(
        &mut record,
        112..127,
        &detail.remitting_branch_name,
        "detail",
        "remitting_branch_name",
        encoding,
    )?;
    fixed::put_optional_digits(
        &mut record,
        127..128,
        &detail.cancellation_type,
        "detail",
        "cancellation_type",
    )?;

    match format {
        Format::A => {
            fixed::put_optional_text(
                &mut record,
                128..148,
                &detail.edi_info,
                "detail",
                "edi_info",
                encoding,
            )?;
        }
        Format::B => {
            fixed::put_number(
                &mut record,
                128..140,
                if use_wide_amount { detail.amount } else { 0 },
                "detail",
                "amount_2",
            )?;
            fixed::put_number(
                &mut record,
                140..152,
                if use_wide_amount {
                    detail.other_bank_check_amount
                } else {
                    0
                },
                "detail",
                "other_bank_check_amount_2",
            )?;
            fixed::put_optional_text(
                &mut record,
                152..172,
                &detail.edi_info,
                "detail",
                "edi_info",
                encoding,
            )?;
        }
    }

    Ok(record)
}

fn render_trailer(trailer: &Trailer) -> Result<[u8; RECORD_LEN], Error> {
    let mut record = fixed::blank_record(b'8');
    fixed::put_number(
        &mut record,
        1..7,
        trailer.total_count.into(),
        "trailer",
        "total_count",
    )?;
    fixed::put_number(
        &mut record,
        7..19,
        trailer.total_amount,
        "trailer",
        "total_amount",
    )?;
    fixed::put_number(
        &mut record,
        19..25,
        trailer.cancellation_count.into(),
        "trailer",
        "cancellation_count",
    )?;
    fixed::put_number(
        &mut record,
        25..37,
        trailer.cancellation_amount,
        "trailer",
        "cancellation_amount",
    )?;
    Ok(record)
}

fn render_end(_end: &End) -> [u8; RECORD_LEN] {
    fixed::blank_record(b'9')
}

fn validate_header(header: &Header) -> Result<(), Error> {
    if header.kind_code != 1 {
        return Err(Error::Validation(format!(
            "header kind_code must be 01, got {:02}",
            header.kind_code
        )));
    }

    fixed::ensure_supported_code_division(header.code_division, "header", "code_division")?;
    fixed::validate_digit_str("header", "creation_date", &header.creation_date, 6)?;
    fixed::validate_digit_str("header", "account_date_from", &header.account_date_from, 6)?;
    fixed::validate_digit_str("header", "account_date_to", &header.account_date_to, 6)?;
    fixed::validate_digit_str("header", "bank_code", &header.bank_code, 4)?;
    fixed::validate_text_value_allow_empty("header", "bank_name", &header.bank_name)?;
    fixed::validate_digit_str("header", "branch_code", &header.branch_code, 3)?;
    fixed::validate_text_value_allow_empty("header", "branch_name", &header.branch_name)?;
    fixed::validate_numeric_width("header", "account_type", header.account_type.into(), 1)?;
    fixed::validate_digit_str("header", "account_number", &header.account_number, 7)?;
    fixed::validate_text_value("header", "account_name", &header.account_name)?;
    Ok(())
}

fn validate_detail(detail: &Detail, format: Format) -> Result<(), Error> {
    fixed::validate_digit_str("detail", "inquiry_number", &detail.inquiry_number, 6)?;
    fixed::validate_digit_str("detail", "account_date", &detail.account_date, 6)?;
    fixed::validate_digit_str("detail", "value_date", &detail.value_date, 6)?;

    match format {
        Format::A => {
            fixed::validate_numeric_width("detail", "amount", detail.amount, 10)?;
            fixed::validate_numeric_width(
                "detail",
                "other_bank_check_amount",
                detail.other_bank_check_amount,
                10,
            )?;
        }
        Format::B => {
            fixed::validate_amount_width("detail", "amount", detail.amount)?;
            fixed::validate_amount_width(
                "detail",
                "other_bank_check_amount",
                detail.other_bank_check_amount,
            )?;
        }
    }

    fixed::validate_digit_str("detail", "remitter_code", &detail.remitter_code, 10)?;
    fixed::validate_text_value("detail", "remitter_name", &detail.remitter_name)?;
    fixed::validate_text_value_allow_empty(
        "detail",
        "remitting_bank_name",
        &detail.remitting_bank_name,
    )?;
    fixed::validate_text_value_allow_empty(
        "detail",
        "remitting_branch_name",
        &detail.remitting_branch_name,
    )?;
    fixed::validate_optional_digit_str(
        "detail",
        "cancellation_type",
        &detail.cancellation_type,
        1,
    )?;
    fixed::validate_text_value_allow_empty("detail", "edi_info", &detail.edi_info)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{Detail, End, File, Format, Header, Trailer, parse};
    use crate::{CodeDivision, OutputFormat, to_bytes};

    fn sample_file(format: Format, amount: u64) -> File {
        File {
            format,
            header: Header {
                kind_code: 1,
                code_division: CodeDivision::Jis,
                creation_date: "060425".to_string(),
                account_date_from: "060425".to_string(),
                account_date_to: "060425".to_string(),
                bank_code: "0001".to_string(),
                bank_name: "BANK ALPHA".to_string(),
                branch_code: "123".to_string(),
                branch_name: "MAIN".to_string(),
                account_type: 1,
                account_number: "7654321".to_string(),
                account_name: "ACME ACCOUNT".to_string(),
            },
            details: vec![Detail {
                inquiry_number: "000001".to_string(),
                account_date: "060425".to_string(),
                value_date: "060425".to_string(),
                amount,
                other_bank_check_amount: 0,
                remitter_code: "1234567890".to_string(),
                remitter_name: "TARO YAMADA".to_string(),
                remitting_bank_name: "BANK BETA".to_string(),
                remitting_branch_name: "WEST".to_string(),
                cancellation_type: String::new(),
                edi_info: "EDI123".to_string(),
            }],
            trailer: Trailer {
                total_count: 1,
                total_amount: amount,
                cancellation_count: 0,
                cancellation_amount: 0,
            },
            end: End,
        }
    }

    #[test]
    fn roundtrips_format_a_payment_notice() {
        let file = sample_file(Format::A, 1200);
        let encoded = to_bytes(&file, OutputFormat::readable()).unwrap();

        let lines = encoded
            .split(|byte| *byte == b'\n')
            .filter(|line| !line.is_empty())
            .map(|line| line.strip_suffix(b"\r").unwrap_or(line))
            .collect::<Vec<_>>();
        for line in &lines {
            assert_eq!(line.len(), 200);
        }

        assert_eq!(&lines[0][1..3], b"01");
        assert_eq!(&lines[1][19..29], b"0000001200");
        assert_eq!(&lines[1][128..134], b"EDI123");
        assert!(lines[1][148..200].iter().all(|byte| *byte == b' '));

        let decoded = parse(&encoded).unwrap();
        assert_eq!(decoded, file);
    }

    #[test]
    fn roundtrips_format_b_payment_notice_with_12_digit_amount() {
        let file = sample_file(Format::B, 10_000_000_000);
        let encoded = to_bytes(&file, OutputFormat::readable()).unwrap();

        let lines = encoded
            .split(|byte| *byte == b'\n')
            .filter(|line| !line.is_empty())
            .map(|line| line.strip_suffix(b"\r").unwrap_or(line))
            .collect::<Vec<_>>();
        assert_eq!(&lines[1][19..29], b"0000000000");
        assert_eq!(&lines[1][128..140], b"010000000000");
        assert_eq!(&lines[1][152..158], b"EDI123");

        let decoded = parse(&encoded).unwrap();
        assert_eq!(decoded.format, Format::B);
        assert_eq!(decoded.details[0].amount, 10_000_000_000);
        assert_eq!(decoded, file);
    }
}
