// 提取共用的包管理参数选项 (install, add, remove, link, unlink 均会使用)
const packageOptions = [
  { labels: ["-c", "--config"], description: "Load config(bunfig.toml)" },
  { labels: ["-y", "--yarn"], description: "Write a yarn.lock file (yarn v1)" },
  { labels: ["-p", "--production"], description: "Don't install devDependencies" },
  { labels: ["--no-save"], description: "Don't save a lockfile" },
  { labels: ["--save"], description: "Save to package.json" },
  { labels: ["--dry-run"], description: "Don't install anything" },
  { labels: ["--frozen-lockfile"], description: "Disallow changes to lockfile" },
  {
    labels: ["-f", "--force"],
    description: "Always request the latest versions from the registry",
  },
  {
    labels: ["--cache-dir"],
    description: "Store & load cached data from a specific directory path",
  },
  { labels: ["--no-cache"], description: "Ignore manifest cache entirely" },
  { labels: ["--silent"], description: "Don't log anything" },
  { labels: ["--verbose"], description: "Excessively verbose logging" },
  { labels: ["--no-progress"], description: "Disable the progress bar" },
  { labels: ["--no-summary"], description: "Don't print a summary" },
  { labels: ["--no-verify"], description: "Skip verifying integrity of newly downloaded packages" },
  { labels: ["--ignore-scripts"], description: "Skip lifecycle scripts in the package.json" },
  { labels: ["-g", "--global"], description: "Add a package globally" },
  { labels: ["--cwd"], description: "Set a specific cwd" },
  { labels: ["--backend"], description: "Platform-specific optimizations" },
  { labels: ["--help"], description: "Print this help menu" },
];

// 定义共用的 add / install 命令结构
const addCommand = {
  description: "Add a dependency to package.json (bun a)",
  options: [
    ...packageOptions,
    { labels: ["-d", "--dev"], description: "Add dependence to devDependencies" },
    { labels: ["-D"], description: "Add dependence to devDependencies" },
    { labels: ["--optional"], description: "Add dependency to optionalDependencies" },
    { labels: ["--peer"], description: "Add dependency to peerDependencies" },
    { labels: ["--exact"], description: "Add the exact version instead of the ^range" },
  ],
  // 动态获取推荐的 npm 包（这里调用原生 bun 自带的补全推荐）
  args: dynamic(async (ctx) => {
    const { prefix } = ctx;

    const pkgs = await exec(`bun getcompletes a ${prefix}`);
    return pkgs
      .split("\n")
      .filter(Boolean)
      .map((line) => ({
        display: line,
        description: "NPM Package",
      }));
  }),
};

const installCommand = {
  description: "Install dependencies for a package.json (bun i)",
  options: packageOptions,
};

const removeCommand: CommandNode = {
  description: "Remove a dependency from package.json (bun rm)",
  options: packageOptions,
  // 动态读取当前项目的 package.json 里的依赖项进行移除提示
  args: dynamic(async () => {
    const pkg = await readJson("package.json");
    const deps = Object.keys(pkg.dependencies || {});
    const devDeps = Object.keys(pkg.devDependencies || {});
    return [...deps, ...devDeps].map((d) => ({
      display: d,
      description: "Installed dependency",
    }));
  }),
};

