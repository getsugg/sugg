import * as t from "virtual:i18n/sugg";

const SHELLS = ["bash", "zsh", "fish", "nushell", "powershell"];

const helpOpt: OptionNode = { labels: ["-h", "--help"], description: t.opt_help };

const completionsDirOpt: OptionNode = {
  labels: ["--completions-dir"],
  description: t.opt_completions_dir,
  args: [],
};

const langOpt: OptionNode = { labels: ["--lang"], description: t.opt_lang, args: [] };

const cacheDirOpt: OptionNode = {
  labels: ["--cache-dir"],
  description: t.opt_cache_dir,
  args: [],
};

const dumpDynamicOpt: OptionNode = {
  labels: ["--dump-dynamic"],
  description: t.opt_dump_dynamic,
  args: [],
};

const maxResultsOpt: OptionNode = {
  labels: ["--max-results"],
  description: t.opt_max_results,
  args: [],
};

export default createCompletion({
  sugg: {
    description: t.desc,
    options: [helpOpt, { labels: ["-V", "--version"], description: t.opt_version }],
    commands: {
      complete: {
        description: t.complete_desc,
        options: [cacheDirOpt, maxResultsOpt, helpOpt],
        args: SHELLS,
      },
      commands: {
        description: t.commands_desc,
        options: [helpOpt],
      },
      reload: {
        description: t.reload_desc,
        options: [completionsDirOpt, langOpt, cacheDirOpt, dumpDynamicOpt, helpOpt],
      },
      upgrade: {
        description: t.upgrade_desc,
        options: [helpOpt],
      },
      dev: {
        description: t.dev_desc,
        options: [completionsDirOpt, langOpt, helpOpt],
        commands: {
          init: {
            description: t.dev_init_desc,
            options: [completionsDirOpt, helpOpt],
          },
          i18n: {
            description: t.dev_i18n_desc,
            options: [completionsDirOpt, langOpt, helpOpt],
          },
        },
      },
      init: {
        description: t.init_desc,
        options: [helpOpt],
        args: SHELLS,
      },
      help: {
        description: t.help_desc,
      },
    },
  },
});
