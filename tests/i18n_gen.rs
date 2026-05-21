mod common;
use std::fs;
use tempfile::tempdir;

/// 设置快照目录为 tests/snapshots/i18n_gen/
macro_rules! i18n_gen_snapshot {
    ($($tt:tt)*) => {{
        static INIT: std::sync::Once = std::sync::Once::new();
        INIT.call_once(|| {
            let mut settings = insta::Settings::new();
            settings.set_snapshot_path(
                std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("tests/snapshots/i18n_gen"),
            );
            Box::leak(Box::new(settings.bind_to_scope()));
        });
        insta::assert_snapshot!($($tt)*)
    }};
}

fn run_i18n_gen(completions_dir: &std::path::Path) {
    let status = std::process::Command::new(common::sugg_bin())
        .args(["dev", "i18n"])
        .arg("--completions-dir")
        .arg(completions_dir)
        .status()
        .expect("failed to run dev i18n");
    assert!(status.success(), "i18n-gen failed");
}

fn read_dts(completions_dir: &std::path::Path) -> String {
    fs::read_to_string(completions_dir.join(".sugg").join("i18n.d.ts")).unwrap()
}

#[test]
fn test_i18n_gen_greet_keys() {
    let temp = tempdir().unwrap();
    let completions_dir = temp.path().join("completions");
    let greet_i18n_dir = completions_dir.join("greet").join("i18n");
    fs::create_dir_all(&greet_i18n_dir).unwrap();

    // 从 fixtures 的 greet/i18n/ 复制翻译文件
    let fixture = common::get_fixture_dir("completions")
        .join("greet")
        .join("i18n");
    for entry in fs::read_dir(&fixture).unwrap() {
        let entry = entry.unwrap();
        fs::copy(entry.path(), greet_i18n_dir.join(entry.file_name())).unwrap();
    }

    run_i18n_gen(&completions_dir);
    i18n_gen_snapshot!(read_dts(&completions_dir));
}

#[test]
fn test_i18n_gen_nested_keys() {
    let temp = tempdir().unwrap();
    let completions_dir = temp.path().join("completions");
    let git_i18n_dir = completions_dir.join("git").join("i18n");
    fs::create_dir_all(&git_i18n_dir).unwrap();

    fs::write(
        git_i18n_dir.join("en.json"),
        r#"{"commit": "Commit changes"}"#,
    )
    .unwrap();

    run_i18n_gen(&completions_dir);
    i18n_gen_snapshot!(read_dts(&completions_dir));
}

#[test]
fn test_i18n_gen_merges_keys_from_multiple_files() {
    let temp = tempdir().unwrap();
    let completions_dir = temp.path().join("completions");
    let ns_i18n_dir = completions_dir.join("somecmd").join("i18n");
    fs::create_dir_all(&ns_i18n_dir).unwrap();

    fs::write(ns_i18n_dir.join("en.json"), r#"{"a": "A"}"#).unwrap();
    fs::write(ns_i18n_dir.join("extra.json"), r#"{"b": "B"}"#).unwrap();

    run_i18n_gen(&completions_dir);
    i18n_gen_snapshot!(read_dts(&completions_dir));
}

#[test]
fn test_i18n_gen_empty_i18n_dir() {
    let temp = tempdir().unwrap();
    let completions_dir = temp.path().join("completions");
    // 创建空子目录（无 i18n），不会产生任何命名空间
    fs::create_dir_all(completions_dir.join("somecmd")).unwrap();

    run_i18n_gen(&completions_dir);
    i18n_gen_snapshot!(read_dts(&completions_dir));
}

#[test]
fn test_i18n_gen_keys_are_sorted() {
    let temp = tempdir().unwrap();
    let completions_dir = temp.path().join("completions");
    let ns_i18n_dir = completions_dir.join("myspace").join("i18n");
    fs::create_dir_all(&ns_i18n_dir).unwrap();

    fs::write(
        ns_i18n_dir.join("en.json"),
        r#"{"z_key": "Z", "a_key": "A", "m_key": "M"}"#,
    )
    .unwrap();

    run_i18n_gen(&completions_dir);
    i18n_gen_snapshot!(read_dts(&completions_dir));
}
