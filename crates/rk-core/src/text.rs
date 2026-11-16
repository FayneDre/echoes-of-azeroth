use serde::{Deserialize, Serialize};

/// User-facing text from the catalog. English is mandatory and is the fallback.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LocalizedText {
    pub en: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fr: Option<String>,
}

impl LocalizedText {
    /// Returns the text for `locale` (e.g. `"fr"`, `"fr-FR"`), falling back to English.
    pub fn get(&self, locale: &str) -> &str {
        let language = locale.split(['-', '_']).next().unwrap_or(locale);
        match language {
            "fr" => self.fr.as_deref().unwrap_or(&self.en),
            _ => &self.en,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn falls_back_to_english() {
        let text = LocalizedText {
            en: "Hello".into(),
            fr: None,
        };
        assert_eq!(text.get("fr"), "Hello");
        let text = LocalizedText {
            en: "Hello".into(),
            fr: Some("Bonjour".into()),
        };
        assert_eq!(text.get("fr-FR"), "Bonjour");
        assert_eq!(text.get("de"), "Hello");
    }
}
