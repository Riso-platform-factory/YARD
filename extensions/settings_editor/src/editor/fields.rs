use serde_json::{Number, Value};
use std::collections::HashSet;

#[derive(Debug, PartialEq)]
pub enum FieldKind {
    Switch,
    Choice(Vec<String>),
    Number {
        integer: bool,
        minimum: Option<Number>,
        maximum: Option<Number>,
        format: Option<String>,
    },
    Text,
    FileOnly,
}

pub fn field_kind(root: &Value, schema: &Value) -> FieldKind {
    field_control(root, schema).kind
}

pub(super) struct FieldControl {
    pub kind: FieldKind,
    pub nullable: bool,
}

pub(super) fn field_control(root: &Value, schema: &Value) -> FieldControl {
    let mut nullable = false;
    let kind = classify(root, schema, &mut nullable).unwrap_or(FieldKind::FileOnly);
    FieldControl { kind, nullable }
}

fn classify<'a>(root: &'a Value, mut schema: &'a Value, nullable: &mut bool) -> Option<FieldKind> {
    let mut visited = HashSet::new();
    let mut bound_schemas = Vec::new();
    loop {
        if let Some(reference) = schema.get("$ref") {
            let reference = reference.as_str()?;
            let pointer = decode_fragment(reference.strip_prefix('#')?)?;
            if !pointer.starts_with("/definitions/") || !visited.insert(pointer.clone()) {
                return None;
            }
            schema = root.pointer(&pointer)?;
        } else if let Some(all_of) = schema.get("allOf") {
            let [only] = all_of.as_array()?.as_slice() else {
                return None;
            };
            bound_schemas.push(schema);
            schema = only;
        } else if let Some(any_of) = schema.get("anyOf") {
            let [first, second] = any_of.as_array()?.as_slice() else {
                return None;
            };
            schema = match (first["type"].as_str(), second["type"].as_str()) {
                (Some("null"), _) => second,
                (_, Some("null")) => first,
                _ => return None,
            };
            *nullable = true;
        } else {
            break;
        }
    }
    if schema.get("oneOf").is_some() {
        return None;
    }
    let schema_type = &schema["type"];
    let name = if let Some(name) = schema_type.as_str() {
        name
    } else {
        let [first, second] = schema_type.as_array()?.as_slice() else {
            return None;
        };
        *nullable = true;
        match (first.as_str()?, second.as_str()?) {
            ("null", name) | (name, "null") => name,
            _ => return None,
        }
    };
    bound_schemas.push(schema);
    Some(match name {
        "boolean" => FieldKind::Switch,
        "string" if schema.get("enum").is_some() => {
            let options: Vec<_> = schema["enum"]
                .as_array()?
                .iter()
                .filter(|value| !(*nullable && value.is_null()))
                .map(|value| value.as_str().map(str::to_owned))
                .collect::<Option<_>>()?;
            FieldKind::Choice(options)
        }
        "string" => FieldKind::Text,
        "integer" | "number" => FieldKind::Number {
            integer: name == "integer",
            minimum: bound(&bound_schemas, name == "integer", true),
            maximum: bound(&bound_schemas, name == "integer", false),
            format: schema["format"].as_str().map(str::to_owned),
        },
        _ => FieldKind::FileOnly,
    })
}

fn decode_fragment(fragment: &str) -> Option<String> {
    let mut bytes = fragment.bytes();
    let mut decoded = Vec::with_capacity(fragment.len());
    while let Some(byte) = bytes.next() {
        decoded.push(if byte == b'%' {
            let high = char::from(bytes.next()?).to_digit(16)?;
            let low = char::from(bytes.next()?).to_digit(16)?;
            (high * 16 + low) as u8
        } else {
            byte
        });
    }
    String::from_utf8(decoded).ok()
}

