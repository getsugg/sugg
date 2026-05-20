import * as t from "virtual:i18n/bun";

// ---------- 通用辅助函数（基于 bun getcompletes）----------
// 执行 bun getcompletes 并解析结果
async function getCompletes(type: "z" | "a" | "b"): Promise<string[]> {
  try {
    const out = await exec(`bun getcompletes ${type}`);
    if (type === "z") {
      // scripts 输出格式：name\tdescription
      return out
        .trim()
        .split("\n")
        .filter((line) => line.includes("\t"))
        .map((line) => line.split("\t")[0]);
    } else {
      // a, b 输出空格分隔的列表
      return out.trim().split(/\s+/).filter(Boolean);
    }
  } catch {
    return [];
  }
}

// 获取 package.json 中的脚本名（使用 getcompletes z）
async function getScriptNames(): Promise<Suggestion[]> {
  const scripts = await getCompletes("z");
  return scripts.map((s) => ({ display: s, description: t.suggestion_script }));
}

// 获取可执行二进制名（bins）
async function getBinNames(): Promise<Suggestion[]> {
  const bins = await getCompletes("b");
  return bins.map((b) => ({ display: b, description: t.suggestion_package_bin }));
}

// 获取 JS/TS 文件（支持文件夹前缀补全，默认不显示）
async function getScriptFiles(ctx: CompletionContext) {
  const allFiles = await scanPath(ctx.prefix);
  const jsTsFiles = allFiles.filter(
    (f) => /\.(ts|js|tsx|jsx|mjs|cjs)$/i.test(f.display) || f.isDir,
  );
  return jsTsFiles;
}

// 获取测试文件（同样规则，但只显示 .test. 或 .spec. 文件）
async function getTestFiles(ctx: CompletionContext): Promise<Suggestion[]> {
  const allFiles = await scanPath(ctx.prefix);
  const testFiles = allFiles.filter(
    (f) => /\.(test|spec)\.(ts|js|tsx|jsx)$/i.test(f.display) || (ctx.prefix !== "" && f.isDir),
  );
  return testFiles;
}

// 获取已安装的包名（从 package.json 读取，因为 getcompletes 无此功能）
async function getInstalledPackages(): Promise<Suggestion[]> {
  const pkg = await readJson("package.json");
  const deps = { ...pkg.dependencies, ...pkg.devDependencies };
  return Object.keys(deps).map((name) => ({ display: name, description: t.suggestion_installed }));
}

// 获取全局安装的包名（通过 bun pm ls -g）
async function getGlobalPackages(): Promise<Suggestion[]> {
  try {
    const out = await exec("bun pm ls -g");
    const lines = out.trim().split("\n");
    // 第一行是路径和总数，跳过；后续行格式：├── pkg@version 或 └── pkg@version
    const packages = lines
      .slice(1)
      .map((line) => line.replace(/^[├└][─ ]{2,}\s+/, "").trim())
      .filter(Boolean)
      .map((name) => name.replace(/@[^@]+$/, ""))
      .filter((name) => name.length > 0);
    return packages.map((name) => ({
      display: name,
      description: t.suggestion_global_installed,
    }));
  } catch {
    return [];
  }
}

// 获取用于 bun add 的包名（使用 getcompletes a，并传递当前输入前缀）
async function getAddPackages(ctx: CompletionContext): Promise<Suggestion[]> {
  try {
    // 注意：bun getcompletes a 支持传递前缀参数
    const out = await exec(`bun getcompletes a ${ctx.prefix}`);
    const names = out.trim().split(/\s+/).filter(Boolean);
    return names.map((name) => ({
      display: name,
      description: t.suggestion_package_from_registry,
    }));
  } catch {
    return [];
  }
}

