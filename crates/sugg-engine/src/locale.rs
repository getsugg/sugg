use sys_locale::get_locale;

/// 检测系统当前最偏好的 locale（BCP47 标签，如 `en-US`、`zh-CN`）。
///
/// 通过 [`sys_locale::get_locale`] 在各平台读取相应环境：
/// - Windows: `GetUserDefaultLocaleName`
/// - macOS / iOS: `CFLocaleCopyCurrent`
/// - Linux/BSD: `LANG` / `LC_ALL` 等环境变量
///
/// 若检测失败（极少数情况下）则回退到 `"en"`，以保证 i18n 回退链始终有起点。
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
