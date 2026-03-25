use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SmartQuery {
    pub subject: String,
    pub relation: Relation,
    pub support: Vec<String>,
    pub kind: Option<String>,
    pub path_hint: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Relation {
    Defined,
    CalledFrom,
    TriggeredFrom,
    Rendered,
    Populated,
    ComesFrom,
    Handled,
    Implementation,
    Custom(String),
}

impl Relation {
    pub fn parse(value: &str) -> Self {
        match normalize_key(value).as_str() {
            "defined" | "definition" => Self::Defined,
            "called_from" | "called-from" | "calledfrom" | "callers" => Self::CalledFrom,
            "triggered_from" | "triggered-from" | "triggeredfrom" => Self::TriggeredFrom,
            "rendered" | "render" | "drawn" => Self::Rendered,
            "populated" | "populate" | "set" | "assigned" => Self::Populated,
            "comes_from" | "comes-from" | "source" | "origin" => Self::ComesFrom,
            "handled" | "handler" | "handles" => Self::Handled,
            "implementation" | "implemented" => Self::Implementation,
            other => Self::Custom(other.to_string()),
        }
    }

    pub fn as_str(&self) -> &str {
        match self {
            Self::Defined => "defined",
            Self::CalledFrom => "called_from",
            Self::TriggeredFrom => "triggered_from",
            Self::Rendered => "rendered",
            Self::Populated => "populated",
            Self::ComesFrom => "comes_from",
            Self::Handled => "handled",
            Self::Implementation => "implementation",
            Self::Custom(value) => value.as_str(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParseError {
    MissingSubject,
    MissingRelation,
    InvalidTerm(String),
    DuplicateKey(&'static str),
    EmptyValue(String),
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingSubject => {
                write!(f, "smart query is missing required subject:<value> term")
            }
            Self::MissingRelation => {
                write!(f, "smart query is missing required relation:<value> term")
            }
            Self::InvalidTerm(term) => write!(
                f,
                "invalid smart DSL term: {term} (expected key:value or key=value)"
            ),
            Self::DuplicateKey(key) => write!(f, "duplicate smart DSL key: {key}"),
            Self::EmptyValue(key) => write!(f, "smart DSL key has empty value: {key}"),
        }
    }
}

impl std::error::Error for ParseError {}

pub fn parse_smart_query<I, S>(terms: I) -> Result<SmartQuery, ParseError>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let mut subject = None;
    let mut relation = None;
    let mut support = Vec::new();
    let mut kind = None;
    let mut path_hint = None;

    for raw_term in terms {
        let raw_term = raw_term.as_ref().trim();
        if raw_term.is_empty() {
            continue;
        }

        let Some((raw_key, raw_value)) = split_term(raw_term) else {
            return Err(ParseError::InvalidTerm(raw_term.to_string()));
        };

        let key = normalize_key(raw_key);
        let value = raw_value.trim();
        if value.is_empty() {
            return Err(ParseError::EmptyValue(key));
        }

        match key.as_str() {
            "subject" => {
                if subject.is_some() {
                    return Err(ParseError::DuplicateKey("subject"));
                }
                subject = Some(value.to_string());
            }
            "relation" => {
                if relation.is_some() {
                    return Err(ParseError::DuplicateKey("relation"));
                }
                relation = Some(Relation::parse(value));
            }
            "support" => support.push(value.to_string()),
            "kind" => {
                if kind.is_some() {
                    return Err(ParseError::DuplicateKey("kind"));
                }
                kind = Some(value.to_string());
            }
            "path" | "path_hint" | "pathhint" => {
                if path_hint.is_some() {
                    return Err(ParseError::DuplicateKey("path_hint"));
                }
                path_hint = Some(value.to_string());
            }
            _ => return Err(ParseError::InvalidTerm(raw_term.to_string())),
        }
    }

    Ok(SmartQuery {
        subject: subject.ok_or(ParseError::MissingSubject)?,
        relation: relation.ok_or(ParseError::MissingRelation)?,
        support,
        kind,
        path_hint,
    })
}

fn split_term(term: &str) -> Option<(&str, &str)> {
    term.split_once(':').or_else(|| term.split_once('='))
}

fn normalize_key(key: &str) -> String {
    key.trim().to_ascii_lowercase().replace('-', "_")
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn parses_basic_colon_dsl() {
        let query = parse_smart_query([
            "subject:auth_status",
            "relation:rendered",
            "support:ui",
            "support:status",
        ])
        .unwrap();

        assert_eq!(
            query,
            SmartQuery {
                subject: "auth_status".to_string(),
                relation: Relation::Rendered,
                support: vec!["ui".to_string(), "status".to_string()],
                kind: None,
                path_hint: None,
            }
        );
    }

    #[test]
    fn parses_equals_dsl_and_optional_fields() {
        let query = parse_smart_query([
            "subject=provider_name",
            "relation=comes_from",
            "kind=code",
            "path=src/provider",
        ])
        .unwrap();

        assert_eq!(query.subject, "provider_name");
        assert_eq!(query.relation, Relation::ComesFrom);
        assert_eq!(query.kind.as_deref(), Some("code"));
        assert_eq!(query.path_hint.as_deref(), Some("src/provider"));
    }

    #[test]
    fn rejects_missing_subject() {
        let err = parse_smart_query(["relation:rendered"]).unwrap_err();
        assert_eq!(err, ParseError::MissingSubject);
    }

    #[test]
    fn rejects_missing_relation() {
        let err = parse_smart_query(["subject:auth_status"]).unwrap_err();
        assert_eq!(err, ParseError::MissingRelation);
    }

    #[test]
    fn rejects_unknown_keys() {
        let err =
            parse_smart_query(["subject:auth_status", "relation:rendered", "foo:bar"]).unwrap_err();
        assert_eq!(err, ParseError::InvalidTerm("foo:bar".to_string()));
    }
}
