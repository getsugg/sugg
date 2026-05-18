// tests/fixtures/completions/testkit.ts
// 综合性测试脚本，覆盖选项值补全、options 传递、dynamic 复用、混合节点、display/value 分离等特性

// 外部 dynamic 变量，用于测试复用
const sharedDynamic = dynamic(async () => {
  return ["shared-a", "shared-b", "shared-c"];
});

export default createCompletion({
  testkit: {
    description: "All-in-one test command",
    options: [
      { labels: ["-h", "--help"], description: "Show help" },
    ],
    commands: {
      // ── 场景1：选项值补全（静态数组 + 动态）──
      opts: {
        description: "Test option value completion",
        options: [
          {
            labels: ["--mode"],
            description: "Select mode",
            args: ["dev", "build", "lint"],
          },
          {
            labels: ["--log-level"],
            description: "Log level",
            args: ["error", "warn", "info", "debug"],
          },
          {
            labels: ["-d", "--dynamic-opt"],
            description: "Dynamic option",
            args: dynamic(async () => {
              return ["item1", "item2", "item3"];
            }),
          },
          {
            labels: ["--name"],
            description: "Enter name (no suggestions)",
            args: [],   // 只标记需要值，不提供补全
          },
          // 两个选项引用同一个外部 dynamic，测试复用
          {
            labels: ["--shared1"],
            description: "Shared dynamic 1",
            args: sharedDynamic,
          },
          {
            labels: ["-s2", "--shared2"],
            description: "Shared dynamic 2",
            args: sharedDynamic,
          },
        ],
      },

      // ── 场景2：ctx.options 传递 ──
      optsinfo: {
        description: "Test ctx.options passed to dynamic",
        options: [
          { labels: ["-g", "--global"] },
          { labels: ["-c", "--config"], args: [] },
          { labels: ["--cwd"], args: [] },
          { labels: ["--exclude"], args: [] },
        ],
        args: dynamic(async (ctx) => {
          const lines: string[] = [];
          const opts = ctx.options;
          if (opts["-g"]) {
            lines.push("global:true");
          }
          if (opts["--config"]) {
            lines.push(`config:${(opts["--config"] as string[]).join(",")}`);
          }
          if (opts["--cwd"]) {
            lines.push(`cwd:${(opts["--cwd"] as string[]).join(",")}`);
          }
          if (opts["--exclude"]) {
            lines.push(`exclude:${(opts["--exclude"] as string[]).join(",")}`);
          }
          if (lines.length === 0) {
            lines.push("(no options)");
          }
          return lines.map((s) => ({ display: s, value: s }));
        }),
      },

      // ── 场景3：dynamic 复用（已在 opts 中通过 sharedDynamic 覆盖）──
      // 额外再加一个独立命令，用同一个 sharedDynamic 作为子命令的 args
      dynamicReuse: {
        description: "Test reusing dynamic functions (subcommand args)",
        commands: {
          cmd1: {
            description: "Subcommand 1",
            args: sharedDynamic,
          },
          cmd2: {
            description: "Subcommand 2",
            args: sharedDynamic,
          },
        },
      },

      // ── 场景4：节点同时有子命令和参数 ──
      hybrid: {
        description: "Has both subcommands and args",
        args: dynamic(async () => {
          return ["file1.txt", "file2.txt", "file3.txt"];
        }),
        commands: {
          push: { description: "Push operation" },
          pop: { description: "Pop operation" },
          list: { description: "List operation" },
        },
      },

      // ── 场景5：display 与 value 分离 ──
      displayValue: {
        description: "Test display/value separation",
        args: [
          { display: "Add", value: "add", description: "Add an item" },
          { display: "Remove", value: "rm", description: "Remove an item" },
        ],
      },

      // ── 场景6：空 args 标记（选项需要值但不提供补全） ──
      emptyArgs: {
        description: "Command with an option that takes a value but no suggestions",
        options: [
          { labels: ["--token"], args: [] },
        ],
      },
    },
  },
});