// ---------- 全局选项 ----------
const commonGlobalOpts: OptionNode[] = [
  { labels: ["--watch"], description: t.option_watch },
  { labels: ["--hot"], description: t.option_hot },
  { labels: ["--smol"], description: t.option_smol },
  { labels: ["--no-clear-screen"], description: t.option_no_clear_screen },
  { labels: ["-r", "--preload"], args: [], description: t.option_preload },
  { labels: ["--inspect"], args: [], description: t.option_inspect },
  { labels: ["--inspect-wait"], args: [], description: t.option_inspect_wait },
  { labels: ["--inspect-brk"], args: [], description: t.option_inspect_brk },
  { labels: ["--cpu-prof"], description: t.option_cpu_prof },
  { labels: ["--heap-prof"], description: t.option_heap_prof },
  { labels: ["--if-present"], description: t.option_if_present },
  { labels: ["--no-install"], description: t.option_no_install },
  { labels: ["--install"], args: [], description: t.option_install },
  { labels: ["-i"], description: t.option_i },
  { labels: ["-e", "--eval"], args: [], description: t.option_eval },
  { labels: ["-p", "--print"], args: [], description: t.option_print },
  { labels: ["--prefer-offline"], description: t.option_prefer_offline },
  { labels: ["--prefer-latest"], description: t.option_prefer_latest },
  { labels: ["--port"], args: [], description: t.option_port },
  { labels: ["--conditions"], args: [], description: t.option_conditions },
  { labels: ["--fetch-preconnect"], args: [], description: t.option_fetch_preconnect },
  { labels: ["--max-http-header-size"], args: [], description: t.option_max_http_header_size },
  { labels: ["--dns-result-order"], args: [], description: t.option_dns_result_order },
  { labels: ["--expose-gc"], description: t.option_expose_gc },
  { labels: ["--no-deprecation"], description: t.option_no_deprecation },
  { labels: ["--throw-deprecation"], description: t.option_throw_deprecation },
  { labels: ["--title"], args: [], description: t.option_title },
  { labels: ["--zero-fill-buffers"], description: t.option_zero_fill_buffers },
  { labels: ["--use-system-ca"], description: t.option_use_system_ca },
  { labels: ["--use-openssl-ca"], description: t.option_use_openssl_ca },
  { labels: ["--use-bundled-ca"], description: t.option_use_bundled_ca },
  { labels: ["--redis-preconnect"], description: t.option_redis_preconnect },
  { labels: ["--sql-preconnect"], description: t.option_sql_preconnect },
  { labels: ["--no-addons"], description: t.option_no_addons },
  { labels: ["--unhandled-rejections"], args: [], description: t.option_unhandled_rejections },
  { labels: ["--console-depth"], args: [], description: t.option_console_depth },
  { labels: ["--user-agent"], args: [], description: t.option_user_agent },
  { labels: ["--cron-title"], args: [], description: t.option_cron_title },
  { labels: ["--cron-period"], args: [], description: t.option_cron_period },
  { labels: ["--silent"], description: t.option_silent },
  { labels: ["--elide-lines"], args: [], description: t.option_elide_lines },
  { labels: ["-v", "--version"], description: t.option_version },
  { labels: ["--revision"], description: t.option_revision },
  { labels: ["-F", "--filter"], args: [], description: t.option_filter },
  { labels: ["-b", "--bun"], description: t.option_bun },
  { labels: ["--shell"], args: [], description: t.option_shell },
  { labels: ["--workspaces"], description: t.option_workspaces },
  { labels: ["--parallel"], description: t.option_parallel },
  { labels: ["--sequential"], description: t.option_sequential },
  { labels: ["--no-exit-on-error"], description: t.option_no_exit_on_error },
  { labels: ["--env-file"], args: [], description: t.option_env_file },
  { labels: ["--no-env-file"], description: t.option_no_env_file },
  { labels: ["--cwd"], args: [], description: t.option_cwd },
  { labels: ["-c", "--config"], args: [], description: t.option_config },
  { labels: ["-h", "--help"], description: t.option_help },
];

// ---------- 各命令专属选项 ----------
const testOptions: OptionNode[] = [
  { labels: ["--timeout"], args: [], description: t.test_option_timeout },
  { labels: ["-u", "--update-snapshots"], description: t.test_option_update_snapshots },
  { labels: ["--rerun-each"], args: [], description: t.test_option_rerun_each },
  { labels: ["--retry"], args: [], description: t.test_option_retry },
  { labels: ["--todo"], description: t.test_option_todo },
  { labels: ["--only"], description: t.test_option_only },
  { labels: ["--pass-with-no-tests"], description: t.test_option_pass_with_no_tests },
  { labels: ["--concurrent"], description: t.test_option_concurrent },
  { labels: ["--randomize"], description: t.test_option_randomize },
  { labels: ["--seed"], args: [], description: t.test_option_seed },
  { labels: ["--coverage"], description: t.test_option_coverage },
  { labels: ["--coverage-reporter"], args: [], description: t.test_option_coverage_reporter },
  { labels: ["--coverage-dir"], args: [], description: t.test_option_coverage_dir },
  { labels: ["--bail"], args: [], description: t.test_option_bail },
  { labels: ["-t", "--test-name-pattern"], args: [], description: t.test_option_test_name_pattern },
  { labels: ["--reporter"], args: [], description: t.test_option_reporter },
  { labels: ["--reporter-outfile"], args: [], description: t.test_option_reporter_outfile },
  { labels: ["--dots"], description: t.test_option_dots },
  { labels: ["--only-failures"], description: t.test_option_only_failures },
  { labels: ["--max-concurrency"], args: [], description: t.test_option_max_concurrency },
  { labels: ["--path-ignore-patterns"], args: [], description: t.test_option_path_ignore_patterns },
  { labels: ["--changed"], args: [], description: t.test_option_changed },
  { labels: ["--isolate"], description: t.test_option_isolate },
  { labels: ["--parallel"], args: [], description: t.test_option_parallel },
  { labels: ["--parallel-delay"], args: [], description: t.test_option_parallel_delay },
  { labels: ["--shard"], args: [], description: t.test_option_shard },
];

