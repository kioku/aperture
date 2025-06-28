use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct CachedSpec {
    pub name: String,
    pub version: String,
    pub commands: Vec<CachedCommand>,
}

#[derive(Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct CachedCommand {
    pub name: String,
    pub description: Option<String>,
    pub operation_id: String,
    pub method: String,
    pub path: String,
    pub parameters: Vec<CachedParameter>,
    pub request_body: Option<CachedRequestBody>,
    pub responses: Vec<CachedResponse>,
}

#[derive(Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct CachedParameter {
    pub name: String,
    pub location: String,
    pub required: bool,
    pub schema: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct CachedRequestBody {
    pub content: String,
    pub required: bool,
}

#[derive(Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct CachedResponse {
    pub status_code: String,
    pub content: Option<String>,
}