fn bound(schemas: &[&Value], integer: bool, minimum: bool) -> Option<Number> {
    let (key, round): (_, fn(f64) -> f64) = if minimum {
        ("minimum", f64::ceil)
    } else {
        ("maximum", f64::floor)
    };
    schemas
        .iter()
        .filter_map(|schema| schema[key].as_number())
        .max_by(|left, right| {
            let order = if integer {
                integer_bound(left, round).cmp(&integer_bound(right, round))
            } else {
                left.as_f64()
                    .partial_cmp(&right.as_f64())
                    .expect("finite JSON bounds")
            };
            if minimum { order } else { order.reverse() }
        })
        .cloned()
}

pub fn field_title(name: &str) -> String {
    let readable = name.replace('_', " ");
    let mut chars = readable.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => readable,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NumberError {
    NotANumber,
    NotAnInteger,
    BelowMinimum,
    AboveMaximum,
}

fn integer_format_bounds(format: Option<&str>) -> (i128, i128) {
    match format {
        Some("int8") => (i8::MIN.into(), i8::MAX.into()),
        Some("int16") => (i16::MIN.into(), i16::MAX.into()),
        Some("int32") => (i32::MIN.into(), i32::MAX.into()),
        Some("int64") => (i64::MIN.into(), i64::MAX.into()),
        Some("int") => (isize::MIN as i128, isize::MAX as i128),
        Some("uint8") => (0, u8::MAX.into()),
        Some("uint16") => (0, u16::MAX.into()),
        Some("uint32") => (0, u32::MAX.into()),
        Some("uint64") => (0, u64::MAX.into()),
        Some("uint") => (0, usize::MAX as i128),
        _ => (i64::MIN.into(), u64::MAX.into()),
    }
}

fn integer_bound(bound: &Number, round: fn(f64) -> f64) -> i128 {
    if let Some(value) = bound.as_i64() {
        value.into()
    } else if let Some(value) = bound.as_u64() {
        value.into()
    } else {
        round(bound.as_f64().expect("JSON number")) as i128
    }
}

pub fn parse_number(text: &str, kind: &FieldKind) -> Result<Value, NumberError> {
    use NumberError::*;
    let FieldKind::Number {
        integer,
        minimum,
        maximum,
        format,
    } = kind
    else {
        return Err(NotANumber);
    };
    let text = text.trim();
    if *integer {
        let digits = text.strip_prefix(['-', '+']).unwrap_or(text);
        if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
            return Err(
                if digits.starts_with(|ch: char| ch.is_ascii_digit())
                    && text.contains(['.', 'e', 'E'])
                {
                    NotAnInteger
                } else {
                    NotANumber
                },
            );
        }
        let number = text.parse::<i128>().map_err(|_| {
            if text.starts_with('-') {
                BelowMinimum
            } else {
                AboveMaximum
            }
        })?;
        let (min, max) = integer_format_bounds(format.as_deref());
        let min = minimum
            .as_ref()
            .map_or(min, |bound| min.max(integer_bound(bound, f64::ceil)));
        let max = maximum
            .as_ref()
            .map_or(max, |bound| max.min(integer_bound(bound, f64::floor)));
        if number < min {
            return Err(BelowMinimum);
        }
        if number > max {
            return Err(AboveMaximum);
        }
        return Ok(Value::Number(if number < 0 {
            i64::try_from(number).map_err(|_| BelowMinimum)?.into()
        } else {
            u64::try_from(number).map_err(|_| AboveMaximum)?.into()
        }));
    }
    let number = text.parse::<f64>().map_err(|_| NotANumber)?;
    if !number.is_finite() {
        return Err(NotANumber);
    }
    let limit = if format.as_deref() == Some("float") {
        f64::from(f32::MAX)
    } else {
        f64::MAX
    };
    let min = minimum
        .as_ref()
        .and_then(Number::as_f64)
        .unwrap_or(-limit)
        .max(-limit);
    let max = maximum
        .as_ref()
        .and_then(Number::as_f64)
        .unwrap_or(limit)
        .min(limit);
    if number < min {
        return Err(BelowMinimum);
    }
    if number > max {
        return Err(AboveMaximum);
    }
    Ok(Value::Number(Number::from_f64(number).ok_or(NotANumber)?))
}
