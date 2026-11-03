//! Exact replay of supplied JSON exchanges; no inferred responses or fallbacks.

use std::collections::{BTreeMap, BTreeSet};

use serde::Deserialize;
use serde_json::Value;
use wiremock::{Request, ResponseTemplate};

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Recording {
    pub host: String,
    source: String,
    exchanges: Vec<Exchange>,
}

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct Exchange {
    path: String,
    #[serde(default)]
    query: BTreeMap<String, String>,
    status: u16,
    body: Value,
    #[serde(default)]
    headers: BTreeMap<String, String>,
}

impl Recording {
    pub fn validate(&self) -> super::Result<()> {
        if !super::valid_host(&self.host)
            || self.source.trim().is_empty()
            || self.exchanges.is_empty()
        {
            return Err(
                "recordings need a SharePoint host, source description, and exchanges".into(),
            );
        }
        let mut requests = BTreeSet::new();
        for exchange in &self.exchanges {
            if !exchange.path.starts_with('/')
                || exchange.path.contains(['?', '#', '\r', '\n'])
                || !(200..=599).contains(&exchange.status)
                || !requests.insert((&exchange.path, &exchange.query))
            {
                return Err("invalid or duplicate recorded request".into());
            }
            for (name, value) in &exchange.headers {
                if !matches!(
                    name.to_ascii_lowercase().as_str(),
                    "content-type" | "retry-after"
                ) {
                    return Err(format!("recorded response header {name} is unsupported; keep only Content-Type/Retry-After").into());
                }
                reqwest::header::HeaderValue::from_str(value)?;
            }
        }
        Ok(())
    }

    pub fn respond(&self, request: &Request, endpoint: &str) -> ResponseTemplate {
        let mut query = BTreeMap::new();
        for (key, value) in request.url.query_pairs() {
            if query.insert(key.into_owned(), value.into_owned()).is_some() {
                return super::error(400, "Duplicate recorded query parameter.");
            }
        }
        let Some(exchange) = self
            .exchanges
            .iter()
            .find(|e| e.path == request.url.path() && e.query == query)
        else {
            return super::error(
                404,
                "No recorded exchange matches this exact path and query.",
            );
        };
        let mut body = exchange.body.clone();
        localize_continuations(&mut body, endpoint);
        let mut response = ResponseTemplate::new(exchange.status).set_body_json(body);
        for (name, value) in &exchange.headers {
            response = response.insert_header(name.as_str(), value.as_str());
        }
        response
    }
}

fn localize_continuations(body: &mut Value, endpoint: &str) {
    match body {
        Value::Object(fields) => {
            for (key, value) in fields {
                if matches!(
                    key.as_str(),
                    "odata.nextLink" | "@odata.nextLink" | "__next"
                ) {
                    if let Value::String(link) = value {
                        *link = link.replace("{{endpoint}}", endpoint);
                    }
                } else {
                    localize_continuations(value, endpoint);
                }
            }
        }
        Value::Array(values) => {
            for value in values {
                localize_continuations(value, endpoint);
            }
        }
        _ => {}
    }
}
