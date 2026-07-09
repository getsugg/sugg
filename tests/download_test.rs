#![cfg(feature = "install-tests")]

use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::{Read, Write};
use std::net::TcpListener;

struct MockServer {
    port: u16,
}

impl MockServer {
    fn with_files(files: HashMap<String, String>) -> Self {
        Self::with_files_failing(files, HashSet::new())
    }

    fn with_files_failing(files: HashMap<String, String>, failing_paths: HashSet<String>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();

        std::thread::spawn(move || {
            for stream in listener.incoming() {
                if let Ok(mut stream) = stream {
                    let mut buf = [0u8; 4096];
                    let _ = stream.read(&mut buf);

                    let request = String::from_utf8_lossy(&buf);
                    let path = request
                        .lines()
                        .next()
                        .and_then(|line| line.split_whitespace().nth(1))
                        .unwrap_or("/");

                    if failing_paths.contains(path) {
                        let msg = "Internal Server Error";
                        let response = format!(
                            "HTTP/1.1 500 Internal Server Error\r\nContent-Type: text/plain\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                            msg.len(),
                            msg
                        );
                        let _ = stream.write_all(response.as_bytes());
                    } else if let Some(body) = files.get(path) {
                        let response = format!(
                            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                            body.len(),
                            body
                        );
                        let _ = stream.write_all(response.as_bytes());
                    } else {
                        let msg = format!("Not found: {}", path);
                        let response = format!(
                            "HTTP/1.1 404 Not Found\r\nContent-Type: text/plain\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                            msg.len(),
                            msg
                        );
                        let _ = stream.write_all(response.as_bytes());
                    }
                }
            }
        });

        Self { port }
    }

    fn url(&self) -> String {
        format!("http://127.0.0.1:{}", self.port)
    }

    fn registry_url(&self) -> String {
        format!("{}/registry.json", self.url())
    }
}

fn registry(name: &str, source: &str, deps: &[&str], i18n: &[(&str, &[&str])]) -> String {
    let deps_json: Vec<String> = deps.iter().map(|d| format!("\"{}\"", d)).collect();
    let i18n_entries: Vec<String> = i18n
        .iter()
        .map(|(ns, langs)| {
            let langs_json: Vec<String> = langs.iter().map(|l| format!("\"{}\"", l)).collect();
            format!("\"{}\": [{}]", ns, langs_json.join(","))
        })
        .collect();
    format!(
        r#"{{"scripts":[{{"name":"{}","description":"{}","source":"{}","deps":[{}],"i18n":{{{}}}}}]}}"#,
        name,
        name,
        source,
        deps_json.join(","),
        i18n_entries.join(",")
    )
}

