use sugg::{build_bundles, path_to_slash};
use std::path::Path;

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
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/completions")
}

fn normalize(s: String, dir: &Path) -> String {
    s.replace(&path_to_slash(dir), "<COMPLETIONS_DIR>")
}

#[tokio::test]
async fn test_bundle_greet_en() {
    let dir = fixture_completions_dir();
    let (static_js, _) = build_bundles(&dir, "en").await;
    bundle_snapshot!(normalize(static_js, &dir));
}

#[tokio::test]
async fn test_bundle_greet_zh() {
    let dir = fixture_completions_dir();
    let (static_js, _) = build_bundles(&dir, "zh").await;
    bundle_snapshot!(normalize(static_js, &dir));
}

#[tokio::test]
async fn test_bundle_dynamic_en() {
    let dir = fixture_completions_dir();
    let (_, dynamic_bundles) = build_bundles(&dir, "en").await;
    let combined = combine_dynamic_bundles(dynamic_bundles);
    bundle_snapshot!(normalize(combined, &dir));
}

#[tokio::test]
async fn test_bundle_dynamic_zh() {
    let dir = fixture_completions_dir();
    let (_, dynamic_bundles) = build_bundles(&dir, "zh").await;
    let combined = combine_dynamic_bundles(dynamic_bundles);
    bundle_snapshot!(normalize(combined, &dir));
}

/// 验证 docker dynamic bundle 产物（含 i18n.docker 命名空间翻译）
#[tokio::test]
async fn test_bundle_docker_dynamic() {
    let dir = fixture_completions_dir();
    let (_, dynamic_bundles) = build_bundles(&dir, "en").await;
    let docker = dynamic_bundles.into_iter().find(|(stem, _, _)| stem == "docker").map(|(_, js, _)| js).unwrap_or_default();
    bundle_snapshot!(normalize(docker, &dir));
}