const xOptions: OptionNode[] = [
  { labels: ["--bun"], description: t.x_option_bun },
  { labels: ["-p", "--package"], args: [], description: t.x_option_package },
  { labels: ["--no-install"], description: t.x_option_no_install },
  { labels: ["--verbose"], description: t.x_option_verbose },
  { labels: ["--silent"], description: t.x_option_silent },
];

const installOptions: OptionNode[] = [
  { labels: ["-p", "--production"], description: t.install_option_production },
  { labels: ["--no-save"], description: t.install_option_no_save },
  { labels: ["--save"], description: t.install_option_save },
  { labels: ["--dry-run"], description: t.install_option_dry_run },
  { labels: ["--frozen-lockfile"], description: t.install_option_frozen_lockfile },
  { labels: ["-f", "--force"], description: t.install_option_force },
  { labels: ["--cache-dir"], args: [], description: t.install_option_cache_dir },
  { labels: ["--no-cache"], description: t.install_option_no_cache },
  { labels: ["--verbose"], description: t.install_option_verbose },
  { labels: ["--no-progress"], description: t.install_option_no_progress },
  { labels: ["--no-summary"], description: t.install_option_no_summary },
  { labels: ["--no-verify"], description: t.install_option_no_verify },
  { labels: ["--ignore-scripts"], description: t.install_option_ignore_scripts },
  { labels: ["--trust"], description: t.install_option_trust },
  { labels: ["-g", "--global"], description: t.install_option_global },
  { labels: ["--backend"], args: [], description: t.install_option_backend },
  { labels: ["--registry"], args: [], description: t.install_option_registry },
  { labels: ["--concurrent-scripts"], args: [], description: t.install_option_concurrent_scripts },
  {
    labels: ["--network-concurrency"],
    args: [],
    description: t.install_option_network_concurrency,
  },
  { labels: ["--save-text-lockfile"], description: t.install_option_save_text_lockfile },
  { labels: ["--omit"], args: [], description: t.install_option_omit },
  { labels: ["--lockfile-only"], description: t.install_option_lockfile_only },
  { labels: ["--linker"], args: [], description: t.install_option_linker },
  {
    labels: ["--minimum-release-age"],
    args: [],
    description: t.install_option_minimum_release_age,
  },
  { labels: ["--cpu"], args: [], description: t.install_option_cpu },
  { labels: ["--os"], args: [], description: t.install_option_os },
  { labels: ["-d", "--dev"], description: t.install_option_dev },
  { labels: ["--optional"], description: t.install_option_optional },
  { labels: ["--peer"], description: t.install_option_peer },
  { labels: ["-E", "--exact"], description: t.install_option_exact },
  { labels: ["-a", "--analyze"], description: t.install_option_analyze },
  { labels: ["--only-missing"], description: t.install_option_only_missing },
];

const updateOptions: OptionNode[] = [
  { labels: ["--latest"], description: t.update_option_latest },
  { labels: ["-i", "--interactive"], description: t.update_option_interactive },
  { labels: ["-r", "--recursive"], description: t.update_option_recursive },
];

const auditOptions: OptionNode[] = [
  { labels: ["--json"], description: t.audit_option_json },
  { labels: ["--audit-level"], args: [], description: t.audit_option_audit_level },
  { labels: ["--ignore"], args: [], description: t.audit_option_ignore },
];

const outdatedOptions: OptionNode[] = [
  { labels: ["-F", "--filter"], args: [], description: t.outdated_option_filter },
  { labels: ["-r", "--recursive"], description: t.outdated_option_recursive },
];

