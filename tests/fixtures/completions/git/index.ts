export default createCompletion({
  git: {
    commands: {
      commit: { description: "提交变更" },
      push: { description: "推送到远端" },
      pull: { description: "拉取远端" },
    },
  },
});
