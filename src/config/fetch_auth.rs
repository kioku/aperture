//! Environment-only specification download authentication, independent of API security schemes.
use crate::error::Error;
use base64::Engine;
use reqwest::header::{HeaderName, HeaderValue, AUTHORIZATION};
use reqwest::Url;
use serde::{Deserialize, Serialize};

/// Explicit selection for a download. Omission permits same-origin reference reuse.
#[derive(Debug, Clone, Copy, clap::ValueEnum, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum FetchMethod {
    None,
    Basic,
    Bearer,
    Header,
}

/// Only references are persisted; resolved values never belong in this structure.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct FetchAuth {
    pub method: FetchMethod,
    pub env_var: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub header_name: Option<String>,
    pub origin: String,
}

fn invalid(message: &str) -> Error {
    Error::invalid_config(format!("Specification fetch authentication: {message}"))
}

fn target(url: &str) -> Result<Url, Error> {
    let parsed = Url::parse(url).map_err(|_| invalid("invalid URL"))?;
    if parsed.scheme() != "https" || !parsed.username().is_empty() || parsed.password().is_some() {
        return Err(invalid("requires HTTPS without URL userinfo"));
    }
    Ok(parsed)
}

fn validate_env(name: &str) -> Result<(), Error> {
    let mut chars = name.chars();
    if !chars
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
    {
        return Err(invalid("invalid environment-variable name"));
    }
    if !chars.all(|c| c.is_ascii_alphanumeric() || c == '_') {
        return Err(invalid("invalid environment-variable name"));
    }
    Ok(())
}

fn custom_header(name: &str) -> Result<HeaderName, Error> {
    let header =
        HeaderName::from_bytes(name.as_bytes()).map_err(|_| invalid("invalid header name"))?;
    match header.as_str() {
        "authorization"
        | "proxy-authorization"
        | "host"
        | "content-length"
        | "transfer-encoding"
        | "connection"
        | "trailer"
        | "te"
        | "upgrade"
        | "keep-alive"
        | "proxy-connection" => Err(invalid(
            "reserved header; use Basic/Bearer for Authorization",
        )),
        _ => Ok(header),
    }
}

impl FetchAuth {
    /// Validate a reference before resolving secrets or making requests.
    ///
    /// # Errors
    /// Rejects invalid method/flag combinations, environment names, headers or HTTPS targets.
    pub fn new(
        method: FetchMethod,
        env_var: &str,
        header_name: Option<&str>,
        url: &str,
    ) -> Result<Self, Error> {
        validate_env(env_var)?;
        match (method, header_name) {
            (FetchMethod::Header, Some(name)) => {
                custom_header(name)?;
            }
            (FetchMethod::Basic | FetchMethod::Bearer, None) => {}
            _ => return Err(invalid("method and header flags conflict")),
        }
        Ok(Self {
            method,
            env_var: env_var.into(),
            header_name: header_name.map(str::to_owned),
            origin: target(url)?.origin().ascii_serialization(),
        })
    }

    /// Every authenticated redirect must retain the effective HTTPS origin.
    ///
    /// # Errors
    /// Rejects userinfo, non-HTTPS URLs and changed origins.
    pub fn validate_target(&self, url: &str) -> Result<(), Error> {
        if target(url)?.origin().ascii_serialization() != self.origin {
            return Err(invalid("target must retain the bound HTTPS origin; select authentication explicitly for a new origin"));
        }
        Ok(())
    }

    /// Resolve afresh on every download; diagnostics contain no resolved values.
    ///
    /// # Errors
    /// Rejects missing, non-UTF-8, empty or malformed environment credentials.
    pub fn resolve(&self) -> Result<(HeaderName, HeaderValue), Error> {
        validate_env(&self.env_var)?;
        let value = std::env::var(&self.env_var)
            .map_err(|_| invalid("environment variable is missing or not UTF-8"))?;
        self.header_for_value(&value)
    }

