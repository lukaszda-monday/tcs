use std::path::Path;
use tokei::{Config, LanguageType, Languages};

/// Languages that don't represent actual source code.
const IGNORE_LANGS: &[LanguageType] = &[
    LanguageType::Json,
    LanguageType::Yaml,
    LanguageType::Toml,
    LanguageType::Markdown,
    LanguageType::Text,
    LanguageType::Css,
    LanguageType::Svg,
];

/// Detect the primary language in a directory using tokei.
/// Respects .gitignore. Excludes data/config formats (JSON, YAML, etc.).
pub fn detect(dir: &Path) -> Option<String> {
    let config = Config::default();
    let mut languages = Languages::new();
    languages.get_statistics(&[dir], &[], &config);

    languages
        .into_iter()
        .filter(|(lt, lang)| !lang.is_empty() && !IGNORE_LANGS.contains(lt))
        .max_by_key(|(_, lang)| lang.code)
        .map(|(lang_type, _)| lang_type.name().to_lowercase())
}
