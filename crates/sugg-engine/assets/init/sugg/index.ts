import * as t from "virtual:i18n/sugg";

export default createCompletion({
  sugg: {
    description: t.desc,
    commands: {
      reload: {
        description: t.reload_desc,
        options: [
          { labels: ["--completions-dir"], description: t.opt_completions_dir, args: [] },
          { labels: ["--lang"], description: t.opt_lang, args: [] },
          { labels: ["--cache-dir"], description: t.opt_cache_dir, args: [] },
          { labels: ["--dump-dynamic"], description: t.opt_dump_dynamic, args: [] },
        ],
      },
      upgrade: {
        description: t.upgrade_desc,
      },
      dev: {
        description: t.dev_desc,
        commands: {
          init: {
            description: t.dev_init_desc,
            options: [
              { labels: ["--completions-dir"], description: t.opt_completions_dir, args: [] },
            ],
          },
          i18n: {
            description: t.dev_i18n_desc,
            options: [
              { labels: ["--completions-dir"], description: t.opt_completions_dir, args: [] },
              { labels: ["--lang"], description: t.opt_lang, args: [] },
            ],
          },
        },
      },
    },
  },
});