const buildOptions: OptionNode[] = [
  { labels: ["--production"], description: t.build_option_production },
  { labels: ["--compile"], description: t.build_option_compile },
  { labels: ["--bytecode"], description: t.build_option_bytecode },
  { labels: ["--target"], args: [], description: t.build_option_target },
  { labels: ["--outdir"], args: [], description: t.build_option_outdir },
  { labels: ["--outfile"], args: [], description: t.build_option_outfile },
  { labels: ["--metafile"], args: [], description: t.build_option_metafile },
  { labels: ["--sourcemap"], args: [], description: t.build_option_sourcemap },
  { labels: ["--format"], args: [], description: t.build_option_format },
  { labels: ["--splitting"], description: t.build_option_splitting },
  { labels: ["-e", "--external"], args: [], description: t.build_option_external },
  { labels: ["--minify"], description: t.build_option_minify },
  { labels: ["--minify-syntax"], description: t.build_option_minify_syntax },
  { labels: ["--minify-whitespace"], description: t.build_option_minify_whitespace },
  { labels: ["--minify-identifiers"], description: t.build_option_minify_identifiers },
];

const initOptions: OptionNode[] = [
  { labels: ["-y", "--yes"], description: t.init_option_yes },
  { labels: ["-m", "--minimal"], description: t.init_option_minimal },
  { labels: ["-r", "--react"], description: t.init_option_react },
];

const createOptions: OptionNode[] = [
  { labels: ["-h", "--help"], description: t.create_option_help },
];

const patchOptions: OptionNode[] = [
  { labels: ["--commit"], args: [], description: t.patch_option_commit },
  { labels: ["--patches-dir"], args: [], description: t.patch_option_patches_dir },
];

// ---------- pm 子命令 ----------
const pmSubCommands: Record<string, CommandNode> = {
  scan: { description: t.pm_scan_desc, options: [...commonGlobalOpts] },
  pack: {
    description: t.pm_pack_desc,
    options: [
      ...commonGlobalOpts,
      { labels: ["--dry-run"], description: t.install_option_dry_run },
      {
        labels: ["--destination"],
        args: [],
        description: "the directory the tarball will be saved in",
      },
      { labels: ["--filename"], args: [], description: "the name of the tarball" },
      { labels: ["--ignore-scripts"], description: t.install_option_ignore_scripts },
      {
        labels: ["--gzip-level"],
        args: [],
        description: "specify a custom compression level for gzip (0-9, default is 9)",
      },
      { labels: ["--quiet"], description: "only output the tarball filename" },
    ],
  },
  bin: {
    description: t.pm_bin_desc,
    options: [
      ...commonGlobalOpts,
      { labels: ["-g"], description: "print the global path to bin folder" },
    ],
  },
  list: {
    description: t.pm_list_desc,
    options: [
      ...commonGlobalOpts,
      { labels: ["--all"], description: "list the entire dependency tree" },
    ],
  },
  why: {
    description: t.pm_why_desc,
    args: dynamic(async () => getInstalledPackages()),
    options: [...commonGlobalOpts],
  },
  whoami: { description: t.pm_whoami_desc, options: [...commonGlobalOpts] },
  view: { description: t.pm_view_desc, args: [], options: [...commonGlobalOpts] },
  version: { description: t.pm_version_desc, args: [], options: [...commonGlobalOpts] },
  pkg: {
    description: t.pm_pkg_desc,
    args: dynamic(async (ctx) => {
      const sub = ["get", "set", "delete", "fix"];
      return sub.filter((c) => c.startsWith(ctx.prefix));
    }),
    options: [...commonGlobalOpts],
  },
  hash: { description: t.pm_hash_desc, options: [...commonGlobalOpts] },
  "hash-string": { description: t.pm_hash_string_desc, options: [...commonGlobalOpts] },
  "hash-print": { description: t.pm_hash_print_desc, options: [...commonGlobalOpts] },
  cache: {
    description: t.pm_cache_desc,
    commands: {
      rm: { description: t.pm_cache_rm_desc, options: [...commonGlobalOpts] },
    },
  },
  migrate: { description: t.pm_migrate_desc, options: [...commonGlobalOpts] },
  untrusted: { description: t.pm_untrusted_desc, options: [...commonGlobalOpts] },
  trust: {
    description: t.pm_trust_desc,
    args: dynamic(async () => getInstalledPackages()),
    options: [
      ...commonGlobalOpts,
      { labels: ["--all"], description: "trust all untrusted dependencies" },
    ],
  },
  "default-trusted": { description: t.pm_default_trusted_desc, options: [...commonGlobalOpts] },
};

