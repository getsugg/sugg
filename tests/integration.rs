/// 设置快照目录为 tests/snapshots/integration/
macro_rules! integration_snapshot {
    ($($tt:tt)*) => {{
        static INIT: std::sync::Once = std::sync::Once::new();
        INIT.call_once(|| {
            let mut settings = insta::Settings::new();
            settings.set_snapshot_path(
                std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("tests/snapshots/integration"),
            );
            Box::leak(Box::new(settings.bind_to_scope()));
        });
        insta::assert_json_snapshot!($($tt)*)
    }};
}

mod common;
use common::{
    complete, get_fixture_dir, get_sandbox, has_item, normalize_items, reload, reload_with_lang,
};
use std::fs;
use tempfile::tempdir;

#[test]
fn test_reload_success() {
    let temp_dir = tempdir().unwrap();
    let cache_dir = temp_dir.path().join(".cache");
    fs::create_dir_all(&cache_dir).unwrap();
    let completions_dir = temp_dir.path().join("completions");
    reload(&cache_dir, &completions_dir);
}

#[test]
fn test_root_command_cleaning() {
    let sandbox = get_sandbox();
    let proj_bun = &sandbox.project_dirs["bun"];
    let proj_pnpm = &sandbox.project_dirs["pnpm"];

    let all = serde_json::json!([
        { "name": "bun_run",          "input": "bun run ",       "result": complete("bun run ",       proj_bun, &sandbox.cache_dir) },
        { "name": "bun_exe_run",      "input": "bun.exe run ",   "result": complete("bun.exe run ",   proj_bun, &sandbox.cache_dir) },
        { "name": "pnpm_run",         "input": "pnpm run ",      "result": complete("pnpm run ",      proj_pnpm, &sandbox.cache_dir) },
        { "name": "pnpm_cmd_run",     "input": "pnpm.cmd run ",  "result": complete("pnpm.cmd run ",  proj_pnpm, &sandbox.cache_dir) },
    ]);

    integration_snapshot!(all);
}

#[test]
fn test_style_attribute() {
    let sandbox = get_sandbox();

    let items = complete(
        "pnpm install -",
        &sandbox.project_dirs["minimal"],
        &sandbox.cache_dir,
    );
    integration_snapshot!(
        "option_with_style",
        normalize_items(&items, &sandbox.project_dirs["minimal"])
    );

    let items = complete("", &sandbox.project_dirs["minimal"], &sandbox.cache_dir);
    integration_snapshot!(
        "top_level_commands",
        normalize_items(&items, &sandbox.project_dirs["minimal"])
    );
}

#[test]
fn test_real_bun_project_run_scripts() {
    let sandbox = get_sandbox();
    let items = complete("bun run ", &sandbox.project_dirs["bun"], &sandbox.cache_dir);
    integration_snapshot!(
        "real_bun_project_run_scripts",
        normalize_items(&items, &sandbox.project_dirs["bun"])
    );
}

#[test]
fn test_dir_index_completion_loaded() {
    // 验证 git/index.ts 目录形式的补全脚本被正确加载
    let sandbox = get_sandbox();
    let items = complete("git ", &sandbox.project_dirs["minimal"], &sandbox.cache_dir);
    assert!(has_item(&items, "commit"), "应包含 commit 子命令");
    assert!(has_item(&items, "push"), "应包含 push 子命令");
    assert!(has_item(&items, "pull"), "应包含 pull 子命令");
}

/// 递归复制目录
fn copy_dir(src: &std::path::Path, dst: &std::path::Path) {
    fs::create_dir_all(dst).unwrap();
    for entry in fs::read_dir(src).unwrap() {
        let entry = entry.unwrap();
        let s = entry.path();
        let d = dst.join(entry.file_name());
        if s.is_dir() {
            copy_dir(&s, &d);
        } else {
            fs::copy(&s, &d).unwrap();
        }
    }
}

