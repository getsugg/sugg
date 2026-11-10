import { a } from "./_lib.ts";
import { readJson } from "sugg";
export default createCompletion({
  pnpm: {
    commands: {
      install: {
        description: "Install dependencies",
        options: await (async () => [
          { labels: ["-g", "--global"], description: "Install globally", style: { fg: "green" } },
        ])(),
      },
      run: {
        description: "Run a task or file",
        args: dynamic(async () => {
          const pkg = await readJson("package.json");
          const scripts = pkg.scripts || {};
          return Object.entries(scripts).map(([name, cmd]) => ({
            display: name,
            description: `Run script ${a}: ${cmd}`,
          }));
        }),
      },
    },
  },
});
