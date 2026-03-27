use std::path::Path;
use tokei::{Config, Languages};

/// Detect the primary language in a directory using tokei.
/// Returns the language name in lowercase (e.g. "rust", "javascript", "typescript").
pub fn detect(dir: &Path) -> Option<String> {
    let config = Config::default();
    let mut languages = Languages::new();
    languages.get_statistics(&[dir], &[], &config);

    // Find the language with the most code lines
    languages
        .into_iter()
        .filter(|(_, lang)| !lang.is_empty())
        .max_by_key(|(_, lang)| lang.code)
        .map(|(lang_type, _)| lang_type.name().to_lowercase())
}