fn setup_i18n_completions(
    fixture: &str,
) -> (tempfile::TempDir, std::path::PathBuf, std::path::PathBuf) {
    let temp_dir = tempdir().unwrap();
    let cache_dir = temp_dir.path().join(".cache");
    let completions_dir = temp_dir.path().join("completions");
    let fixture_dir = common::get_fixture_dir("completions");
    fs::create_dir_all(&completions_dir).unwrap();
    // 复制主脚本文件
    fs::copy(fixture_dir.join(fixture), completions_dir.join(fixture)).unwrap();
    // 复制对应命令命名空间的 i18n/ 目录（如 greet/i18n/）
    let cmd_name = fixture.trim_end_matches(".ts").trim_end_matches(".js");
    let cmd_i18n_src = fixture_dir.join(cmd_name).join("i18n");
    if cmd_i18n_src.is_dir() {
        let cmd_i18n_dst = completions_dir.join(cmd_name).join("i18n");
        fs::create_dir_all(&cmd_i18n_dst).unwrap();
        for entry in fs::read_dir(&cmd_i18n_src).unwrap() {
            let entry = entry.unwrap();
            fs::copy(entry.path(), cmd_i18n_dst.join(entry.file_name())).unwrap();
        }
    }
    (temp_dir, cache_dir, completions_dir)
}

#[test]
fn test_i18n_greet_subcommands_en() {
    let (temp_dir, cache_dir, completions_dir) = setup_i18n_completions("greet.ts");
    reload_with_lang(&cache_dir, &completions_dir, "en");
    let items = complete("greet ", temp_dir.path(), &cache_dir);
    integration_snapshot!(items);
}

#[test]
fn test_i18n_greet_subcommands_zh() {
    let (temp_dir, cache_dir, completions_dir) = setup_i18n_completions("greet.ts");
    reload_with_lang(&cache_dir, &completions_dir, "zh");
    let items = complete("greet ", temp_dir.path(), &cache_dir);
    integration_snapshot!(items);
}

#[test]
fn test_i18n_greet_subcommands_fallback() {
    let (temp_dir, cache_dir, completions_dir) = setup_i18n_completions("greet.ts");
    reload_with_lang(&cache_dir, &completions_dir, "fr");
    let items = complete("greet ", temp_dir.path(), &cache_dir);
    integration_snapshot!(items);
}

#[test]
fn test_i18n_dynamic_en() {
    let (temp_dir, cache_dir, completions_dir) = setup_i18n_completions("greet_dynamic.ts");
    reload_with_lang(&cache_dir, &completions_dir, "en");
    let items = complete("greet_dynamic ", temp_dir.path(), &cache_dir);
    integration_snapshot!(items);
}

/// 为文件夹格式的脚本设置 i18n 测试环境
fn setup_i18n_folder_completions(
    folder: &str,
) -> (tempfile::TempDir, std::path::PathBuf, std::path::PathBuf) {
    let temp_dir = tempdir().unwrap();
    let cache_dir = temp_dir.path().join(".cache");
    let completions_dir = temp_dir.path().join("completions");
    let fixture_dir = common::get_fixture_dir("completions");
    fs::create_dir_all(&completions_dir).unwrap();
    // 复制整个文件夹格式脚本目录（含自身 i18n/ 子目录）
    let src_dir = fixture_dir.join(folder);
    let dst_dir = completions_dir.join(folder);
    copy_dir(&src_dir, &dst_dir);
    // 不再复制根 i18n/ 目录，命令自己的 i18n/ 已随目录复制
    (temp_dir, cache_dir, completions_dir)
}

#[test]
fn test_i18n_folder_format_en() {
    let (temp_dir, cache_dir, completions_dir) = setup_i18n_folder_completions("docker");
    reload_with_lang(&cache_dir, &completions_dir, "en");
    let items = complete("docker ", temp_dir.path(), &cache_dir);
    integration_snapshot!(items);
}

