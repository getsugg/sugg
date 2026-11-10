// tests/fixtures/completions/git/index.ts
// Git 补全脚本，演示 ctx.positionals 按位置相关补全的能力
// 关键场景：
//   - git remote add <name> <url>  第一位置补 remote name 列表，第二位置补 url
//   - git push <remote> <branch>   第一位置补 remote name 列表，第二位置补 branch
//   - git pull <remote> <branch>   同上
//   - git remote rename <old> <new>  位置相关
//   - git remote remove <name>      第一位置补 remote name 列表
//   - git checkout <branch>         第一位置补 branch 列表
import { exec } from "sugg";

const getRemotes = async (): Promise<string[]> => {
  try {
    const out = await exec("git remote");
    return out.split(/\r?\n/).map((s) => s.trim()).filter(Boolean);
  } catch {
    return [];
  }
};

const getBranches = async (): Promise<string[]> => {
  try {
    // No shell metacharacters: cmd /C and sh -c must both pass this through.
    const out = await exec("git branch --no-color");
    return out
      .split(/\r?\n/)
      .map((s) => s.replace(/^[*+]\s*/, "").trim())
      .filter(Boolean);
  } catch {
    return [];
  }
};

export default createCompletion({
  git: {
    description: "Distributed version control system",
    commands: {
      // 简单子命令
      init: { description: "Create an empty repository" },
      status: { description: "Show working tree status" },
      log: { description: "Show commit logs" },

      // git add <files>...：任意多文件（count: Infinity 表达无限）
      add: {
        description: "Add files to the index",
        args: {
          count: Infinity,
          items: dynamic(async (ctx) => {
            if (ctx.positionals.length === 0) {
              return ["."];
            }
            return [];
          }),
        },
      },

      // git commit [-m <message>]
      commit: {
        description: "Record changes to the repository",
        options: [{ labels: ["-m", "--message"], description: "Commit message", args: [] }],
      },

      // git checkout <branch>：第一位置补 branch 列表
      checkout: {
        description: "Switch branches",
        args: dynamic(async () => {
          const branches = await getBranches();
          return branches;
        }),
      },

      // git branch <name>：第一位置补 branch 列表
      branch: {
        description: "List or create branches",
        args: dynamic(async () => {
          const branches = await getBranches();
          return branches;
        }),
      },

      // git push <remote> <branch>：ctx.positionals.length 区分位置（严格 count=2）
      push: {
        description: "Update remote refs",
        args: {
          count: 2,
          items: dynamic(async (ctx) => {
            if (ctx.positionals.length === 0) {
              return getRemotes();
            }
            if (ctx.positionals.length === 1) {
              return getBranches();
            }
            return [];
          }),
        },
      },

      // git pull <remote> <branch>：同上（严格 count=2）
      pull: {
        description: "Fetch and integrate with another repository",
        args: {
          count: 2,
          items: dynamic(async (ctx) => {
            if (ctx.positionals.length === 0) {
              return getRemotes();
            }
            if (ctx.positionals.length === 1) {
              return getBranches();
            }
            return [];
          }),
        },
      },

      // git remote 子命令群
      remote: {
        description: "Manage tracked repositories",
        commands: {
          // git remote -v：列出远端（带 URL），无补全
          list: { description: "List all remotes" },

          // git remote add <name> <url>（严格 count=2）
          add: {
            description: "Add a new remote",
            args: {
              count: 2,
              items: dynamic(async (ctx) => {
                if (ctx.positionals.length === 0) {
                  return getRemotes();
                }
                if (ctx.positionals.length === 1) {
                  return [
                    `git@github.com:user/${ctx.positionals[0]}.git`,
                    `https://github.com/user/${ctx.positionals[0]}.git`,
                  ];
                }
                return [];
              }),
            },
          },

          // git remote rename <old> <new>（严格 count=2）
          rename: {
            description: "Rename a remote",
            args: {
              count: 2,
              items: dynamic(async (ctx) => {
                if (ctx.positionals.length === 0) {
                  return getRemotes();
                }
                return [];
              }),
            },
          },

          // git remote remove <name>
          remove: {
            description: "Remove a remote",
            args: dynamic(async () => {
              return getRemotes();
            }),
          },
        },
      },
    },
  },
});
