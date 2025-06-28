use crate::cache::models::CachedSpec;
use crate::error::Error;
use crate::fs::{FileSystem, OsFileSystem};
use openapiv3::{OpenAPI, Operation, Parameter, ReferenceOr, RequestBody, SecurityScheme};
use std::path::{Path, PathBuf};
use std::process::Command;

pub struct ConfigManager<F: FileSystem> {
    fs: F,
    config_dir: PathBuf,
}

impl ConfigManager<OsFileSystem> {
    /// Creates a new `ConfigManager` with the default filesystem and config directory.
    ///
    /// # Errors
    ///
    /// Returns an error if the home directory cannot be determined.
    pub fn new() -> Result<Self, Error> {
        let config_dir = get_config_dir()?;
        Ok(Self {
            fs: OsFileSystem,
            config_dir,
        })
    }
}

impl<F: FileSystem> ConfigManager<F> {
    pub const fn with_fs(fs: F, config_dir: PathBuf) -> Self {
        Self { fs, config_dir }
    }

    /// Adds a new `OpenAPI` specification to the configuration.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The spec already exists and `force` is false
    /// - File I/O operations fail
    /// - The `OpenAPI` spec is invalid YAML
    /// - The spec contains unsupported features
    ///
    /// # Panics
    ///
    /// Panics if the spec path parent directory is None (should not happen in normal usage).
    pub fn add_spec(&self, name: &str, file_path: &Path, force: bool) -> Result<(), Error> {
        let spec_path = self.config_dir.join("specs").join(format!("{name}.yaml"));
        let cache_path = self.config_dir.join(".cache").join(format!("{name}.bin"));

        if self.fs.exists(&spec_path) && !force {
            return Err(Error::Config(format!(
                "Spec '{name}' already exists. Use --force to overwrite."
            )));
        }

        let content = self.fs.read_to_string(file_path)?;
        let openapi_spec: OpenAPI = serde_yaml::from_str(&content)?;

        // Validate against Aperture's supported feature set (SDD §5)
        Self::validate_spec(&openapi_spec)?;

        // Transform into internal cached representation
        let cached_spec = Self::transform_to_cached_spec(name, &openapi_spec);

        // Create directories
        self.fs.create_dir_all(spec_path.parent().unwrap())?;
        self.fs.create_dir_all(cache_path.parent().unwrap())?;

        // Write original spec file
        self.fs.write_all(&spec_path, content.as_bytes())?;

        // Serialize and write cached representation
        let cached_data = bincode::serialize(&cached_spec)
            .map_err(|e| Error::Config(format!("Failed to serialize cached spec: {e}")))?;
        self.fs.write_all(&cache_path, &cached_data)?;

        Ok(())
    }

    /// Lists all registered API contexts.
    ///
    /// # Errors
    ///
    /// Returns an error if the specs directory cannot be read.
    pub fn list_specs(&self) -> Result<Vec<String>, Error> {
        let specs_dir = self.config_dir.join("specs");
        if !self.fs.exists(&specs_dir) {
            return Ok(Vec::new());
        }

        let mut specs = Vec::new();
        for entry in self.fs.read_dir(&specs_dir)? {
            if self.fs.is_file(&entry) {
                if let Some(file_name) = entry.file_name().and_then(|s| s.to_str()) {
                    if std::path::Path::new(file_name)
                        .extension()
                        .is_some_and(|ext| ext.eq_ignore_ascii_case("yaml"))
                    {
                        specs.push(file_name.trim_end_matches(".yaml").to_string());
                    }
                }
            }
        }
        Ok(specs)
    }

    /// Removes an API specification from the configuration.
    ///
    /// # Errors
    ///
    /// Returns an error if the spec does not exist or cannot be removed.
    pub fn remove_spec(&self, name: &str) -> Result<(), Error> {
        let spec_path = self.config_dir.join("specs").join(format!("{name}.yaml"));
        let cache_path = self.config_dir.join(".cache").join(format!("{name}.bin"));

        if !self.fs.exists(&spec_path) {
            return Err(Error::Config(format!("Spec '{name}' does not exist.")));
        }

        self.fs.remove_file(&spec_path)?;
        if self.fs.exists(&cache_path) {
            self.fs.remove_file(&cache_path)?;
        }

        Ok(())
    }

