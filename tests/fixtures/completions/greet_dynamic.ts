import * as greet_dynamic from "virtual:i18n/greet_dynamic";

export default createCompletion({
  greet_dynamic: {
    args: dynamic(() => [
      { display: "hello", description: greet_dynamic.hello_desc },
      { display: "bye", description: greet_dynamic.bye_desc },
    ]),
  },
});