#[test]
fn test_i18n_folder_format_zh() {
    let (temp_dir, cache_dir, completions_dir) = setup_i18n_folder_completions("docker");
    reload_with_lang(&cache_dir, &completions_dir, "zh");
    let items = complete("docker ", temp_dir.path(), &cache_dir);
    integration_snapshot!(items);
}

#[test]
fn test_i18n_folder_format_fallback() {
    let (temp_dir, cache_dir, completions_dir) = setup_i18n_folder_completions("docker");
    reload_with_lang(&cache_dir, &completions_dir, "fr");
    let items = complete("docker ", temp_dir.path(), &cache_dir);
    integration_snapshot!(items);
}

#[test]
fn test_i18n_dynamic_zh() {
    let (temp_dir, cache_dir, completions_dir) = setup_i18n_completions("greet_dynamic.ts");
    reload_with_lang(&cache_dir, &completions_dir, "zh");
    let items = complete("greet_dynamic ", temp_dir.path(), &cache_dir);
    integration_snapshot!(items);
}

#[test]
fn test_bun_run_path_completion() {
    let sandbox = get_sandbox();

    // 创建临时项目目录，包含子目录和文件用于测试路径补全
    let project_dir = tempdir().unwrap();
    let project_path = project_dir.path();

    // 创建 package.json（需要 scripts 字段验证合并逻辑）
    let pkg = r#"{"scripts":{"dev":"echo dev"}}"#;
    fs::write(project_path.join("package.json"), pkg).unwrap();

    // 创建 src 目录和文件
    fs::create_dir_all(project_path.join("src")).unwrap();
    fs::write(project_path.join("src").join("main.ts"), "// main").unwrap();
    fs::write(project_path.join("src").join("utils.ts"), "// utils").unwrap();

    // 创建嵌套子目录
    fs::create_dir_all(project_path.join("src").join("components")).unwrap();
    fs::write(
        project_path
            .join("src")
            .join("components")
            .join("button.tsx"),
        "// button",
    )
    .unwrap();

    // 测试 1：bun run src/ — 补全 src 下的文件和子目录
    let items = complete("bun run src/", project_path, &sandbox.cache_dir);
    integration_snapshot!("bun_run_src_path", normalize_items(&items, project_path));

    // 测试 2：bun run src/components/ — 补全嵌套子目录
    let items = complete("bun run src/components/", project_path, &sandbox.cache_dir);
    integration_snapshot!(
        "bun_run_src_components_path",
        normalize_items(&items, project_path)
    );

    // 测试 3：bun run 空 prefix 时，src 目录应显示为目录条目
    //（scanDir 只返回直接子项，不递归，因此 src/main.ts 不会出现）
    let items = complete("bun run ", project_path, &sandbox.cache_dir);
    // 验证 src 目录被正确扫描显示为目录条目
    assert!(
        items.iter().any(|item| {
            item["value"] == "src/"
                && item["display_override"] == "src/"
                && item["description"] == "directory"
        }),
        "应包含 src/ 目录条目"
    );
    // 验证 package.json（直接子文件）被列出（文件 value 带尾随空格）
    assert!(
        items.iter().any(|item| item["value"] == "package.json "),
        "应包含 package.json"
    );
    // 验证 npm script 在空 prefix 下也被显示
    assert!(
        items.iter().any(|item| item["display_override"] == "dev"),
        "应包含 dev script"
    );

    // 测试 4：bun run src/com — 部分路径前缀匹配，应展开为 src/components/
    let items = complete("bun run src/com", project_path, &sandbox.cache_dir);
    integration_snapshot!(
        "bun_run_src_com_partial",
        normalize_items(&items, project_path)
    );
}

