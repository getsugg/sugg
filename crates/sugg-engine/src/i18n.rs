use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use sugg_core::log_warn;

pub fn run_i18n_gen(completions_dir: &Path, lang: &str) {
    if !completions_dir.exists() {
        fs::create_dir_all(completions_dir).expect("Failed to create completions directory");
    }

    let preferred_lang = lang;

    let fallbacks = sugg_engine::get_fallback_chain(preferred_lang);

    // keys_map: namespace -> key -> lang -> translation
    let mut keys_map: BTreeMap<String, BTreeMap<String, BTreeMap<String, String>>> =
        BTreeMap::new();

    for (ns, i18n_dir) in sugg_engine::scan_i18n_dirs(completions_dir) {
        let Ok(dir_entries) = fs::read_dir(&i18n_dir) else {
            continue;
        };
        for entry in dir_entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some("json") {
                let lang = path
                    .file_stem()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string();
                if let Ok(s) = fs::read_to_string(&path)
                    && let Ok(map) =
                        serde_json::from_str::<serde_json::Map<String, serde_json::Value>>(&s)
                {
                    for (k, v) in map {
                        let val_str = if let Some(s) = v.as_str() {
                            s.replace('\n', " ").replace("*/", "* /")
                        } else {
                            v.to_string()
                        };
                        keys_map
                            .entry(ns.clone())
                            .or_default()
                            .entry(k)
                            .or_default()
                            .insert(lang.clone(), val_str);
                    }
                }
            }
        }
    }

    fn find_best_lang<'a>(
        fallbacks: &[String],
        translations: &'a BTreeMap<String, String>,
    ) -> Option<&'a str> {
        for fb in fallbacks.iter().rev() {
            for (lang, _) in translations.iter() {
                if lang.eq_ignore_ascii_case(fb) {
                    return Some(lang.as_str());
                }
            }
        }
        None
    }

    let mut s = String::new();
    if keys_map.is_empty() {
        s.push_str("// No i18n keys found.\n");
    } else {
        for (ns, ns_keys) in &keys_map {
            if ns_keys.is_empty() {
                continue;
            }
            let module_path = format!("virtual:i18n/{}", ns);
            s.push_str(&format!("declare module \"{}\" {{\n", module_path));
            for (key, translations) in ns_keys {
                s.push_str("  /**\n");
                let best_lang = find_best_lang(&fallbacks, translations);
                if best_lang.is_none() {
                    log_warn!(
                        "i18n key '{}' in namespace '{}' has no translation for preferred language '{}' (fallback chain: {}). Available translations: {}",
                        key,
                        ns,
                        preferred_lang,
                        fallbacks.join(", "),
                        translations.keys().cloned().collect::<Vec<_>>().join(", ")
                    );
                }
                if let Some(bl) = best_lang
                    && let Some(text) = translations.get(bl)
                {
                    s.push_str(&format!("   * - 🚩 **{}**: {}\n", bl, text));
                }
                for (lang, text) in translations {
                    if Some(lang.as_str()) == best_lang {
                        continue;
                    }
                    s.push_str(&format!("   * - **{}**: {}\n", lang, text));
                }
                s.push_str("   */\n");
                s.push_str(&format!("  export const {}: string;\n", key));
            }
            s.push_str("}\n\n");
        }
    }

    let sugg_dir = completions_dir.join(".sugg");
    fs::create_dir_all(&sugg_dir).expect("Failed to create .sugg directory");
    let out_path = sugg_dir.join("i18n.d.ts");
    fs::write(&out_path, &s).expect("Failed to write i18n.d.ts");
    println!(
        "{} Generated {} with {} namespaces.",
        sugg_core::ICON_SUCCESS,
        sugg_core::path_to_slash(&out_path),
        keys_map.len()
    );
}