    /// Build a sensitive header from a synthetic or environment-supplied value.
    ///
    /// # Errors
    /// Rejects malformed credentials or a reference with an invalid authentication method.
    pub fn header_for_value(&self, value: &str) -> Result<(HeaderName, HeaderValue), Error> {
        if value.trim().is_empty() {
            return Err(invalid("environment credential is empty"));
        }
        // Basic encoding must not hide invalid control bytes in the supplied credentials.
        if value.bytes().any(|byte| byte.is_ascii_control()) {
            return Err(invalid("credential contains control bytes"));
        }
        HeaderValue::from_str(value).map_err(|_| invalid("invalid credential header value"))?;
        let (name, encoded) = self.encode(value)?;
        let mut header = HeaderValue::from_str(&encoded)
            .map_err(|_| invalid("invalid credential header value"))?;
        header.set_sensitive(true);
        Ok((name, header))
    }

    fn encode(&self, value: &str) -> Result<(HeaderName, String), Error> {
        match self.method {
            FetchMethod::Basic => basic(value),
            FetchMethod::Bearer => bearer(value),
            FetchMethod::Header => Ok((
                custom_header(
                    self.header_name
                        .as_deref()
                        .ok_or_else(|| invalid("missing header name"))?,
                )?,
                value.to_owned(),
            )),
            FetchMethod::None => Err(invalid("no-auth cannot contain a credential reference")),
        }
    }
}

/// CLI flags shared by the canonical command and its compatibility alias.
#[derive(Debug, Clone, Default, clap::Args)]
pub struct FetchAuthArgs {
    /// Download authentication only; none clears a saved reference on successful replacement.
    #[arg(long, value_enum)]
    pub fetch_auth: Option<FetchMethod>,
    /// Environment variable containing username:password, a raw token, or a header value.
    #[arg(long, requires = "fetch_auth")]
    pub fetch_auth_env: Option<String>,
    /// Custom download header; transport headers and Authorization are forbidden.
    #[arg(long, requires = "fetch_auth")]
    pub fetch_header_name: Option<String>,
}

impl FetchAuthArgs {
    fn validate_flags(&self) -> Result<(), Error> {
        if matches!(self.fetch_auth, None | Some(FetchMethod::None))
            && (self.fetch_auth_env.is_some() || self.fetch_header_name.is_some())
        {
            return Err(invalid(
                "credential flags require Basic, Bearer or Header selection",
            ));
        }
        Ok(())
    }

    /// Resolve explicit choice or reuse only a saved reference with the same HTTPS origin.
    ///
    /// # Errors
    /// Rejects conflicting flags, invalid references or saved-reference origin changes.
    pub fn select(&self, url: &str, saved: Option<&FetchAuth>) -> Result<Option<FetchAuth>, Error> {
        self.validate_flags()?;
        match self.fetch_auth {
            Some(FetchMethod::None) => Ok(None),
            Some(method) => Ok(Some(FetchAuth::new(
                method,
                self.fetch_auth_env
                    .as_deref()
                    .ok_or_else(|| invalid("missing --fetch-auth-env"))?,
                self.fetch_header_name.as_deref(),
                url,
            )?)),
            None => reuse(url, saved),
        }
    }
}

fn basic(value: &str) -> Result<(HeaderName, String), Error> {
    let (username, password) = value
        .split_once(':')
        .ok_or_else(|| invalid("Basic requires username:password"))?;
    if username.is_empty() || password.is_empty() {
        return Err(invalid("Basic username or password is empty"));
    }
    Ok((
        AUTHORIZATION,
        format!(
            "Basic {}",
            base64::engine::general_purpose::STANDARD.encode(value)
        ),
    ))
}

fn bearer(value: &str) -> Result<(HeaderName, String), Error> {
    if !value
        .bytes()
        .all(|c| c.is_ascii_alphanumeric() || b"-._~+/=".contains(&c))
    {
        return Err(invalid("Bearer requires a raw token"));
    }
    Ok((AUTHORIZATION, format!("Bearer {value}")))
}

fn reuse(url: &str, saved: Option<&FetchAuth>) -> Result<Option<FetchAuth>, Error> {
    if let Some(auth) = saved {
        auth.validate_target(url)?;
    }
    Ok(saved.cloned())
}