    /// Opens an API specification in the default editor.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The spec does not exist.
    /// - The `$EDITOR` environment variable is not set.
    /// - The editor command fails to execute.
    pub fn edit_spec(&self, name: &str) -> Result<(), Error> {
        let spec_path = self.config_dir.join("specs").join(format!("{name}.yaml"));

        if !self.fs.exists(&spec_path) {
            return Err(Error::Config(format!("Spec '{name}' does not exist.")));
        }

        let editor = std::env::var("EDITOR")
            .map_err(|_| Error::Config("EDITOR environment variable not set.".to_string()))?;

        Command::new(editor)
            .arg(&spec_path)
            .status()
            .map_err(Error::Io)?
            .success()
            .then_some(()) // Convert bool to Option<()>
            .ok_or_else(|| Error::Config(format!("Editor command failed for spec '{name}'.")))
    }

    /// Validates an `OpenAPI` specification against Aperture's supported features.
    ///
    /// # Errors
    ///
    /// Returns an error if the spec contains unsupported features as defined in SDD §5.
    fn validate_spec(spec: &OpenAPI) -> Result<(), Error> {
        // Validate security schemes
        if let Some(components) = &spec.components {
            for (name, scheme_ref) in &components.security_schemes {
                match scheme_ref {
                    ReferenceOr::Item(scheme) => {
                        Self::validate_security_scheme(name, scheme)?;
                    }
                    ReferenceOr::Reference { .. } => {
                        return Err(Error::Config(format!(
                            "Security scheme references are not supported: '{name}'"
                        )));
                    }
                }
            }
        }

        // Validate operations
        for (path, path_item_ref) in &spec.paths.paths {
            if let ReferenceOr::Item(path_item) = path_item_ref {
                let operations = [
                    ("get", &path_item.get),
                    ("post", &path_item.post),
                    ("put", &path_item.put),
                    ("delete", &path_item.delete),
                    ("patch", &path_item.patch),
                    ("head", &path_item.head),
                    ("options", &path_item.options),
                    ("trace", &path_item.trace),
                ];

                for (method, operation_opt) in operations {
                    if let Some(operation) = operation_opt {
                        Self::validate_operation(path, method, operation)?;
                    }
                }
            }
        }

        Ok(())
    }

    /// Validates a security scheme against Aperture's supported types.
    fn validate_security_scheme(name: &str, scheme: &SecurityScheme) -> Result<(), Error> {
        match scheme {
            SecurityScheme::APIKey { .. } => Ok(()),
            SecurityScheme::HTTP {
                scheme: http_scheme,
                ..
            } => {
                if http_scheme == "bearer" {
                    Ok(())
                } else {
                    Err(Error::Config(format!(
                        "Unsupported HTTP scheme '{http_scheme}' in security scheme '{name}'. Only 'bearer' is supported."
                    )))
                }
            }
            SecurityScheme::OAuth2 { .. } => Err(Error::Config(format!(
                "OAuth2 security scheme '{name}' is not supported in v1.0."
            ))),
            SecurityScheme::OpenIDConnect { .. } => Err(Error::Config(format!(
                "OpenID Connect security scheme '{name}' is not supported in v1.0."
            ))),
        }
    }

    /// Validates an operation against Aperture's supported features.
    fn validate_operation(path: &str, method: &str, operation: &Operation) -> Result<(), Error> {
        // Validate parameters
        for param_ref in &operation.parameters {
            match param_ref {
                ReferenceOr::Item(param) => {
                    Self::validate_parameter(path, method, param)?;
                }
                ReferenceOr::Reference { .. } => {
                    return Err(Error::Config(format!(
                        "Parameter references are not supported in {method} {path}"
                    )));
                }
            }
        }

        // Validate request body
        if let Some(request_body_ref) = &operation.request_body {
            match request_body_ref {
                ReferenceOr::Item(request_body) => {
                    Self::validate_request_body(path, method, request_body)?;
                }
                ReferenceOr::Reference { .. } => {
                    return Err(Error::Config(format!(
                        "Request body references are not supported in {method} {path}."
                    )));
                }
            }
        }

        Ok(())
    }

    /// Validates a parameter against Aperture's supported features.
    fn validate_parameter(path: &str, method: &str, param: &Parameter) -> Result<(), Error> {
        let param_data = match param {
            Parameter::Query { parameter_data, .. }
            | Parameter::Header { parameter_data, .. }
            | Parameter::Path { parameter_data, .. }
            | Parameter::Cookie { parameter_data, .. } => parameter_data,
        };

        match &param_data.format {
            openapiv3::ParameterSchemaOrContent::Schema(_) => Ok(()),
            openapiv3::ParameterSchemaOrContent::Content(_) => {
                Err(Error::Config(format!(
                    "Parameter '{}' in {method} {path} uses unsupported content-based serialization. Only schema-based parameters are supported.",
                    param_data.name
                )))
            }
        }
    }