#[test]
fn test_bun_x_vs_run_paths() {
    let sandbox = get_sandbox();
    let project_path = get_fixture_dir("projects/bun_x_vs_run");

    // ── bun run — 顯示檔案、目錄、scripts ──
    let items_run = complete("bun run ", &project_path, &sandbox.cache_dir);
    integration_snapshot!(
        "bun_run_vs_x__run",
        normalize_items(&items_run, &project_path)
    );

    // ── bun x — 只掃 node_modules/.bin ──
    let items_x = complete("bun x ", &project_path, &sandbox.cache_dir);
    integration_snapshot!("bun_run_vs_x__x", normalize_items(&items_x, &project_path));
}

// =========================================================================
// 以下测试全部基于 testkit 命令，无需单独的补全脚本
// =========================================================================

#[test]
fn test_testkit_option_value_completion() {
    let sandbox = get_sandbox();
    let proj = &sandbox.project_dirs["minimal"];

    let all = serde_json::json!([
        { "name": "mode_empty",       "input": "testkit opts --mode ",        "result": complete("testkit opts --mode ",        proj, &sandbox.cache_dir) },
        { "name": "mode_prefix_d",    "input": "testkit opts --mode d",       "result": complete("testkit opts --mode d",       proj, &sandbox.cache_dir) },
        { "name": "log_level_prefix", "input": "testkit opts --log-level E",  "result": complete("testkit opts --log-level E",  proj, &sandbox.cache_dir) },
        { "name": "dynamic_opt_empty",   "input": "testkit opts --dynamic-opt ", "result": complete("testkit opts --dynamic-opt ", proj, &sandbox.cache_dir) },
        { "name": "dynamic_opt_prefix",  "input": "testkit opts --dynamic-opt item2", "result": complete("testkit opts --dynamic-opt item2", proj, &sandbox.cache_dir) },
        { "name": "dynamic_opt_short",   "input": "testkit opts -d item",      "result": complete("testkit opts -d item",      proj, &sandbox.cache_dir) },
        { "name": "name_no_suggestions", "input": "testkit opts --name ",       "result": complete("testkit opts --name ",       proj, &sandbox.cache_dir) },
        // shared dynamic 复用
        { "name": "shared1_empty",       "input": "testkit opts --shared1 ",    "result": complete("testkit opts --shared1 ",    proj, &sandbox.cache_dir) },
        { "name": "shared2_empty",       "input": "testkit opts --shared2 ",    "result": complete("testkit opts --shared2 ",    proj, &sandbox.cache_dir) },
        { "name": "shared1_prefix_b",    "input": "testkit opts --shared1 b",   "result": complete("testkit opts --shared1 b",   proj, &sandbox.cache_dir) },
        { "name": "shared2_short_alias", "input": "testkit opts -s2 c",         "result": complete("testkit opts -s2 c",         proj, &sandbox.cache_dir) },
    ]);

    integration_snapshot!(all);
}

#[test]
fn test_testkit_options_passed_to_dynamic() {
    let sandbox = get_sandbox();
    let proj = &sandbox.project_dirs["minimal"];

    let all = serde_json::json!([
        { "name": "no_options",       "input": "testkit optsinfo ",                 "result": complete("testkit optsinfo ",                 proj, &sandbox.cache_dir) },
        { "name": "with_g",           "input": "testkit optsinfo -g ",              "result": complete("testkit optsinfo -g ",              proj, &sandbox.cache_dir) },
        { "name": "with_global",      "input": "testkit optsinfo --global ",         "result": complete("testkit optsinfo --global ",         proj, &sandbox.cache_dir) },
        { "name": "with_c",           "input": "testkit optsinfo -c ",              "result": complete("testkit optsinfo -c ",              proj, &sandbox.cache_dir) },
        { "name": "with_config",      "input": "testkit optsinfo --config ",         "result": complete("testkit optsinfo --config ",         proj, &sandbox.cache_dir) },
        { "name": "with_g_c",         "input": "testkit optsinfo -g -c ",           "result": complete("testkit optsinfo -g -c ",           proj, &sandbox.cache_dir) },
        { "name": "with_cwd",         "input": "testkit optsinfo --cwd mydir ",     "result": complete("testkit optsinfo --cwd mydir ",     proj, &sandbox.cache_dir) },
        { "name": "with_exclude",     "input": "testkit optsinfo --exclude react --exclude vue ", "result": complete("testkit optsinfo --exclude react --exclude vue ", proj, &sandbox.cache_dir) },
    ]);

    integration_snapshot!(all);
}

