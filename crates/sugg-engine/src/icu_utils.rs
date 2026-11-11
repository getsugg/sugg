use icu::locale::Locale;
use icu::locale::fallback::LocaleFallbacker;

/// Generate a language fallback chain based on the BCP 47 spec (using the ICU4X standard implementation)
///
/// # Example
/// ```
/// // en is always the base fallback; "en-US" etc. return ["en", "en-US"]
/// assert_eq!(sugg_engine::get_fallback_chain("en"),        vec!["en"]);
/// assert_eq!(sugg_engine::get_fallback_chain("en-US"),     vec!["en", "en-US"]);
/// ```
pub fn get_fallback_chain(lang: &str) -> Vec<String> {
    let mut chain = vec!["en".to_string()];
    if lang.is_empty() || lang.eq_ignore_ascii_case("en") {
        return chain;
    }

    if let Ok(locale) = lang.parse::<Locale>() {
        let fallbacker = LocaleFallbacker::new();
        let mut iter = fallbacker
            .for_config(Default::default())
            .fallback_for(locale.into());

        let mut sequence = Vec::new();
        loop {
            let s = iter.get().to_string();
            if s == "und" {
                break;
            }
            if s != "en" {
                sequence.push(s);
            }
            iter.step();
        }
        sequence.reverse();
        for s in sequence {
            if !chain.contains(&s) {
                chain.push(s);
            }
        }
    } else {
        if !lang.eq_ignore_ascii_case("en") {
            chain.push(lang.to_string());
        }
    }

    chain
}

#[cfg(test)]
mod fallback_tests {
    use super::*;

    #[test]
    fn test_fallback_chain_edge_cases() {
        assert_eq!(get_fallback_chain("en"), vec!["en"]);
        assert_eq!(get_fallback_chain(""), vec!["en"]);
        let chain = get_fallback_chain("???");
        assert!(chain.contains(&"en".to_string()));
        assert!(chain.contains(&"???".to_string()));
        let en_prefix: Vec<_> = chain.iter().filter(|s| s.starts_with("en")).collect();
        assert_eq!(en_prefix, vec![&"en"], "must not add en-* entries");
    }

    #[test]
    fn test_fallback_chain_zh() {
        let chain = get_fallback_chain("zh-Hans-CN");
        assert!(chain.contains(&"en".to_string()));
        assert!(chain.contains(&"zh".to_string()));
        assert!(chain.contains(&"zh-CN".to_string()));
    }

    #[test]
    fn test_fallback_chain_fr() {
        let chain = get_fallback_chain("fr-FR");
        assert!(chain.contains(&"en".to_string()));
        assert!(chain.contains(&"fr".to_string()));
        assert!(chain.contains(&"fr-FR".to_string()));
    }
}