    /// Validates a request body against Aperture's supported features.
    fn validate_request_body(
        path: &str,
        method: &str,
        request_body: &RequestBody,
    ) -> Result<(), Error> {
        // Check for unsupported content types first
        for (content_type, _) in &request_body.content {
            if content_type != "application/json" {
                return Err(Error::Config(format!(
                    "Unsupported request body content type '{content_type}' in {method} {path}. Only 'application/json' is supported in v1.0."
                )));
            }
        }

        // If we get here, all content types are application/json
        Ok(())
    }

    /// Transforms an `OpenAPI` specification into Aperture's cached representation.
    fn transform_to_cached_spec(name: &str, spec: &OpenAPI) -> CachedSpec {
        let mut commands = Vec::new();

        // Extract version from info
        let version = spec.info.version.clone();

        // Process all operations
        for (path, path_item_ref) in &spec.paths.paths {
            if let ReferenceOr::Item(path_item) = path_item_ref {
                let operations = [
                    ("get", &path_item.get),
                    ("post", &path_item.post),
                    ("put", &path_item.put),
                    ("delete", &path_item.delete),
                    ("patch", &path_item.patch),
                    ("head", &path_item.head),
                    ("options", &path_item.options),
                    ("trace", &path_item.trace),
                ];

                for (method, operation_opt) in operations {
                    if let Some(operation) = operation_opt {
                        let cached_command = Self::transform_operation(path, method, operation);
                        commands.push(cached_command);
                    }
                }
            }
        }

        CachedSpec {
            name: name.to_string(),
            version,
            commands,
        }
    }

    /// Transforms an `OpenAPI` operation into a cached command.
    fn transform_operation(
        path: &str,
        method: &str,
        operation: &Operation,
    ) -> crate::cache::models::CachedCommand {
        use crate::cache::models::{
            CachedCommand, CachedParameter, CachedRequestBody, CachedResponse,
        };

        // Generate command name from operationId or fallback to method
        let operation_id = operation
            .operation_id
            .clone()
            .unwrap_or_else(|| method.to_string());

        // Convert to kebab-case for command name
        let command_name = operation_id
            .chars()
            .enumerate()
            .map(|(i, c)| {
                if c.is_uppercase() && i > 0 {
                    format!("-{}", c.to_lowercase())
                } else {
                    c.to_lowercase().to_string()
                }
            })
            .collect::<String>();

        // Transform parameters
        let mut parameters = Vec::new();
        for param_ref in &operation.parameters {
            if let ReferenceOr::Item(param) = param_ref {
                let (param_data, location_str) = match param {
                    Parameter::Query { parameter_data, .. } => (parameter_data, "query"),
                    Parameter::Header { parameter_data, .. } => (parameter_data, "header"),
                    Parameter::Path { parameter_data, .. } => (parameter_data, "path"),
                    Parameter::Cookie { parameter_data, .. } => (parameter_data, "cookie"),
                };

                parameters.push(CachedParameter {
                    name: param_data.name.clone(),
                    location: location_str.to_string(),
                    required: param_data.required,
                    schema: None, // Simplified for now
                });
            }
        }

        // Transform request body
        let request_body =
            operation
                .request_body
                .as_ref()
                .and_then(|req_body_ref| match req_body_ref {
                    ReferenceOr::Item(req_body) => Some(CachedRequestBody {
                        content: serde_json::Value::Object(serde_json::Map::new()), // Simplified
                        required: req_body.required,
                    }),
                    ReferenceOr::Reference { .. } => None,
                });

        // Transform responses
        let mut responses = Vec::new();
        for (status, _response_ref) in &operation.responses.responses {
            responses.push(CachedResponse {
                status_code: status.to_string(),
                content: None, // Simplified for now
            });
        }

        CachedCommand {
            name: command_name,
            description: operation.summary.clone(),
            operation_id,
            method: method.to_uppercase(),
            path: path.to_string(),
            parameters,
            request_body,
            responses,
        }
    }
}

/// Gets the default configuration directory path.
///
/// # Errors
///
/// Returns an error if the home directory cannot be determined.
pub fn get_config_dir() -> Result<PathBuf, Error> {
    let home_dir = dirs::home_dir()
        .ok_or_else(|| Error::Config("Could not determine home directory.".to_string()))?;
    let config_dir = home_dir.join(".config").join("aperture");
    Ok(config_dir)
}