// 导出主补全配置
export default createCompletion({
  bun: {
    options: [
      { labels: ["-V", "--version"], description: "Show version and exit" },
      { labels: ["--cwd"], description: "Change directory" },
      { labels: ["-h", "--help"], description: "Show command help" },
      { labels: ["--use"], description: 'Use a framework, e.g. "next"' },
    ],
    commands: {
      run: {
        description: "Run JavaScript with Bun, a package.json script, or a bin",
        options: [
          { labels: ["-h", "--help"], description: "Display this help and exit" },
          {
            labels: ["-b", "--bun"],
            description: "Force a script or package to use Bun's runtime",
          },
          {
            labels: ["--watch"],
            description: "Automatically restart bun's JavaScript runtime on file change",
          },
          { labels: ["--hot"], description: "Enable auto reload in bun's JavaScript runtime" },
          { labels: ["--inspect"], description: "Activate Bun's Debugger" },
        ],
        // 动态返回 npm scripts 或者当前目录的可执行 js/ts 文件
        args: dynamic(async (ctx) => {
          const { prefix } = ctx;

            // ── 路径补全：scanPath 自动解析用户输入 ──
          const entries = await scanPath(prefix);
          const pathItems = entries
            .filter((e) => e.display.startsWith(prefix))
            .map((e) => ({
              ...e,
              description: e.isDir ? "directory" : "file",
            }));

          // ── npm scripts（不设 value，系统自动加尾随空格）──
          const pkg = await readJson("package.json");
          const scripts = Object.keys(pkg.scripts || {}).map((s) => ({
            display: s,
            description: `npm script: ${pkg.scripts[s]}`,
          }));

          // ── global bins（不设 value，系统自动加尾随空格）──
          const binsStr = await exec("bun getcompletes b");
          const bins = binsStr
            .split("\n")
            .filter(Boolean)
            .map((b) => ({ display: b, description: "bin" }));

          // 合并去重
          const seen = new Set();
          return [...scripts, ...bins, ...pathItems].filter((item) => {
            if (!item || seen.has(item.display)) return false;
            seen.add(item.display);
            return true;
          });
        }),
      },
      test: {
        description: "Run unit tests with Bun",
        options: [
          { labels: ["--watch"], description: "Automatically restart on file change" },
          { labels: ["--timeout"], description: "Set the per-test timeout in milliseconds" },
          { labels: ["--coverage"], description: "Generate a coverage profile" },
          { labels: ["--bail"], description: "Exit the test suite after failures" },
        ],
        // 动态扫描匹配测试规范的文件名
        args: dynamic(async () => {
          const entries = await scanPath(".");
          return entries
            .filter((e) => !e.isDir && e.display.match(/(_|\.)(test|spec)\.(js|ts|jsx|tsx)$/))
            .map((e) => ({ ...e, description: "test file" }));
        }),
      },
      init: {
        description: "Start an empty Bun project from a blank template",
        options: [{ labels: ["-y", "--yes"], description: "Answer yes to all prompts" }],
      },
      create: {
        description: "Create a new project from a template (bun c)",
        commands: {
          "next-app": { description: "Next.js app" },
          "react-app": { description: "React app" },
        },
      },
      pm: {
        description: "More commands for managing packages",
        commands: {
          bin: {
            description: "print the path to bin folder",
            options: [{ labels: ["-g"], description: "print the global path to bin folder" }],
          },
          ls: {
            description: "list the dependency tree according to the current lockfile",
            options: [{ labels: ["--all"], description: "list the entire dependency tree" }],
          },
          hash: { description: "generate & print the hash of the current lockfile" },
          "hash-string": { description: "print the string used to hash the lockfile" },
          cache: {
            description: "print the path to the cache folder",
            commands: { rm: { description: "remove cache" } },
          },
          version: {
            description: "bump the version in package.json and create a git tag",
            commands: {
              patch: { description: "increment patch version" },
              minor: { description: "increment minor version" },
              major: { description: "increment major version" },
            },
          },
        },
      },
      build: {
        description: "Bundle TypeScript & JavaScript into a single file",
        options: [
          { labels: ["--outfile"], description: "Write the output to a specific file" },
          { labels: ["--outdir"], description: "Write the output to a directory" },
          { labels: ["--minify"], description: "Enable all minification flags" },
          { labels: ["--sourcemap"], description: "Generate sourcemaps" },
          { labels: ["--target"], description: "The intended execution environment" },
        ],
        args: dynamic(async () => {
          const entries = await scanPath(".");
          return entries
            .filter((e) => !e.isDir && e.display.match(/\.(ts|js|tsx|jsx)$/))
            .map((e) => ({ ...e, description: "source file" }));
        }),
      },
      x: {
        description: "Install and execute a package bin (bunx)",
        // 读取本地已经安装的 bin 工具
        args: dynamic(async () => {
          // 虚拟根目录模式：扫描 node_modules/.bin，前缀不暴露 .bin 路径
          const entries = await scanPath("", "node_modules/.bin");
          return entries
            .filter((e) => !e.isDir)
            .map((e) => ({ ...e, description: "local bin" }));
        }),
      },
      update: {
        description: "Update outdated dependencies & save to package.json",
        options: [
          ...packageOptions,
          { labels: ["--latest"], description: "Updates dependencies to latest version" },
        ],
      },
      link: { description: "Link an npm package globally", options: packageOptions },
      unlink: { description: "Globally unlink an npm package", options: packageOptions },
      outdated: { description: "Display the latest versions of outdated dependencies" },
      upgrade: {
        description: "Get the latest version of bun",
        options: [{ labels: ["--canary"], description: "Upgrade to canary build" }],
      },
      repl: { description: "Start a REPL session with Bun" },

      // 注册常见别名映射
      install: { ...installCommand, aliases: ["i"] },
      a: addCommand,
      add: addCommand,
      rm: removeCommand,
      remove: removeCommand,
      c: { description: "Alias for create" }, // 可以直接简写
    },
  },
});
