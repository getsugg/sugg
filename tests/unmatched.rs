mod common;
use common::{complete, get_sandbox, normalize_items};

/// 快照宏，统一输出到 tests/snapshots/unmatched/
macro_rules! unmatched_snapshot {
    ($($tt:tt)*) => {{
        static INIT: std::sync::Once = std::sync::Once::new();
        INIT.call_once(|| {
            let mut settings = insta::Settings::new();
            settings.set_snapshot_path(
                std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("tests/snapshots/unmatched"),
            );
            Box::leak(Box::new(settings.bind_to_scope()));
        });
        insta::assert_json_snapshot!($($tt)*)
    }};
}

#[test]
fn test_unmatched_word_with_non_empty_prefix_filters_dynamic() {
    let sandbox = get_sandbox();
    let items = complete(
        "bun run djajd",
        &sandbox.project_dirs["bun"],
        &sandbox.cache_dir,
    );
    unmatched_snapshot!(items);
}

#[test]
fn test_valid_subcommand_with_trailing_space_returns_all_dynamic() {
    let sandbox = get_sandbox();
    let items = complete("bun run ", &sandbox.project_dirs["bun"], &sandbox.cache_dir);
    unmatched_snapshot!(
        "valid_subcommand_trailing_space",
        normalize_items(&items, &sandbox.project_dirs["bun"])
    );
}

#[test]
fn test_option_completion_still_works() {
    let sandbox = get_sandbox();
    let items = complete(
        "pnpm install -",
        &sandbox.project_dirs["minimal"],
        &sandbox.cache_dir,
    );
    unmatched_snapshot!(
        "option_completion_on_dash",
        normalize_items(&items, &sandbox.project_dirs["minimal"])
    );
}

/// 策略：选项后无效单词如果是未知参数，仍应触发 args 补全
#[test]
fn test_unmatched_after_option_still_shows_args() {
    let sandbox = get_sandbox();
    let items = complete(
        "bun run --watch x ",
        &sandbox.project_dirs["bun"],
        &sandbox.cache_dir,
    );
    unmatched_snapshot!(
        "unmatched_after_option",
        normalize_items(&items, &sandbox.project_dirs["bun"])
    );
}
