use sys_locale::get_locale;

/// Detect the system currently preferred locale (BCP47 tag, e.g. `en-US`, `zh-CN`).
///
/// Through [`sys_locale::get_locale`], reads the matching environment on each platform:
/// - Windows: `GetUserDefaultLocaleName`
/// - macOS / iOS: `CFLocaleCopyCurrent`
/// - Linux/BSD: environment variables such as `LANG` / `LC_ALL`
///
/// If detection fails (rare), falls back to `"en"` so the i18n fallback chain always has a starting point.
pub fn detect_locale() -> String {
    get_locale()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "en".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_locale_returns_non_empty() {
        let lang = detect_locale();
        assert!(!lang.is_empty(), "detect_locale() must never return empty");
    }

    #[test]
    fn detect_locale_matches_sys_locale_when_available() {
        if let Some(sys) = get_locale()
            && !sys.is_empty()
        {
            assert_eq!(detect_locale(), sys);
        }
    }
}