// ---------- 根命令定义 ----------
const bunCommands: Record<string, CommandNode> = {
  run: {
    description: t.cmd_run_desc,
    args: dynamic(async (ctx) => {
      let [scripts, bins, files] = await Promise.all([
        getScriptNames(),
        getBinNames(),
        getScriptFiles(ctx),
      ]);

      // 按用户已输入的前缀过滤
      if (ctx.prefix) {
        scripts = scripts.filter((s) => s.display.startsWith(ctx.prefix));
        bins = bins.filter((b) => b.display.startsWith(ctx.prefix));
      }
      // 有其他选项，只显示文件
      if (scripts.length + bins.length > 0) {
        // 如果没输入不显示文件夹
        if (ctx.prefix === "") {
          files = files.filter((f) => !f.isDir);
        }
        files = files.map((f) => ({
          ...f,
          description: t.suggestion_file,
        }));
      }
      return [...scripts, ...files, ...bins];
    }),
    options: [...commonGlobalOpts],
  },
  test: {
    description: t.cmd_test_desc,
    args: dynamic(async (ctx) => getTestFiles(ctx)),
    options: [...commonGlobalOpts, ...testOptions],
  },
  x: {
    aliases: ["bunx"],
    description: t.cmd_x_desc,
    args: [], // 包名+参数，用户自由输入
    options: [...commonGlobalOpts, ...xOptions],
  },
  repl: {
    description: t.cmd_repl_desc,
    options: [...commonGlobalOpts],
  },
  exec: {
    description: t.cmd_exec_desc,
    args: [],
    options: [...commonGlobalOpts],
  },
  install: {
    aliases: ["i"],
    description: t.cmd_install_desc,
    args: dynamic(async (ctx) => getAddPackages(ctx)),
    options: [...commonGlobalOpts, ...installOptions],
  },
  add: {
    aliases: ["a"],
    description: t.cmd_add_desc,
    args: dynamic(async (ctx) => getAddPackages(ctx)),
    options: [...commonGlobalOpts, ...installOptions],
  },
  remove: {
    aliases: ["rm"],
    description: t.cmd_remove_desc,
    args: dynamic(async (ctx) => {
      // 检测 -g / --global 标志，若启用则补全全局已安装的包
      if (ctx.options["-g"] === true || ctx.options["--global"] === true) {
        return getGlobalPackages();
      }
      return getInstalledPackages();
    }),
    options: [...commonGlobalOpts, ...installOptions],
  },
  update: {
    description: t.cmd_update_desc,
    args: dynamic(async () => getInstalledPackages()),
    options: [...commonGlobalOpts, ...installOptions, ...updateOptions],
  },
  audit: {
    description: t.cmd_audit_desc,
    options: [...commonGlobalOpts, ...auditOptions],
  },
  outdated: {
    description: t.cmd_outdated_desc,
    options: [...commonGlobalOpts, ...outdatedOptions],
  },
  link: {
    description: t.cmd_link_desc,
    args: [],
    options: [...commonGlobalOpts, ...installOptions],
  },
  unlink: {
    description: t.cmd_unlink_desc,
    options: [...commonGlobalOpts, ...installOptions],
  },
  publish: {
    description: t.cmd_publish_desc,
    args: [],
    options: [...commonGlobalOpts, ...installOptions],
  },
  patch: {
    description: t.cmd_patch_desc,
    args: [],
    options: [...commonGlobalOpts, ...installOptions, ...patchOptions],
  },
  pm: {
    description: t.cmd_pm_desc,
    commands: pmSubCommands,
    options: [...commonGlobalOpts],
  },
  info: {
    description: t.cmd_info_desc,
    args: [],
    options: [...commonGlobalOpts, ...installOptions],
  },
  why: {
    description: t.cmd_why_desc,
    args: dynamic(async () => getInstalledPackages()),
    options: [...commonGlobalOpts],
  },
  build: {
    description: t.cmd_build_desc,
    args: dynamic(async (ctx) => getScriptFiles(ctx)),
    options: [...commonGlobalOpts, ...buildOptions],
  },
  init: {
    description: t.cmd_init_desc,
    args: [],
    options: [...commonGlobalOpts, ...initOptions],
  },
  create: {
    aliases: ["c"],
    description: t.cmd_create_desc,
    args: [],
    options: [...commonGlobalOpts, ...createOptions],
  },
  upgrade: {
    description: t.cmd_upgrade_desc,
    options: [...commonGlobalOpts],
  },
  feedback: {
    description: t.cmd_feedback_desc,
    args: [],
    options: [...commonGlobalOpts],
  },
};

export default createCompletion({
  bun: {
    description: t.description,
    options: commonGlobalOpts,
    commands: bunCommands,
  },
});
