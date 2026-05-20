use rquickjs::{
    Ctx, Function, Object, Value,
    function::{Async, Opt, Rest},
};

/// Rust 原生结构体，通过 IntoJs 自动转化为 JS 对象
#[derive(Debug, Clone)]
pub struct ScanDirEntry {
    pub display: String,
    pub value: String,
    pub is_dir: bool,
}

impl<'js> rquickjs::IntoJs<'js> for ScanDirEntry {
    fn into_js(self, ctx: &Ctx<'js>) -> rquickjs::Result<Value<'js>> {
        let obj = Object::new(ctx.clone())?;
        obj.set("display", self.display)?;
        obj.set("value", self.value)?;
        obj.set("isDir", self.is_dir)?;
        if self.is_dir {
            let style = Object::new(ctx.clone())?;
            style.set("fg", "blue")?;
            obj.set("style", style)?;
        }
        Ok(obj.into_value())
    }
}

/// 极致性能的判断目录方法（避开全量 stat 系统调用）
#[inline]
async fn is_dir_fast(entry: &tokio::fs::DirEntry) -> bool {
    if let Ok(file_type) = entry.file_type().await {
        if file_type.is_dir() {
            return true;
        } else if file_type.is_symlink()
            && let Ok(meta) = entry.metadata().await
        {
            return meta.is_dir();
        }
    }
    false
}

/// 向 QuickJS 环境中注入全局函数
pub fn inject_globals(ctx: Ctx<'_>) {
    let globals = ctx.globals();

    if let Err(e) = globals.set(
        "readFile",
        Function::new(
            ctx.clone(),
            Async(|path: String| async move {
                match tokio::fs::read_to_string(&path).await {
                    Ok(content) => content,
                    Err(e) => {
                        crate::log_warn!("Failed to read file {}: {:?}", &path, e);
                        String::new()
                    }
                }
            }),
        ),
    ) {
        crate::log_error!("Failed to inject readFile global: {:?}", e);
    }

    // =========================================================================
    // 终极 API: scanPath(input, baseDir?)
    // 大一统方案：同时解决普通文件补全与 bun x 虚拟根目录补全
    // =========================================================================
    if let Err(e) = globals.set(
        "scanPath",
        Function::new(
            ctx.clone(),
            Async(|input: String, base_dir_opt: Opt<String>| async move {
                // 如果未提供 baseDir，则默认为当前目录 "."
                let base_dir = base_dir_opt.0.unwrap_or_else(|| ".".to_string());

                let (s_dir, p_dir) = if input.is_empty() {
                    (".".to_string(), "".to_string())
                } else if input.ends_with('/') || input.ends_with('\\') {
                    (input.clone(), input)
                } else if let Some(idx) = input.rfind(['/', '\\']) {
                    let s = &input[..idx];
                    let p = &input[..=idx];
                    let s = if s.is_empty() || s.ends_with(':') {
                        p // 匹配到 "/" 或 Windows 盘符 "C:" -> 强转为 "C:\"
                    } else {
                        s
                    };
                    (s.to_string(), p.to_string())
                } else {
                    (".".to_string(), "".to_string())
                };

                // 物理扫描路径：将 baseDir 与用户输入的逻辑目录进行 Join
                // 如果 s_dir 是绝对路径，Rust 会聪明地忽略 base_dir，表现出原生 Shell 质感！
                let scan_path = std::path::Path::new(&base_dir).join(&s_dir);

                let mut entries = Vec::with_capacity(32);
                match tokio::fs::read_dir(&scan_path).await {
                    Ok(mut dir) => {
                        while let Ok(Some(entry)) = dir.next_entry().await {
                            let name = entry.file_name().to_string_lossy().into_owned();
                            let is_dir = is_dir_fast(&entry).await;

                            // 组装逻辑全路径（不暴露内部 baseDir）
                            let full = format!("{}{}", p_dir, name);

                            let (display, value) = if is_dir {
                                (format!("{}/", full), format!("{}/", full))
                            } else {
                                (full.clone(), format!("{} ", full)) // 文件补全后加上空格
                            };
                            entries.push(ScanDirEntry {
                                display,
                                value,
                                is_dir,
                            });
                        }
                    }
                    Err(e) => {
                        if e.kind() != std::io::ErrorKind::NotFound {
                            crate::log_warn!("Path scan failed {}: {:?}", scan_path.display(), e);
                        }
                    }
                }
                // 极致体验：文件夹置顶排在前面，然后再按字母表顺序排序
                entries.sort_by(|a, b| {
                    b.is_dir
                        .cmp(&a.is_dir)
                        .then_with(|| a.display.cmp(&b.display))
                });
                entries
            }),
        ),
    ) {
        crate::log_error!("Failed to inject scanPath global: {:?}", e);
    }

    if let Err(e) = globals.set(
        "exec",
        Function::new(
            ctx.clone(),
            Async(|command: String| async move {
                #[cfg(target_os = "windows")]
                let output = {
                    use std::os::windows::process::CommandExt;
                    let mut std_cmd = std::process::Command::new("cmd");
                    std_cmd.arg("/C");
                    // 使用 raw_arg，告诉 Rust 绝对不要在里面瞎加双引号或转义！
                    std_cmd.raw_arg(&command);
                    tokio::process::Command::from(std_cmd).output().await
                };

                #[cfg(not(target_os = "windows"))]
                let output = tokio::process::Command::new("sh")
                    .args(&["-c", &command])
                    .output()
                    .await;
                match output {
                    Ok(o) => String::from_utf8_lossy(&o.stdout).into_owned(),
                    Err(e) => {
                        crate::log_warn!("Command execution failed `{}`: {:?}", &command, e);
                        String::new()
                    }
                }
            }),
        ),
    ) {
        crate::log_error!("Failed to inject exec global: {:?}", e);
    }

    // =========================================================================
    // execFile(cmd, args) —— 极速直接进程拉起，无 Shell 开销与注入风险
    //
    // 与 exec(cmd) 不同，execFile 不走 sh -c / cmd /C，而是直接调用
    // tokio::process::Command::new(cmd).args(&args)，零中间进程、零 Shell 解析。
    //
    // 适用场景：
    //   - 纯命令行执行（无需管道、重定向、变量展开等内容）
    //   - 高频调用场景（每次省去 fork sh 的几毫秒，积少成多）
    //   - 传入不可信参数时（userInput 作为 args 单独传递，无注入风险）
    //
    // 不适用的场景（此时请用 exec）：
    //   - 需要管道符 |、重定向 >、逻辑链 && 等 Shell 语法
    //   - 需要执行 Windows 内置命令（dir, echo, cd 等）
    //   - 依赖 Shell 变量展开（$HOME, %PATH% 等）
    // =========================================================================
    if let Err(e) = globals.set(
        "execFile",
        Function::new(
            ctx.clone(),
            Async(|cmd: String, args: Opt<Vec<String>>| async move {
                let args = args.0.unwrap_or_default();
                match tokio::process::Command::new(&cmd)
                    .args(&args)
                    .output()
                    .await
                {
                    Ok(o) => String::from_utf8_lossy(&o.stdout).into_owned(),
                    Err(e) => {
                        crate::log_warn!("execFile execution failed `{}`: {:?}", &cmd, e);
                        String::new()
                    }
                }
            }),
        ),
    ) {
        crate::log_error!("Failed to inject execFile global: {:?}", e);
    }

    // 提取为一个显式声明同生命周期 'js 的内部函数，解决闭包生命周期推断歧义
    fn args_to_string<'js>(ctx: &Ctx<'js>, args: Rest<Value<'js>>) -> String {
        let mut parts = Vec::with_capacity(args.0.len());
        for arg in args.0 {
            // 如果是原生字符串，直接提取（避免被序列化带上双引号）
            if let Some(s) = arg.as_string()
                && let Ok(s_str) = s.to_string()
            {
                parts.push(s_str);
                continue;
            }

            // 如果是对象或数组，尝试优雅地 JSON.stringify
            if (arg.is_object() || arg.is_array())
                && let Ok(json) = ctx.globals().get::<_, Object>("JSON")
                && let Ok(stringify) = json.get::<_, Function>("stringify")
                && let Ok(s) = stringify.call::<_, String>((arg.clone(),))
            {
                parts.push(s);
                continue;
            }

            // 兜底：使用 JS 的 String() 强转 (例如处理 boolean, number, undefined, null 等)
            if let Ok(string_func) = ctx.globals().get::<_, Function>("String")
                && let Ok(s) = string_func.call::<_, String>((arg,))
            {
                parts.push(s);
                continue;
            }

            parts.push(String::from("[unknown]"));
        }
        parts.join(" ")
    }