fn multi_registry(entries: &[(&str, &str, Vec<&str>, Vec<(&str, Vec<&str>)>)]) -> String {
    let scripts: Vec<String> = entries
        .iter()
        .map(|(name, source, deps, i18n)| {
            let deps_json: Vec<String> = deps.iter().map(|d| format!("\"{}\"", d)).collect();
            let i18n_entries: Vec<String> = i18n
                .iter()
                .map(|(ns, langs)| {
                    let langs_json: Vec<String> =
                        langs.iter().map(|l| format!("\"{}\"", l)).collect();
                    format!("\"{}\": [{}]", ns, langs_json.join(","))
                })
                .collect();
            format!(
                r#"{{"name":"{}","description":"{}","source":"{}","deps":[{}],"i18n":{{{}}}}}"#,
                name,
                name,
                source,
                deps_json.join(","),
                i18n_entries.join(",")
            )
        })
        .collect();
    format!(r#"{{"scripts":[{}]}}"#, scripts.join(","))
}

#[tokio::test]
async fn test_list_scripts() {
    let server = MockServer::with_files(HashMap::from([(
        "/registry.json".to_string(),
        registry("git", "git/index.ts", &[], &[]),
    )]));

    let dir = tempfile::tempdir().unwrap();
    let completions_dir = dir.path().join("completions");

    let result = sugg_engine::install::run_install(
        vec![],
        true,
        false,
        false,
        &[],
        &completions_dir,
        &server.registry_url(),
        &server.url(),
    )
    .await;

    assert!(result.is_ok());
}

#[tokio::test]
async fn test_install_single_script() {
    let server = MockServer::with_files(HashMap::from([
        (
            "/registry.json".to_string(),
            registry("git", "git/index.ts", &[], &[("git", &["en"])]),
        ),
        (
            "/git/index.ts".to_string(),
            "export default { git: {} }".to_string(),
        ),
        ("/git/i18n/en.json".to_string(), "{}".to_string()),
    ]));

    let dir = tempfile::tempdir().unwrap();
    let completions_dir = dir.path().join("completions");

    let result = sugg_engine::install::run_install(
        vec!["git".to_string()],
        false,
        false,
        false,
        &["en".to_string()],
        &completions_dir,
        &server.registry_url(),
        &server.url(),
    )
    .await;

    assert!(result.is_ok());
    assert!(completions_dir.join("git/index.ts").exists());
}

#[tokio::test]
async fn test_install_script_not_found() {
    let server = MockServer::with_files(HashMap::from([(
        "/registry.json".to_string(),
        registry("git", "git/index.ts", &[], &[("git", &["en"])]),
    )]));

    let dir = tempfile::tempdir().unwrap();
    let completions_dir = dir.path().join("completions");

    let result = sugg_engine::install::run_install(
        vec!["nonexistent".to_string()],
        false,
        false,
        false,
        &["en".to_string()],
        &completions_dir,
        &server.registry_url(),
        &server.url(),
    )
    .await;

    assert!(result.is_err());
    let msg = result.unwrap_err().to_string();
    assert!(msg.contains("not found"), "error: {}", msg);
}

#[tokio::test]
async fn test_install_no_scripts_no_all() {
    let server = MockServer::with_files(HashMap::from([(
        "/registry.json".to_string(),
        registry("git", "git/index.ts", &[], &[]),
    )]));

    let dir = tempfile::tempdir().unwrap();
    let completions_dir = dir.path().join("completions");

    let result = sugg_engine::install::run_install(
        vec![],
        false,
        false,
        false,
        &[],
        &completions_dir,
        &server.registry_url(),
        &server.url(),
    )
    .await;

    assert!(result.is_err());
}

#[tokio::test]
async fn test_install_skip_existing() {
    let server = MockServer::with_files(HashMap::from([
        (
            "/registry.json".to_string(),
            registry("git", "git/index.ts", &[], &[("git", &["en"])]),
        ),
        ("/git/index.ts".to_string(), "export default {}".to_string()),
        ("/git/i18n/en.json".to_string(), "{}".to_string()),
    ]));

    let dir = tempfile::tempdir().unwrap();
    let completions_dir = dir.path().join("completions");
    fs::create_dir_all(completions_dir.join("git")).unwrap();
    fs::write(completions_dir.join("git/index.ts"), "original").unwrap();

    let result = sugg_engine::install::run_install(
        vec!["git".to_string()],
        false,
        false,
        false,
        &["en".to_string()],
        &completions_dir,
        &server.registry_url(),
        &server.url(),
    )
    .await;

    assert!(result.is_ok());
    assert_eq!(
        fs::read_to_string(completions_dir.join("git/index.ts")).unwrap(),
        "original"
    );
}

#[tokio::test]
async fn test_install_force_overwrite() {
    let server = MockServer::with_files(HashMap::from([
        (
            "/registry.json".to_string(),
            registry("git", "git/index.ts", &[], &[("git", &["en"])]),
        ),
        (
            "/git/index.ts".to_string(),
            "export default { git: {} }".to_string(),
        ),
        ("/git/i18n/en.json".to_string(), "{}".to_string()),
    ]));

    let dir = tempfile::tempdir().unwrap();
    let completions_dir = dir.path().join("completions");
    fs::create_dir_all(completions_dir.join("git")).unwrap();
    fs::write(completions_dir.join("git/index.ts"), "original").unwrap();

    let result = sugg_engine::install::run_install(
        vec!["git".to_string()],
        false,
        false,
        true,
        &["en".to_string()],
        &completions_dir,
        &server.registry_url(),
        &server.url(),
    )
    .await;

    assert!(result.is_ok());
    assert_eq!(
        fs::read_to_string(completions_dir.join("git/index.ts")).unwrap(),
        "export default { git: {} }"
    );
}

#[tokio::test]
async fn test_install_with_deps() {
    let server = MockServer::with_files(HashMap::from([
        (
            "/registry.json".to_string(),
            registry(
                "git",
                "git/index.ts",
                &["npm/utils.ts"],
                &[("git", &["en"])],
            ),
        ),
        ("/git/index.ts".to_string(), "export default {}".to_string()),
        (
            "/npm/utils.ts".to_string(),
            "export const foo = 1".to_string(),
        ),
        ("/git/i18n/en.json".to_string(), "{}".to_string()),
    ]));

    let dir = tempfile::tempdir().unwrap();
    let completions_dir = dir.path().join("completions");

    let result = sugg_engine::install::run_install(
        vec!["git".to_string()],
        false,
        false,
        false,
        &["en".to_string()],
        &completions_dir,
        &server.registry_url(),
        &server.url(),
    )
    .await;

    assert!(result.is_ok());
    assert!(completions_dir.join("git/index.ts").exists());
    assert!(completions_dir.join("npm/utils.ts").exists());
}

#[tokio::test]
async fn test_install_i18n_selective() {
    let server = MockServer::with_files(HashMap::from([
        (
            "/registry.json".to_string(),
            registry("git", "git/index.ts", &[], &[("git", &["en", "zh-CN"])]),
        ),
        ("/git/index.ts".to_string(), "export default {}".to_string()),
        ("/git/i18n/en.json".to_string(), r#"{"k":"en"}"#.to_string()),
        (
            "/git/i18n/zh-CN.json".to_string(),
            r#"{"k":"zh"}"#.to_string(),
        ),
    ]));

    let dir = tempfile::tempdir().unwrap();
    let completions_dir = dir.path().join("completions");

    let result = sugg_engine::install::run_install(
        vec!["git".to_string()],
        false,
        false,
        false,
        &["en".to_string()],
        &completions_dir,
        &server.registry_url(),
        &server.url(),
    )
    .await;

    assert!(result.is_ok());
    assert!(completions_dir.join("git/index.ts").exists());
    assert!(completions_dir.join("git/i18n/en.json").exists());
    assert!(!completions_dir.join("git/i18n/zh-CN.json").exists());
}

#[tokio::test]
async fn test_download_failure_with_retry() {
    let mut failing = HashSet::new();
    failing.insert("/git/index.ts".to_string());

    let server = MockServer::with_files_failing(
        HashMap::from([
            (
                "/registry.json".to_string(),
                registry("git", "git/index.ts", &[], &[("git", &["en"])]),
            ),
            ("/git/index.ts".to_string(), "export default {}".to_string()),
            ("/git/i18n/en.json".to_string(), "{}".to_string()),
        ]),
        failing,
    );

    let dir = tempfile::tempdir().unwrap();
    let completions_dir = dir.path().join("completions");

    let result = sugg_engine::install::run_install(
        vec!["git".to_string()],
        false,
        false,
        false,
        &["en".to_string()],
        &completions_dir,
        &server.registry_url(),
        &server.url(),
    )
    .await;

    assert!(
        result.is_err(),
        "should fail when download returns 500 repeatedly"
    );
    let msg = result.unwrap_err().to_string();
    assert!(msg.contains("500"), "error should mention 500: {}", msg);
}

#[tokio::test]
async fn test_concurrent_download_multiple_scripts() {
    let server = MockServer::with_files(HashMap::from([
        (
            "/registry.json".to_string(),
            multi_registry(&[
                ("git", "git/index.ts", vec![], vec![("git", vec!["en"])]),
                (
                    "docker",
                    "docker/index.ts",
                    vec!["npm/utils.ts"],
                    vec![("docker", vec!["en"])],
                ),
                ("bun", "bun/index.ts", vec![], vec![("bun", vec!["en"])]),
            ]),
        ),
        (
            "/git/index.ts".to_string(),
            "export default { git: {} }".to_string(),
        ),
        (
            "/docker/index.ts".to_string(),
            "export default { docker: {} }".to_string(),
        ),
        (
            "/bun/index.ts".to_string(),
            "export default { bun: {} }".to_string(),
        ),
        (
            "/npm/utils.ts".to_string(),
            "export const foo = 1".to_string(),
        ),
        ("/git/i18n/en.json".to_string(), "{}".to_string()),
        ("/docker/i18n/en.json".to_string(), "{}".to_string()),
        ("/bun/i18n/en.json".to_string(), "{}".to_string()),
    ]));

    let dir = tempfile::tempdir().unwrap();
    let completions_dir = dir.path().join("completions");

    let result = sugg_engine::install::run_install(
        vec!["git".to_string(), "docker".to_string(), "bun".to_string()],
        false,
        false,
        false,
        &["en".to_string()],
        &completions_dir,
        &server.registry_url(),
        &server.url(),
    )
    .await;

    assert!(result.is_ok());
    assert!(completions_dir.join("git/index.ts").exists());
    assert!(completions_dir.join("docker/index.ts").exists());
    assert!(completions_dir.join("bun/index.ts").exists());
    assert!(completions_dir.join("npm/utils.ts").exists());
    assert!(completions_dir.join("git/i18n/en.json").exists());
    assert!(completions_dir.join("docker/i18n/en.json").exists());
    assert!(completions_dir.join("bun/i18n/en.json").exists());
}
