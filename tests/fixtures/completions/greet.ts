import * as greet from "virtual:i18n/greet";

export default createCompletion({
  greet: {
    description: greet.description,
    commands: {
      install: {
        description: greet.install,
      },
      run: {
        description: greet.run,
      },
    },
  },
});
