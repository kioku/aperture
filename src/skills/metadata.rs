use super::invalid;
use crate::error::Error;
use serde::Deserialize;

/// Known fields are typed strictly; unknown standard frontmatter fields are ignored.
#[derive(Default, Deserialize)]
pub(super) struct Metadata {
    #[serde(default, deserialize_with = "optional_string")]
    pub name: Option<String>,
    #[serde(default, deserialize_with = "optional_string")]
    pub description: Option<String>,
    #[serde(default)]
    pub aperture: ApiRequirements,
}
#[derive(Default, Deserialize)]
pub(super) struct ApiRequirements {
    #[serde(default)]
    pub required_apis: Vec<String>,
}

pub(super) fn parse(content: &str) -> Result<Metadata, Error> {
    if content.trim().is_empty() {
        return Err(invalid("Skill Markdown is empty"));
    }
    let normalized = content.replace("\r\n", "\n");
    let Some(rest) = normalized.strip_prefix("---\n") else {
        return Ok(Metadata::default());
    };
    let (yaml, body) = frontmatter(rest)?;
    if body.trim().is_empty() {
        return Err(invalid("Skill Markdown body is empty"));
    }
    validate_shape(yaml)?;
    let metadata: Metadata = serde_yaml::from_str(yaml).map_err(|_| invalid("Invalid skill frontmatter: known fields must have their documented types and keys must be unique"))?;
    validate_metadata(&metadata)?;
    Ok(metadata)
}

fn validate_metadata(metadata: &Metadata) -> Result<(), Error> {
    if let Some(name) = &metadata.name {
        validate_name(name)?;
    }
    if metadata
        .description
        .as_ref()
        .is_some_and(|value| value.trim().is_empty())
    {
        return Err(invalid("Skill description cannot be empty"));
    }
    for api in &metadata.aperture.required_apis {
        crate::config::context_name::ApiContextName::new(api)
            .map_err(|_| invalid("Invalid required API context name"))?;
    }
    let mut apis = metadata.aperture.required_apis.clone();
    apis.sort();
    apis.dedup();
    if apis.len() != metadata.aperture.required_apis.len() {
        return Err(invalid("Duplicate required API name"));
    }
    Ok(())
}

pub(super) fn validate_name(name: &str) -> Result<(), Error> {
    let valid = !name.is_empty()
        && name.len() <= 64
        && name
            .bytes()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-' || c == b'_');
    if !valid || reserved(name) {
        return Err(invalid("Skill identifiers must be 1–64 lowercase ASCII letters, digits, '-' or '_', and not reserved filesystem names"));
    }
    Ok(())
}
pub(super) fn reserved(name: &str) -> bool {
    matches!(name, "con" | "prn" | "aux" | "nul" | "conin$" | "conout$")
        || numbered_device(name, "com")
        || numbered_device(name, "lpt")
}
fn numbered_device(name: &str, prefix: &str) -> bool {
    name.strip_prefix(prefix).is_some_and(|suffix| {
        matches!(
            suffix,
            "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "¹" | "²" | "³"
        )
    })
}

pub(super) fn user_name(name: &str) -> Result<(), Error> {
    validate_name(name)?;
    if name.eq_ignore_ascii_case("core") {
        return Err(invalid("Bundled core skill is protected"));
    }
    Ok(())
}

pub(super) fn description(metadata: &Metadata, content: &str) -> String {
    metadata.description.clone().unwrap_or_else(|| {
        content
            .lines()
            .find_map(|line| line.strip_prefix("# "))
            .unwrap_or("User workflow instructions")
            .to_string()
    })
}

fn optional_string<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<String>, D::Error> {
    String::deserialize(deserializer).map(Some)
}
fn frontmatter(rest: &str) -> Result<(&str, &str), Error> {
    let mut offset = 0;
    for line in rest.split_inclusive('\n') {
        let marker = line.trim_end_matches('\n');
        if marker == "---" || marker == "..." {
            return Ok((&rest[..offset], &rest[offset + line.len()..]));
        }
        offset += line.len();
    }
    Err(invalid("Unterminated skill frontmatter"))
}

fn validate_shape(yaml: &str) -> Result<(), Error> {
    let value: serde_yaml::Value = serde_yaml::from_str(yaml)
        .map_err(|_| invalid("Invalid or duplicate skill frontmatter"))?;
    let fields = value
        .as_mapping()
        .ok_or_else(|| invalid("Skill frontmatter must be a mapping"))?;
    validate_string_fields(fields)?;
    if let Some(value) = fields.get(serde_yaml::Value::String("aperture".into())) {
        validate_api_shape(value)?;
    }
    Ok(())
}
fn validate_api_shape(value: &serde_yaml::Value) -> Result<(), Error> {
    let fields = value
        .as_mapping()
        .ok_or_else(|| invalid("aperture metadata must be a mapping"))?;
    let Some(value) = fields.get(serde_yaml::Value::String("required_apis".into())) else {
        return Ok(());
    };
    let apis = value
        .as_sequence()
        .ok_or_else(|| invalid("required_apis must be an array of strings"))?;
    if apis.iter().any(|api| !api.is_string()) {
        return Err(invalid("required_apis must contain only strings"));
    }

    Ok(())
}

fn validate_string_fields(fields: &serde_yaml::Mapping) -> Result<(), Error> {
    for key in ["name", "description"] {
        if fields
            .get(serde_yaml::Value::String(key.into()))
            .is_some_and(|value| !value.is_string())
        {
            return Err(invalid("Skill name/description must be strings"));
        }
    }
    Ok(())
}
