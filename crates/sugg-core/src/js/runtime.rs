use std::sync::OnceLock;

use rquickjs::{
    Ctx, Function, Object, Value,
    function::{Async, Opt, Rest},
};

fn http_client() -> &'static reqwest::Client {
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(5))
            .build()
            .expect("Failed to create reqwest Client")
    })
}

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
        "__readFile",
        Function::new(
            ctx.clone(),
            Async(|path: String| async move {
                match tokio::fs::read_to_string(&path).await {
                    Ok(content) => content,
                    Err(e) => {
                        if e.kind() != std::io::ErrorKind::NotFound {
                            crate::log_warn!("Failed to read file {}: {:?}", &path, e);
                        }
                        String::new()
                    }
                }
            }),
        ),
    ) {
        crate::log_error!("Failed to inject __readFile global: {:?}", e);
    }

    // =========================================================================
    // 终极 API: scanPath(input, baseDir?)
    // 大一统方案：同时解决普通文件补全与 bun x 虚拟根目录补全
    // =========================================================================
    if let Err(e) = globals.set(
        "__scanPath",
        Function::new(
            ctx.clone(),
            Async(|input: String, base_dir_opt: Opt<String>| async move {
                let base_dir = base_dir_opt.0.unwrap_or_else(|| ".".to_string());

                let (s_dir, p_dir) = if input.is_empty() {
                    (".".to_string(), "".to_string())
                } else if input.ends_with('/') || input.ends_with('\\') {
                    (input.clone(), input)
                } else if let Some(idx) = input.rfind(['/', '\\']) {
                    let s = &input[..idx];
                    let p = &input[..=idx];
                    let s = if s.is_empty() || s.ends_with(':') {
                        p
                    } else {
                        s
                    };
                    (s.to_string(), p.to_string())
                } else {
                    (".".to_string(), "".to_string())
                };

                let scan_path = std::path::Path::new(&base_dir).join(&s_dir);

                let mut entries = Vec::with_capacity(32);
                match tokio::fs::read_dir(&scan_path).await {
                    Ok(mut dir) => {
                        while let Ok(Some(entry)) = dir.next_entry().await {
                            let name = entry.file_name().to_string_lossy().into_owned();
                            let is_dir = is_dir_fast(&entry).await;

                            let full = format!("{}{}", p_dir, name);

                            let (display, value) = if is_dir {
                                (format!("{}/", full), format!("{}/", full))
                            } else {
                                (full.clone(), format!("{} ", full))
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
        "__exec",
        Function::new(
            ctx.clone(),
            Async(|command: String| async move {
                #[cfg(target_os = "windows")]
                let output = {
                    use std::os::windows::process::CommandExt;
                    let mut std_cmd = std::process::Command::new("cmd");
                    std_cmd.arg("/C");
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
    // =========================================================================
    if let Err(e) = globals.set(
        "__execFile",
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
        crate::log_error!("Failed to inject __execFile global: {:?}", e);
    }

    // =========================================================================
    // __fetch_raw(config) — 底层 HTTP 请求，返回 JSON 序列化的响应
    // =========================================================================
    if let Err(e) = globals.set(
        "__fetch_raw",
        Function::new(
            ctx.clone(),
            Async(|config_obj: Object<'_>| {
                let url: String = config_obj.get("url").unwrap_or_default();
                let method_str: String = config_obj
                    .get("method")
                    .unwrap_or_else(|_| "GET".to_string());
                let headers: std::collections::HashMap<String, String> =
                    config_obj.get("headers").unwrap_or_default();
                let body: String = config_obj.get("body").unwrap_or_default();
                let timeout_ms: u64 = config_obj.get("timeout").unwrap_or(2000);
                async move {
                    let client = http_client();
                    let timeout = std::time::Duration::from_millis(timeout_ms);
                    let method = match method_str.to_uppercase().as_str() {
                        "POST" => reqwest::Method::POST,
                        "PUT" => reqwest::Method::PUT,
                        "DELETE" => reqwest::Method::DELETE,
                        "PATCH" => reqwest::Method::PATCH,
                        _ => reqwest::Method::GET,
                    };
                    let mut req = client.request(method, &url).timeout(timeout);
                    for (k, v) in &headers {
                        req = req.header(k.as_str(), v.as_str());
                    }
                    if !body.is_empty() {
                        req = req.body(body.clone());
                    }
                    match req.send().await {
                        Ok(resp) => {
                            let status = resp.status().as_u16();
                            let status_text =
                                resp.status().canonical_reason().unwrap_or("").to_string();
                            let mut resp_headers = std::collections::HashMap::new();
                            for (key, val) in resp.headers().iter() {
                                if let Ok(val_str) = val.to_str() {
                                    resp_headers.insert(key.to_string(), val_str.to_string());
                                }
                            }
                            let resp_body = resp.text().await.unwrap_or_default();
                            let res_map = serde_json::json!({
                                "status": status,
                                "statusText": status_text,
                                "headers": resp_headers,
                                "body": resp_body
                            });
                            serde_json::to_string(&res_map).unwrap_or_default()
                        }
                        Err(e) => {
                            crate::log_warn!("__fetch_raw request failed or timed out: {:?}", e);
                            String::new()
                        }
                    }
                }
            }),
        ),
    ) {
        crate::log_error!("Failed to inject __fetch_raw global: {:?}", e);
    }

    fn args_to_string<'js>(ctx: &Ctx<'js>, args: Rest<Value<'js>>) -> String {
        let mut parts = Vec::with_capacity(args.0.len());
        for arg in args.0 {
            if let Some(s) = arg.as_string()
                && let Ok(s_str) = s.to_string()
            {
                parts.push(s_str);
                continue;
            }

            if (arg.is_object() || arg.is_array())
                && let Ok(json) = ctx.globals().get::<_, Object>("JSON")
                && let Ok(stringify) = json.get::<_, Function>("stringify")
                && let Ok(s) = stringify.call::<_, String>((arg.clone(),))
            {
                parts.push(s);
                continue;
            }

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
        if let Err(e) = globals.set("__ui", ui_obj) {
            crate::log_error!("Failed to inject __ui global: {:?}", e);
        }
    }

    // =========================================================================
    // 注入底层磁盘缓存 API：__cache 对象（get / set / delete / take）
    // =========================================================================
    let cache_dir = crate::sugg_root().join("cache").join("cmd_cache");
    let disk_cache = std::sync::Arc::new(crate::cache::DiskCache::new(cache_dir));

    if let Ok(cache_obj) = Object::new(ctx.clone()) {
        let dc = disk_cache.clone();
        if let Ok(f) = Function::new(ctx.clone(), move |key: String| {
            dc.get(&key).unwrap_or_default()
        }) {
            let _ = cache_obj.set("get", f);
        }
        let dc = disk_cache.clone();
        if let Ok(f) = Function::new(ctx.clone(), move |key: String, val: String, ttl_ms: u64| {
            let _ = dc.set(&key, &val, ttl_ms / 1000);
        }) {
            let _ = cache_obj.set("set", f);
        }
        let dc = disk_cache.clone();
        if let Ok(f) = Function::new(ctx.clone(), move |key: String| {
            dc.delete(&key);
        }) {
            let _ = cache_obj.set("delete", f);
        }
        if let Err(e) = globals.set("__cache", cache_obj) {
            crate::log_error!("Failed to inject __cache global: {:?}", e);
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
                .eval("typeof globalThis.__readFile === 'function'")
                .unwrap();
            assert!(is_func, "__readFile should be injected");

            let is_scan_path_func: bool = ctx
                .eval("typeof globalThis.__scanPath === 'function'")
                .unwrap();
            assert!(is_scan_path_func, "__scanPath should be injected");

            let is_exec_func: bool = ctx.eval("typeof globalThis.__exec === 'function'").unwrap();
            assert!(is_exec_func, "__exec should be injected");

            let is_exec_file_func: bool = ctx
                .eval("typeof globalThis.__execFile === 'function'")
                .unwrap();
            assert!(is_exec_file_func, "__execFile should be injected");

            let is_fetch_func: bool = ctx
                .eval("typeof globalThis.__fetch_raw === 'function'")
                .unwrap();
            assert!(is_fetch_func, "__fetch_raw should be injected");
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

            let script = r#"
                (async () => {
                    const out = await __execFile("rustc", ["--version"]);
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

            #[cfg(unix)]
            {
                let script2 = r#"
                    (async () => {
                        const out = await __execFile("/bin/echo", ["hello", "world"]);
                        return out.trim();
                    })()
                "#;
                let promise2: rquickjs::Promise = ctx.eval(script2).unwrap();
                let result2: String = promise2.into_future().await.unwrap();
                assert_eq!(result2, "hello world");
            }

            #[cfg(windows)]
            {
                let script2 = r#"
                    (async () => {
                        const out = await __execFile("cmd.exe", ["/c", "echo", "hello"]);
                        return out.trim();
                    })()
                "#;
                let promise2: rquickjs::Promise = ctx.eval(script2).unwrap();
                let result2: String = promise2.into_future().await.unwrap();
                assert_eq!(result2, "hello");
            }

            {
                let script3 = r#"
                    (async () => {
                        const out = await __execFile("rustc");
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

            #[cfg(unix)]
            {
                let script4 = r#"
                    (async () => {
                        const out = await __execFile("/bin/echo", []);
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

            let script = r#"
                (async () => {
                    const out = await __execFile("nonexistent_command_xyz", ["--version"]);
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

            let script1 = format!("__scanPath('bi', '{}')", base_str);
            let promise1: rquickjs::Promise = ctx.eval(script1.as_str()).unwrap();
            let items1: Vec<rquickjs::Object> = promise1.into_future().await.unwrap();
            let d1: Vec<String> = items1.iter().map(|o| o.get("display").unwrap()).collect();
            assert_eq!(d1, vec!["bin/"]);

            let script2 = format!("globalThis.__scanPath('', '{}/bin')", base_str);
            let promise2: rquickjs::Promise = ctx.eval(script2.as_str()).unwrap();
            let items2: Vec<rquickjs::Object> = promise2.into_future().await.unwrap();
            let d2: Vec<String> = items2.iter().map(|o| o.get("display").unwrap()).collect();
            assert_eq!(d2, vec!["sub/", "eslint"]);

            let script3 = format!("globalThis.__scanPath('sub/', '{}/bin')", base_str);
            let promise3: rquickjs::Promise = ctx.eval(script3.as_str()).unwrap();
            let items3: Vec<rquickjs::Object> = promise3.into_future().await.unwrap();
            let d3: Vec<String> = items3.iter().map(|o| o.get("display").unwrap()).collect();
            assert_eq!(d3, vec!["sub/foo"]);
        }

        async_with!(ctx => |ctx| { async_with_fn(ctx).await }).await;
    }

    #[tokio::test]
    async fn test_fetch_raw() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind");
        let port = listener.local_addr().unwrap().port();

        tokio::spawn(async move {
            use tokio::io::{AsyncReadExt, AsyncWriteExt};
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut buf = [0u8; 4096];
            let _ = stream.read(&mut buf).await;
            let body = r#"{"message":"ok"}"#;
            let resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            stream.write_all(resp.as_bytes()).await.unwrap();
        });

        let rt = AsyncRuntime::new().unwrap();
        let ctx = AsyncContext::full(&rt).await.unwrap();

        async fn run(ctx: Ctx<'_>, port: u16) {
            inject_globals(ctx.clone());

            let script = format!(
                r#"
                (async () => {{
                    const raw = await __fetch_raw({{ url: "http://127.0.0.1:{}/test", timeout: 3000 }});
                    return raw;
                }})()
                "#,
                port
            );
            let promise: rquickjs::Promise = ctx.eval(script.as_str()).unwrap();
            let raw: String = promise.into_future().await.unwrap();
            assert!(!raw.is_empty(), "raw response should not be empty");
            let parsed: serde_json::Value = serde_json::from_str(&raw).unwrap();
            assert_eq!(parsed["status"], 200);
            let body: String = serde_json::from_value(parsed["body"].clone()).unwrap();
            assert_eq!(body, r#"{"message":"ok"}"#);
        }

        async_with!(ctx => |ctx| { run(ctx, port).await }).await;
    }

    #[tokio::test]
    async fn test_fetch_raw_timeout() {
        let rt = AsyncRuntime::new().unwrap();
        let ctx = AsyncContext::full(&rt).await.unwrap();

        async fn run(ctx: Ctx<'_>) {
            inject_globals(ctx.clone());

            let script = r#"
                (async () => {
                    const raw = await __fetch_raw({ url: "http://127.0.0.1:1/nonexistent", timeout: 100 });
                    return raw === "";
                })()
            "#;
            let promise: rquickjs::Promise = ctx.eval(script).unwrap();
            let timed_out: bool = promise.into_future().await.unwrap();
            assert!(timed_out, "timeout should return empty string");
        }

        async_with!(ctx => |ctx| { run(ctx).await }).await;
    }

    #[tokio::test]
    async fn test_cache_global() {
        let rt = AsyncRuntime::new().unwrap();
        let ctx = AsyncContext::full(&rt).await.unwrap();

        async fn run(ctx: Ctx<'_>) {
            inject_globals(ctx.clone());

            let ok: bool = ctx
                .eval("typeof globalThis.__cache === 'object' && typeof globalThis.__cache.get === 'function'")
                .unwrap();
            assert!(ok);

            let result: String = ctx
                .eval(
                    r#"
                    globalThis.__cache.set("test_key", "hello", 60000);
                    globalThis.__cache.get("test_key")
                "#,
                )
                .unwrap();
            assert_eq!(result, "hello");

            let result: String = ctx
                .eval(
                    r#"
                    globalThis.__cache.delete("test_key");
                    globalThis.__cache.get("test_key")
                "#,
                )
                .unwrap();
            assert_eq!(result, "");

            let result: String = ctx
                .eval(
                    r#"
                    globalThis.__cache.set("exp_key", "v", 0);
                    globalThis.__cache.get("exp_key")
                "#,
                )
                .unwrap();
            assert_eq!(result, "");

            let result: bool = ctx
                .eval(
                    r#"
                    globalThis.__cache.set("a\0b", "1", 60000);
                    globalThis.__cache.set("a\0c", "2", 60000);
                    globalThis.__cache.get("a\0b") === "1" && globalThis.__cache.get("a\0c") === "2"
                "#,
                )
                .unwrap();
            assert!(result);
        }

        async_with!(ctx => |ctx| { run(ctx).await }).await;
    }
}
