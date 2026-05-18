export default createCompletion({
  docker: {
    commands: {
      run: { description: i18n.docker.run_desc },
      ps: {
        description: i18n.docker.ps_desc,
        args: dynamic(async () => {
          const out = await exec("docker ps --format '{{.Names}}'");
          return out
            .split("\n")
            .filter(Boolean)
            .map((name) => ({
              display: name,
              description: i18n.docker.container_desc,
            }));
        }),
      },
      stop: { description: i18n.docker.stop_desc },
    },
  },
});
