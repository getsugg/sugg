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
    const out = await exec("git branch --format=%(refname:short)");
    return out.split(/\r?\n/).map((s) => s.trim()).filter(Boolean);
  } catch {
    return [];
  }
};

export default createCompletion({
  git: {
    description: "分布式版本控制系统",
    commands: {
      // 简单子命令
      init: { description: "初始化新仓库" },
      status: { description: "显示工作区状态" },
      log: { description: "显示提交日志" },

      // git add <files>...：任意多文件（count: Infinity 表达无限）
      add: {
        description: "添加文件到暂存区",
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
        description: "提交变更",
        options: [{ labels: ["-m", "--message"], description: "提交信息", args: [] }],
      },

      // git checkout <branch>：第一位置补 branch 列表
      checkout: {
        description: "切换分支",
        args: dynamic(async () => {
          const branches = await getBranches();
          return branches;
        }),
      },

      // git branch <name>：第一位置补 branch 列表
      branch: {
        description: "列出/创建分支",
        args: dynamic(async () => {
          const branches = await getBranches();
          return branches;
        }),
      },

      // git push <remote> <branch>：ctx.positionals.length 区分位置（严格 count=2）
      push: {
        description: "推送到远端",
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
        description: "拉取远端",
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
        description: "管理远端仓库",
        commands: {
          // git remote -v：列出远端（带 URL），无补全
          list: { description: "列出所有远端" },

          // git remote add <name> <url>（严格 count=2）
          add: {
            description: "添加新远端",
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
            description: "重命名远端",
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
            description: "删除远端",
            args: dynamic(async () => {
              return getRemotes();
            }),
          },
        },
      },
    },
  },
});