    // 注入 ui 对象
    if let Ok(ui_obj) = Object::new(ctx.clone()) {
        fn ui_log<'js>(ctx: Ctx<'js>, args: Rest<Value<'js>>) {
            crate::logger::write_log(crate::logger::LogLevel::Log, &args_to_string(&ctx, args));
        }
        fn ui_info<'js>(ctx: Ctx<'js>, args: Rest<Value<'js>>) {
            crate::logger::write_log(crate::logger::LogLevel::Info, &args_to_string(&ctx, args));
        }
        fn ui_warn<'js>(ctx: Ctx<'js>, args: Rest<Value<'js>>) {
            crate::logger::write_log(crate::logger::LogLevel::Warn, &args_to_string(&ctx, args));
        }
        fn ui_error<'js>(ctx: Ctx<'js>, args: Rest<Value<'js>>) {
            crate::logger::write_log(crate::logger::LogLevel::Error, &args_to_string(&ctx, args));
        }
        for (name, func) in [
            ("log", Function::new(ctx.clone(), ui_log)),
            ("info", Function::new(ctx.clone(), ui_info)),
            ("warn", Function::new(ctx.clone(), ui_warn)),
            ("error", Function::new(ctx.clone(), ui_error)),
        ] {
            if let Ok(f) = func {
                let _ = ui_obj.set(name, f);
            }
        }
        if let Err(e) = globals.set("ui", ui_obj) {
            crate::log_error!("Failed to inject ui global: {:?}", e);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rquickjs::{AsyncContext, AsyncRuntime, async_with};

    #[tokio::test]
    async fn test_inject_globals() {
        let rt = AsyncRuntime::new().expect("Failed to create Runtime");
        let ctx = AsyncContext::full(&rt)
            .await
            .expect("Failed to create Context");

        async fn async_with_fn(ctx: Ctx<'_>) {
            inject_globals(ctx.clone());

            let is_func: bool = ctx
                .eval("typeof globalThis.readFile === 'function'")
                .unwrap();
            assert!(is_func, "readFile should be injected");

            let is_scan_path_func: bool = ctx
                .eval("typeof globalThis.scanPath === 'function'")
                .unwrap();
            assert!(is_scan_path_func, "scanPath should be injected");

            let is_exec_func: bool = ctx.eval("typeof globalThis.exec === 'function'").unwrap();
            assert!(is_exec_func, "exec should be injected");

            let is_exec_file_func: bool = ctx
                .eval("typeof globalThis.execFile === 'function'")
                .unwrap();
            assert!(is_exec_file_func, "execFile should be injected");
        }

        async_with!(ctx => |ctx| { async_with_fn(ctx).await }).await;
    }

    #[tokio::test]
    async fn test_exec_file_basic() {
        let rt = AsyncRuntime::new().expect("Failed to create Runtime");
        let ctx = AsyncContext::full(&rt)
            .await
            .expect("Failed to create Context");

        async fn async_with_fn(ctx: Ctx<'_>) {
            inject_globals(ctx.clone());

            // 执行一个不需要 Shell 的简单命令来验证 execFile
            // 跨平台方案：执行 rustc --version（rustc 必定存在于开发环境中）
            let script = r#"
                (async () => {
                    const out = await execFile("rustc", ["--version"]);
                    return out.trim();
                })()
            "#;
            let promise: rquickjs::Promise = ctx.eval(script).unwrap();
            let result: String = promise.into_future().await.unwrap();
            assert!(
                result.starts_with("rustc "),
                "execFile 应返回 rustc 版本信息，实际得到: '{:?}'",
                result
            );

            // 参数数组正确传递：execFile("/bin/echo", ["hello", "world"]) 应输出 "hello world"
            #[cfg(unix)]
            {
                let script2 = r#"
                    (async () => {
                        const out = await execFile("/bin/echo", ["hello", "world"]);
                        return out.trim();
                    })()
                "#;
                let promise2: rquickjs::Promise = ctx.eval(script2).unwrap();
                let result2: String = promise2.into_future().await.unwrap();
                assert_eq!(result2, "hello world");
            }

            // Windows 上用 cmd /c echo 模拟（cmd.exe 是独立 PE 文件，可用 execFile 调起）
            #[cfg(windows)]
            {
                let script2 = r#"
                    (async () => {
                        const out = await execFile("cmd.exe", ["/c", "echo", "hello"]);
                        return out.trim();
                    })()
                "#;
                let promise2: rquickjs::Promise = ctx.eval(script2).unwrap();
                let result2: String = promise2.into_future().await.unwrap();
                assert_eq!(result2, "hello");
            }

            // ========== 场景 4：不传 args 参数也能正常调用 ==========
            // execFile("rustc") 不传第二个参数，等价于 execFile("rustc", [])
            // rustc 无参时 stdout 为空（报错走 stderr），验证不 panic 且返回 string
            {
                let script3 = r#"
                    (async () => {
                        const out = await execFile("rustc");
                        return typeof out === 'string' ? 'ok' : 'wrong_type';
                    })()
                "#;
                let promise3: rquickjs::Promise = ctx.eval(script3).unwrap();
                let result3: String = promise3.into_future().await.unwrap();
                assert_eq!(
                    result3, "ok",
                    "execFile called without args should return a string"
                );
            }

            // 场景 5：显式传空数组效果等价
            #[cfg(unix)]
            {
                let script4 = r#"
                    (async () => {
                        const out = await execFile("/bin/echo", []);
                        return out.trim();
                    })()
                "#;
                let promise4: rquickjs::Promise = ctx.eval(script4).unwrap();
                let result4: String = promise4.into_future().await.unwrap();
                assert_eq!(
                    result4, "",
                    "/bin/echo called without args should output empty"
                );
            }
        }

        async_with!(ctx => |ctx| { async_with_fn(ctx).await }).await;
    }

    #[tokio::test]
    async fn test_exec_file_error_handling() {
        let rt = AsyncRuntime::new().expect("Failed to create Runtime");
        let ctx = AsyncContext::full(&rt)
            .await
            .expect("Failed to create Context");

        async fn async_with_fn(ctx: Ctx<'_>) {
            inject_globals(ctx.clone());

            // 执行不存在的命令应返回空字符串（不 panic）
            let script = r#"
                (async () => {
                    const out = await execFile("nonexistent_command_xyz", ["--version"]);
                    return out;
                })()
            "#;
            let promise: rquickjs::Promise = ctx.eval(script).unwrap();
            let result: String = promise.into_future().await.unwrap();
            assert!(
                result.is_empty(),
                "不存在的命令应返回空字符串, 得到: {:?}",
                result
            );
        }

        async_with!(ctx => |ctx| { async_with_fn(ctx).await }).await;
    }

    #[tokio::test]
    async fn test_scan_path_unified() {
        use tempfile::tempdir;

        let rt = AsyncRuntime::new().expect("Failed to create Runtime");
        let ctx = AsyncContext::full(&rt)
            .await
            .expect("Failed to create Context");

        async fn async_with_fn(ctx: Ctx<'_>) {
            inject_globals(ctx.clone());

            let temp_dir = tempdir().expect("Failed to create temp directory");
            let base_path = temp_dir.path();

            // 构造场景: base_path 下有个 bin 目录，bin 下有 eslint 文件和 sub 目录，sub 里有 foo 文件
            tokio::fs::create_dir(base_path.join("bin")).await.unwrap();
            tokio::fs::write(base_path.join("bin").join("eslint"), "exec")
                .await
                .unwrap();
            tokio::fs::create_dir(base_path.join("bin").join("sub"))
                .await
                .unwrap();
            tokio::fs::write(base_path.join("bin").join("sub").join("foo"), "foo")
                .await
                .unwrap();

            let mut base_str = base_path.display().to_string();
            if cfg!(windows) {
                base_str = base_str.replace("\\", "/");
            }

            // ---- 场景 1: 普通路径推断补全 ----
            // 用户输入 "bi"
            let script1 = format!("scanPath('bi', '{}')", base_str);
            let promise1: rquickjs::Promise = ctx.eval(script1.as_str()).unwrap();
            let items1: Vec<rquickjs::Object> = promise1.into_future().await.unwrap();
            let d1: Vec<String> = items1.iter().map(|o| o.get("display").unwrap()).collect();
            // 当前目录下看到了 bin/
            assert_eq!(d1, vec!["bin/"]);

            // ---- 场景 2: bun x 虚拟目录补全 ----
            // 假设这是 bun x 命令，用户尚未输入任何前缀，我们去扫描 .bin 目录
            let script2 = format!("globalThis.scanPath('', '{}/bin')", base_str);
            let promise2: rquickjs::Promise = ctx.eval(script2.as_str()).unwrap();
            let items2: Vec<rquickjs::Object> = promise2.into_future().await.unwrap();
            let d2: Vec<String> = items2.iter().map(|o| o.get("display").unwrap()).collect();
            // 不包含前缀路径！文件夹置顶，文件在下
            assert_eq!(d2, vec!["sub/", "eslint"]);

            // ---- 场景 3: bun x 虚拟目录下的子目录补全！ ----
            // 用户在 bun x 后输入了 "sub/"
            let script3 = format!("globalThis.scanPath('sub/', '{}/bin')", base_str);
            let promise3: rquickjs::Promise = ctx.eval(script3.as_str()).unwrap();
            let items3: Vec<rquickjs::Object> = promise3.into_future().await.unwrap();
            let d3: Vec<String> = items3.iter().map(|o| o.get("display").unwrap()).collect();
            // 完美拼接出 sub/ 前缀，同时没有暴露物理上隐藏的 bin 目录
            assert_eq!(d3, vec!["sub/foo"]);
        }

        async_with!(ctx => |ctx| { async_with_fn(ctx).await }).await;
    }
}
