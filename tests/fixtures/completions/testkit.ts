// tests/fixtures/completions/testkit.ts
// 综合性测试脚本，覆盖选项值补全、options 传递、dynamic 复用、混合节点、display/value 分离等特性
import { ui } from "sugg";

// 外部 dynamic 变量，用于测试复用
const sharedDynamic = dynamic(async () => {
  return ["shared-a", "shared-b", "shared-c"];
});

export default createCompletion({
  testkit: {
    description: "All-in-one test command",
    options: [{ labels: ["-h", "--help"], description: "Show help" }],
    commands: {
      // 选项值补全（静态数组 + 动态）
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
            args: [], // 只标记需要值，不提供补全
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

      // ctx.options 传递
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

      // dynamic 复用（已在 opts 中通过 sharedDynamic 覆盖）
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

      // 节点同时有子命令和参数
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

      // display 与 value 分离
      displayValue: {
        description: "Test display/value separation",
        args: [
          { display: "Add", value: "add", description: "Add an item" },
          { display: "Remove", value: "rm", description: "Remove an item" },
        ],
      },

      // 空 args 标记（选项需要值但不提供补全）
      emptyArgs: {
        description: "Command with an option that takes a value but no suggestions",
        options: [{ labels: ["--token"], args: [] }],
      },

      // log 与 throw 日志演示
      logdemo: {
        description: "Demonstrate log and throw output in the UI",
        args: dynamic(async (ctx) => {
          const prefix = ctx.prefix ?? "";

          // throw: 故意抛出一个可读性强的错误，让补全菜单顶部展示红色 ERR
          if (prefix.startsWith("bad-")) {
            throw new Error(
              `❌ Invalid prefix detected: "${prefix}" — please remove the leading "bad-"`,
            );
          } else {
            // log: 永远不会抛异常，输出追踪信息
            ui.log("[DEBUG] prefix=", prefix);
            ui.log("[INFO] configuration loaded successfully");
          }

          return [
            { display: "alpha", value: "alpha", description: "First item" },
            { display: "beta", value: "beta", description: "Second item" },
            { display: "gamma", value: "gamma", description: "Third item" },
          ];
        }),
      },

      // 多值位置参数节点：args_count=3
      positionalCount: {
        description: "Command that consumes 3 positional tokens",
        args: { count: 3, items: ["x", "y", "z"] },
      },

      // 多值选项节点：选项 args_count=3
      multiValueOpt: {
        description: "Command with multi-value option (--exclude accepts 3 values)",
        options: [
          { labels: ["--exclude"], args: { count: 3, items: dynamic(() => ["a", "b", "c"]) } },
          { labels: ["--include"] }, // bool 对比
        ],
      },

      // 不接位置参数的命令节点：count=0
      noPositional: {
        description: "Command that accepts 0 positional tokens",
        args: { count: 0 },
        commands: {
          only: { description: "Only subcommand" },
        },
      },

      // 位置相关补全：ctx.positionals 按消耗顺序累计
      // 显式 count: Infinity 表达"无限位置参数"（内部映射为 u32::MAX）；
      // dynamic 函数根据 ctx.positionals.length 判断当前位置，返回相应补全
      dynamicPositional: {
        description: "Test ctx.positionals provides prior positional values",
        args: {
          count: Infinity,
          items: dynamic(async (ctx) => {
            if (ctx.positionals.length === 0) {
              return ["origin", "upstream", "mine"];
            }
            if (ctx.positionals.length === 1) {
              return [`url-for:${ctx.positionals[0]}`, "https://github.com/x"];
            }
            return [];
          }),
        },
      },

      // 无限位置参数 + 静态项（演示 count: Infinity + items: [...]）
      staticUnlimited: {
        description: "Static unlimited positional args",
        args: { count: Infinity, items: ["alpha", "beta", "gamma"] },
      },
    },
  },
});
