mod common;

use common::{complete, has_item, reload};
use std::fs;
use tempfile::tempdir;

/// 回归测试：不同脚本里的同名动态变量必须各自绑定到自己的字节码。
///
/// 曾经 `__dyn_<变量名>` 是全局索引的 key，npm 与 pnpm 都定义 `pkgOrGlobal`
/// 时，npm 的补全会静默执行 pnpm 的实现（`npm update -g` 因此返回空）。
#[test]
fn test_same_dynamic_var_name_in_different_scripts_does_not_collide() {
    let temp_dir = tempdir().unwrap();
    let completions_dir = temp_dir.path().join("completions");
    let cache_dir = temp_dir.path().join(".cache");
    fs::create_dir_all(completions_dir.join("alpha")).unwrap();
    fs::create_dir_all(completions_dir.join("beta")).unwrap();
    fs::create_dir_all(&cache_dir).unwrap();

    fs::write(
        completions_dir.join("alpha/index.ts"),
        r#"
const items = dynamic(async () => [{ display: "from-alpha" }]);
export default createCompletion({ alpha: { args: items } });
"#,
    )
    .unwrap();

    fs::write(
        completions_dir.join("beta/index.ts"),
        r#"
const items = dynamic(async () => [{ display: "from-beta" }]);
export default createCompletion({ beta: { args: items } });
"#,
    )
    .unwrap();

    reload(&cache_dir, &completions_dir);

    let alpha = complete("alpha ", temp_dir.path(), &cache_dir);
    assert!(has_item(&alpha, "from-alpha"), "alpha items: {alpha:?}");
    assert!(
        !has_item(&alpha, "from-beta"),
        "alpha leaked beta items: {alpha:?}"
    );

    let beta = complete("beta ", temp_dir.path(), &cache_dir);
    assert!(has_item(&beta, "from-beta"), "beta items: {beta:?}");
    assert!(
        !has_item(&beta, "from-alpha"),
        "beta leaked alpha items: {beta:?}"
    );
}
