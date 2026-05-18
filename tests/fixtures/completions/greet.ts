export default createCompletion({
  greet: {
    description: i18nStr({ en: "Greeting tool", zh: "问候工具" }),
    commands: {
      install: {
        description: i18n.greet.install,
      },
      run: {
        description: i18n["greet"]["run"],
      },
    },
  },
});
