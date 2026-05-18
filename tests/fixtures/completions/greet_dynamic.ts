export default createCompletion({
  greet_dynamic: {
    args: dynamic(() => [
      { display: "hello", description: i18n.greet_dynamic.hello_desc },
      { display: "bye", description: i18n.greet_dynamic.bye_desc },
    ]),
  },
});
