use std::sync::OnceLock;

use rquickjs::{
    Ctx, Function, Object, Value,
    function::{Async, Opt, Rest},
};

fn http_agent() -> &'static ureq::Agent {
    static AGENT: OnceLock<ureq::Agent> = OnceLock::new();
    AGENT.get_or_init(|| {
        let config = ureq::Agent::config_builder()
            .timeout_global(Some(std::time::Duration::from_secs(10)))
            .build();
        config.into()
    })
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
                    Ok(content) => serde_json::json!({"ok": true, "data": content}).to_string(),
                    Err(e) => serde_json::json!({"ok": false, "error": format!("{e}")}).to_string(),
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

                match tokio::fs::read_dir(&scan_path).await {
                    Ok(mut dir) => {
                        let mut entries = Vec::with_capacity(32);
                        while let Ok(Some(entry)) = dir.next_entry().await {
                            let name = entry.file_name().to_string_lossy().into_owned();
                            let is_dir = is_dir_fast(&entry).await;

                            let full = format!("{}{}", p_dir, name);

                            let display = if is_dir {
                                format!("{}/", full)
                            } else {
                                full.clone()
                            };
                            let value = if is_dir {
                                format!("{}/", full)
                            } else {
                                format!("{} ", full)
                            };
                            let mut item = serde_json::json!({
                                "display": display,
                                "value": value,
                                "isDir": is_dir,
                            });
                            if is_dir {
                                item["style"] = serde_json::json!({"fg": "blue"});
                            }
                            entries.push(item);
                        }
                        entries.sort_by(|a, b| {
                            let a_dir = a["isDir"].as_bool().unwrap_or(false);
                            let b_dir = b["isDir"].as_bool().unwrap_or(false);
                            b_dir.cmp(&a_dir).then_with(|| {
                                a["display"]
                                    .as_str()
                                    .unwrap_or("")
                                    .cmp(b["display"].as_str().unwrap_or(""))
                            })
                        });
                        serde_json::json!({"ok": true, "data": entries}).to_string()
                    }
                    Err(e) => {
                        if e.kind() == std::io::ErrorKind::NotFound {
                            return serde_json::json!({"ok": true, "data": []}).to_string();
                        }
                        serde_json::json!({"ok": false, "error": format!("{e}")}).to_string()
                    }
                }
            }),
        ),
    ) {
        crate::log_error!("Failed to inject scanPath global: {:?}", e);
    }

    if let Err(e) = globals.set(
        "__exec",
        Function::new(
            ctx.clone(),
            Async(|command: String, timeout_ms: u64| async move {
                let fut = async {
                    #[cfg(target_os = "windows")]
                    {
                        use std::os::windows::process::CommandExt;
                        let mut std_cmd = std::process::Command::new("cmd");
                        std_cmd.arg("/C");
                        std_cmd.raw_arg(&command);
                        tokio::process::Command::from(std_cmd).output().await
                    }

                    #[cfg(not(target_os = "windows"))]
                    {
                        tokio::process::Command::new("sh")
                            .args(&["-c", &command])
                            .output()
                            .await
                    }
                };
                match tokio::time::timeout(std::time::Duration::from_millis(timeout_ms), fut).await
                {
                    Ok(Ok(o)) => {
                        serde_json::json!({"ok": true, "data": String::from_utf8_lossy(&o.stdout)}).to_string()
                    }
                    Ok(Err(e)) => {
                        serde_json::json!({"ok": false, "error": format!("{e:?}")}).to_string()
                    }
                    Err(_) => {
                        serde_json::json!({"ok": false, "error": format!("timed out after {timeout_ms}ms")}).to_string()
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
            Async(
                |cmd: String, args: Vec<String>, timeout_ms: u64| async move {
                    match tokio::time::timeout(
                        std::time::Duration::from_millis(timeout_ms),
                        tokio::process::Command::new(&cmd).args(&args).output(),
                    )
                    .await
                    {
                        Ok(Ok(o)) => {
                            serde_json::json!({"ok": true, "data": String::from_utf8_lossy(&o.stdout)}).to_string()
                        }
                        Ok(Err(e)) => {
                            serde_json::json!({"ok": false, "error": format!("{e:?}")}).to_string()
                        }
                        Err(_) => {
                            serde_json::json!({"ok": false, "error": format!("timed out after {timeout_ms}ms")}).to_string()
                        }
                    }
                },
            ),
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
                let timeout_ms: u64 = config_obj.get("timeout").unwrap_or(5000);
                async move {
                    let timeout = std::time::Duration::from_millis(timeout_ms);

                    let result = tokio::time::timeout(
                        timeout,
                        tokio::task::spawn_blocking(move || {
                            use std::str::FromStr;
                            let http_method = ureq::http::Method::from_str(&method_str)
                                .unwrap_or(ureq::http::Method::GET);

                            let mut builder =
                                ureq::http::Request::builder().method(http_method).uri(&url);
                            for (k, v) in &headers {
                                builder = builder.header(k.as_str(), v.as_str());
                            }
                            let request = builder.body(body).unwrap();
                            let agent = http_agent();
                            match agent.run(request) {
                                Ok(mut resp) => {
                                    let status = resp.status().as_u16();
                                    let status_text =
                                        resp.status().canonical_reason().unwrap_or("").to_string();
                                    let mut resp_headers = std::collections::HashMap::new();
                                    for (name, value) in resp.headers() {
                                        if let Ok(v) = value.to_str() {
                                            resp_headers.insert(
                                                name.as_str().to_lowercase(),
                                                v.to_string(),
                                            );
                                        }
                                    }
                                    let resp_body =
                                        resp.body_mut().read_to_string().unwrap_or_default();
                                    serde_json::json!({
                                        "ok": true,
                                        "data": {
                                            "status": status,
                                            "statusText": status_text,
                                            "headers": resp_headers,
                                            "body": resp_body
                                        }
                                    })
                                    .to_string()
                                }
                                Err(e) => serde_json::json!({
                                    "ok": false,
                                    "error": format!("{:?}", e)
                                })
                                .to_string(),
                            }
                        }),
                    )
                    .await;

                    match result {
                        Ok(Ok(json)) => json,
                        Ok(Err(_)) => {
                            serde_json::json!({"ok": false, "error": "join error"}).to_string()
                        }
                        Err(_) => serde_json::json!({"ok": false, "error": "timeout"}).to_string(),
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
    use rquickjs::{AsyncContext, AsyncRuntime};

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

        ctx.async_with(async |ctx| async_with_fn(ctx).await).await;
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
                    const raw = await __execFile("rustc", ["--version"], 5000);
                    const parsed = JSON.parse(raw);
                    if (!parsed.ok) throw new Error(parsed.error);
                    return parsed.data.trim();
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
                        const raw = await __execFile("/bin/echo", ["hello", "world"], 5000);
                        const parsed = JSON.parse(raw);
                        if (!parsed.ok) throw new Error(parsed.error);
                        return parsed.data.trim();
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
                        const raw = await __execFile("cmd.exe", ["/c", "echo", "hello"], 5000);
                        const parsed = JSON.parse(raw);
                        if (!parsed.ok) throw new Error(parsed.error);
                        return parsed.data.trim();
                    })()
                "#;
                let promise2: rquickjs::Promise = ctx.eval(script2).unwrap();
                let result2: String = promise2.into_future().await.unwrap();
                assert_eq!(result2, "hello");
            }
        }

        ctx.async_with(async |ctx| async_with_fn(ctx).await).await;
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
                    const raw = await __execFile("nonexistent_command_xyz", ["--version"], 5000);
                    const parsed = JSON.parse(raw);
                    return JSON.stringify({ ok: parsed.ok, error: parsed.error || "" });
                })()
            "#;
            let promise: rquickjs::Promise = ctx.eval(script).unwrap();
            let result: String = promise.into_future().await.unwrap();
            let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();
            assert!(
                !parsed["ok"].as_bool().unwrap(),
                "不存在的命令应返回 ok=false"
            );
            assert!(
                !parsed["error"].as_str().unwrap_or("").is_empty(),
                "应包含错误信息"
            );
        }

        ctx.async_with(async |ctx| async_with_fn(ctx).await).await;
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

            let script1 = format!(
                r#"(async () => {{ const r = JSON.parse(await __scanPath('bi', '{}')); if (!r.ok) throw new Error(r.error); return JSON.stringify(r.data); }})()"#,
                base_str
            );
            let promise1: rquickjs::Promise = ctx.eval(script1.as_str()).unwrap();
            let items1: String = promise1.into_future().await.unwrap();
            let items1: Vec<serde_json::Value> = serde_json::from_str(&items1).unwrap();
            let d1: Vec<String> = items1
                .iter()
                .map(|o| o["display"].as_str().unwrap().to_string())
                .collect();
            assert_eq!(d1, vec!["bin/"]);

            let script2 = format!(
                r#"(async () => {{ const r = JSON.parse(await globalThis.__scanPath('', '{}/bin')); if (!r.ok) throw new Error(r.error); return JSON.stringify(r.data); }})()"#,
                base_str
            );
            let promise2: rquickjs::Promise = ctx.eval(script2.as_str()).unwrap();
            let items2: String = promise2.into_future().await.unwrap();
            let items2: Vec<serde_json::Value> = serde_json::from_str(&items2).unwrap();
            let d2: Vec<String> = items2
                .iter()
                .map(|o| o["display"].as_str().unwrap().to_string())
                .collect();
            assert_eq!(d2, vec!["sub/", "eslint"]);

            let script3 = format!(
                r#"(async () => {{ const r = JSON.parse(await globalThis.__scanPath('sub/', '{}/bin')); if (!r.ok) throw new Error(r.error); return JSON.stringify(r.data); }})()"#,
                base_str
            );
            let promise3: rquickjs::Promise = ctx.eval(script3.as_str()).unwrap();
            let items3: String = promise3.into_future().await.unwrap();
            let items3: Vec<serde_json::Value> = serde_json::from_str(&items3).unwrap();
            let d3: Vec<String> = items3
                .iter()
                .map(|o| o["display"].as_str().unwrap().to_string())
                .collect();
            assert_eq!(d3, vec!["sub/foo"]);
        }

        ctx.async_with(async |ctx| async_with_fn(ctx).await).await;
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
                    const parsed = JSON.parse(raw);
                    if (!parsed.ok) throw new Error(parsed.error);
                    return JSON.stringify(parsed.data);
                }})()
                "#,
                port
            );
            let promise: rquickjs::Promise = ctx.eval(script.as_str()).unwrap();
            let raw: String = promise.into_future().await.unwrap();
            let parsed: serde_json::Value = serde_json::from_str(&raw).unwrap();
            assert_eq!(parsed["status"], 200);
            let body: String = serde_json::from_value(parsed["body"].clone()).unwrap();
            assert_eq!(body, r#"{"message":"ok"}"#);
        }

        ctx.async_with(async |ctx| run(ctx, port).await).await;
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
                    const parsed = JSON.parse(raw);
                    return JSON.stringify({ ok: parsed.ok, error: parsed.error || "" });
                })()
            "#;
            let promise: rquickjs::Promise = ctx.eval(script).unwrap();
            let raw: String = promise.into_future().await.unwrap();
            let parsed: serde_json::Value = serde_json::from_str(&raw).unwrap();
            assert!(
                !parsed["ok"].as_bool().unwrap(),
                "should return ok=false on timeout"
            );
            let error_msg = parsed["error"].as_str().unwrap_or("");
            assert!(!error_msg.is_empty(), "error message should not be empty");
        }

        ctx.async_with(async |ctx| run(ctx).await).await;
    }

    #[tokio::test]
    async fn test_exec_file_timeout() {
        let rt = AsyncRuntime::new().unwrap();
        let ctx = AsyncContext::full(&rt).await.unwrap();

        async fn run(ctx: Ctx<'_>) {
            inject_globals(ctx.clone());

            #[cfg(windows)]
            let script = r#"
                (async () => {
                    const raw = await __execFile("ping", ["-n", "10", "127.0.0.1"], 100);
                    const parsed = JSON.parse(raw);
                    return JSON.stringify({ ok: parsed.ok, error: parsed.error || "" });
                })()
            "#;
            #[cfg(not(windows))]
            let script = r#"
                (async () => {
                    const raw = await __execFile("sleep", ["10"], 100);
                    const parsed = JSON.parse(raw);
                    return JSON.stringify({ ok: parsed.ok, error: parsed.error || "" });
                })()
            "#;
            let promise: rquickjs::Promise = ctx.eval(script).unwrap();
            let raw: String = promise.into_future().await.unwrap();
            let parsed: serde_json::Value = serde_json::from_str(&raw).unwrap();
            assert!(
                !parsed["ok"].as_bool().unwrap(),
                "should return ok=false on timeout"
            );
            assert!(
                !parsed["error"].as_str().unwrap_or("").is_empty(),
                "error message should not be empty"
            );
        }

        ctx.async_with(async |ctx| run(ctx).await).await;
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

        ctx.async_with(async |ctx| run(ctx).await).await;
    }
}