#[test]
fn test_testkit_dynamic_reuse() {
    let sandbox = get_sandbox();
    let proj = &sandbox.project_dirs["minimal"];

    let items_cmd1 = complete("testkit dynamicReuse cmd1 ", proj, &sandbox.cache_dir);
    let items_cmd2 = complete("testkit dynamicReuse cmd2 ", proj, &sandbox.cache_dir);

    integration_snapshot!(serde_json::json!({
        "cmd1": items_cmd1,
        "cmd2": items_cmd2,
    }));
}

#[test]
fn test_testkit_hybrid() {
    let sandbox = get_sandbox();
    let proj = &sandbox.project_dirs["minimal"];

    let all = serde_json::json!([
        { "name": "empty_prefix",    "input": "testkit hybrid ",     "result": complete("testkit hybrid ",     proj, &sandbox.cache_dir) },
        { "name": "prefix_p",        "input": "testkit hybrid p",    "result": complete("testkit hybrid p",    proj, &sandbox.cache_dir) },
        { "name": "unknown_subcmd",  "input": "testkit hybrid xyz ", "result": complete("testkit hybrid xyz ", proj, &sandbox.cache_dir) },
    ]);

    integration_snapshot!(all);
}

#[test]
fn test_testkit_display_value() {
    let sandbox = get_sandbox();
    let proj = &sandbox.project_dirs["minimal"];

    let items = complete("testkit displayValue ", proj, &sandbox.cache_dir);
    integration_snapshot!(items);
}

#[test]
fn test_testkit_empty_args_option() {
    let sandbox = get_sandbox();
    let proj = &sandbox.project_dirs["minimal"];

    // 选项 --token 需要值但无补全建议
    let items = complete("testkit emptyArgs --token ", proj, &sandbox.cache_dir);
    // 应返回空数组（没有补全建议，引擎不弹窗）
    assert!(items.is_empty());
}

#[test]
fn test_testkit_double_dash_stops_option_parsing() {
    let sandbox = get_sandbox();
    let proj = &sandbox.project_dirs["minimal"];

    // -- 之后，--mode 被视为位置参数，不再作为选项
    // 同时 -- 后仍可补全子命令（如 npm -- run 的场景）
    let all = serde_json::json!([
        // 对照：没有 -- 时，列出子命令 + 选项
        { "name": "no_dash",      "input": "testkit ",               "result": complete("testkit ",               proj, &sandbox.cache_dir) },
        // -- 后只列出子命令，不列出选项（-h/--help 被屏蔽）
        { "name": "dash_only",    "input": "testkit -- ",            "result": complete("testkit -- ",            proj, &sandbox.cache_dir) },
        // -- 后子命令 opt 补全（按前缀过滤）
        { "name": "dash_prefix_o","input": "testkit -- o",          "result": complete("testkit -- o",          proj, &sandbox.cache_dir) },
        // -- 后跟完整子命令 opts，进入 opts 上下文
        { "name": "dash_opts",    "input": "testkit -- opts ",       "result": complete("testkit -- opts ",       proj, &sandbox.cache_dir) },
        // -- 后两层子命令：dynamicReuse -> cmd1
        { "name": "dash_dyn_cmd1","input": "testkit -- dynamicReuse cmd1 ", "result": complete("testkit -- dynamicReuse cmd1 ", proj, &sandbox.cache_dir) },
        // -- 后选项 --mode 被视为位置参数，子命令匹配失败后进入位置参数模式
        { "name": "with_dash",    "input": "testkit opts -- --mode ", "result": complete("testkit opts -- --mode ", proj, &sandbox.cache_dir) },
    ]);

    integration_snapshot!(all);
}

