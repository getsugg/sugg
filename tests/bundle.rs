use std::collections::HashMap;
use std::path::{Path, PathBuf};
use sugg_core::path_to_slash;
use sugg_engine::build_bundles;

/// 设置快照目录为 tests/snapshots/bundle/
macro_rules! bundle_snapshot {
    ($($tt:tt)*) => {{
        static INIT: std::sync::Once = std::sync::Once::new();
        INIT.call_once(|| {
            let mut settings = insta::Settings::new();
            settings.set_snapshot_path(
                std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("tests/snapshots/bundle"),
            );
            Box::leak(Box::new(settings.bind_to_scope()));
        });
        insta::assert_snapshot!($($tt)*)
    }};
}

/// 合并所有静态 bundle，按 stem 排序
fn combine_static_bundles(bundles: Vec<(String, String)>) -> String {
    let mut bundles = bundles
        .into_iter()
        .filter(|(_, js)| !js.trim().is_empty())
        .map(|(stem, js)| format!("// === {} ===\n{}", stem, js))
        .collect::<Vec<_>>();
    bundles.sort();
    bundles.join("\n")
}

/// 合并动态 bundle，跳过 JS 内容为空的条目
fn combine_dynamic_bundles(bundles: Vec<(String, String, Vec<String>)>) -> String {
    bundles
        .into_iter()
        .filter(|(_, js, _)| !js.trim().is_empty())
        .map(|(stem, js, _)| format!("// === {} ===\n{}", stem, js))
        .collect::<Vec<_>>()
        .join("\n")
}

fn fixture_completions_dir() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/completions")
}

fn normalize(s: String, dir: &Path) -> String {
    s.replace(&path_to_slash(dir), "<COMPLETIONS_DIR>")
}

fn empty_cache() -> HashMap<PathBuf, sugg_engine::CachedFile> {
    HashMap::new()
}

#[tokio::test]
async fn test_bundle_greet_en() {
    let dir = fixture_completions_dir();
    let mut cache = empty_cache();
    let (static_bundles, _) = build_bundles(&dir, "en", &mut cache).await.unwrap();
    let combined = combine_static_bundles(static_bundles);
    bundle_snapshot!(normalize(combined, &dir));
}

#[tokio::test]
async fn test_bundle_greet_zh() {
    let dir = fixture_completions_dir();
    let mut cache = empty_cache();
    let (static_bundles, _) = build_bundles(&dir, "zh", &mut cache).await.unwrap();
    let combined = combine_static_bundles(static_bundles);
    bundle_snapshot!(normalize(combined, &dir));
}

#[tokio::test]
async fn test_bundle_dynamic_en() {
    let dir = fixture_completions_dir();
    let mut cache = empty_cache();
    let (_, dynamic_bundles) = build_bundles(&dir, "en", &mut cache).await.unwrap();
    let combined = combine_dynamic_bundles(dynamic_bundles);
    bundle_snapshot!(normalize(combined, &dir));
}

#[tokio::test]
async fn test_bundle_dynamic_zh() {
    let dir = fixture_completions_dir();
    let mut cache = empty_cache();
    let (_, dynamic_bundles) = build_bundles(&dir, "zh", &mut cache).await.unwrap();
    let combined = combine_dynamic_bundles(dynamic_bundles);
    bundle_snapshot!(normalize(combined, &dir));
}

/// 验证 docker dynamic bundle 产物（含 i18n.docker 命名空间翻译）
#[tokio::test]
async fn test_bundle_docker_dynamic() {
    let dir = fixture_completions_dir();
    let mut cache = empty_cache();
    let (_, dynamic_bundles) = build_bundles(&dir, "en", &mut cache).await.unwrap();
    let docker = dynamic_bundles
        .into_iter()
        .find(|(stem, _, _)| stem == "docker")
        .map(|(_, js, _)| js)
        .unwrap_or_default();
    bundle_snapshot!(normalize(docker, &dir));
}
