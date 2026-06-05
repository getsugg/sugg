import * as docker from "virtual:i18n/docker";
import { exec } from "sugg";

export default createCompletion({
  docker: {
    commands: {
      run: { description: docker.run_desc },
      ps: {
        description: docker.ps_desc,
        args: dynamic(async () => {
          const out = await exec("docker ps --format '{{.Names}}'");
          return out
            .split("\n")
            .filter(Boolean)
            .map((name) => ({
              display: name,
              description: docker.container_desc,
            }));
        }),
      },
      stop: { description: docker.stop_desc },
    },
  },
});
