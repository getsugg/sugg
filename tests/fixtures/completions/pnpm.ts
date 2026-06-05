import { a } from "./_lib.ts";
import { readJson } from "sugg";
export default createCompletion({
  pnpm: {
    commands: {
      install: {
        description: "安装依赖",
        options: await (async () => [
          { labels: ["-g", "--global"], description: "全局安装", style: { fg: "green" } },
        ])(),
      },
      run: {
        description: "运行任务或文件",
        args: dynamic(async () => {
          const pkg = await readJson("package.json");
          const scripts = pkg.scripts || {};
          return Object.entries(scripts).map(([name, cmd]) => ({
            display: name,
            description: `运行指令 ${a}: ${cmd}`,
          }));
        }),
      },
    },
  },
});
