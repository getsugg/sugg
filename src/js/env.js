export const createCompletion = (obj) => obj;

export const i18nStr = (obj) => {
  if (obj[__LANG] !== undefined) return obj[__LANG];
  if (obj["en"] !== undefined) return obj["en"];
  const first = Object.values(obj)[0];
  return first !== undefined ? first : "";
};

export const readJson = async (path) => {
  const content = await globalThis.readFile(path);
  if (!content) return {};
  try {
    return JSON.parse(content);
  } catch {
    return {};
  }
};

export const __parseConfig = (modules) => {
  // 排序后回填别名下标：先按字母排（给 Rust 二分查找用），再找真实命令位置填 target
  function sortAndResolveAliases(subcommands) {
    subcommands.sort((a, b) => a.name.localeCompare(b.name));
    for (let sub of subcommands) {
      if (sub.__target_name) {
        let realIdx = subcommands.findIndex(
          (c) => c.name === sub.__target_name && !c.__target_name,
        );
        if (realIdx !== -1) {
          sub.target = realIdx;
        }
        delete sub.__target_name;
      }
    }
  }

  function traverse(name, node, description = "") {
    let cmdNode = {
      name,
      description,
      style: node?.style ?? null,
      target: null,
      subcommands: [],
      options: [],
      dynamic_func: null,
      static_args: null,
    };
    if (!node) return cmdNode;
    if (node.options) {
      for (let opt of node.options) {
        let labels = opt.labels || [];
        let takes_value = false;
        let dynamic_func = null;
        let static_args = null;
        if (opt.args) {
          takes_value = true;
          if (opt.args.__is_dynamic) {
            dynamic_func = opt.args.id;
          } else if (Array.isArray(opt.args)) {
            static_args = opt.args.map((item) => {
              if (typeof item === "string") {
                return { value: item + " ", display: item, description: "", style: null };
              } else {
                return {
                  value: item.value !== undefined ? item.value : item.display + " ",
                  display: item.display,
                  description: item.description || "",
                  style: item.style || null,
                };
              }
            });
          }
        }
        cmdNode.options.push({
          labels,
          description: opt.description || "",
          style: opt.style ?? null,
          takes_value,
          dynamic_func,
          static_args,
        });
      }
    }
    // 支持 args: dynamic(...) 或是静态数组
    if (node.args) {
      if (node.args.__is_dynamic) {
        cmdNode.dynamic_func = node.args.id;
      } else if (Array.isArray(node.args)) {
        cmdNode.static_args = node.args.map((item) => {
          if (typeof item === "string") {
            return { value: item + " ", display: item, description: "", style: null };
          } else {
            return {
              value: item.value !== undefined ? item.value : item.display + " ",
              display: item.display,
              description: item.description || "",
              style: item.style || null,
            };
          }
        });
      }
    }
    if (node.commands) {
      for (let [cmd, def] of Object.entries(node.commands)) {
        cmdNode.subcommands.push(traverse(cmd, def, def.description || ""));

        // 别名作为轻量影子节点推入（不带 options/args/subcommands，只记目标名）
        for (const alias of def.aliases || []) {
          cmdNode.subcommands.push({
            name: alias,
            description: def.description || "",
            style: def.style ?? null,
            __target_name: cmd,
            target: null,
            subcommands: [],
            options: [],
            dynamic_func: null,
            static_args: null,
          });
        }
      }
    }
    sortAndResolveAliases(cmdNode.subcommands);
    return cmdNode;
  }
  let rootNode = {
    name: "",
    description: "",
    style: null,
    target: null,
    subcommands: [],
    options: [],
    dynamic_func: null,
    static_args: null,
  };
  for (let [, def] of modules) {
    if (!def) continue;
    for (let [cmd, subDef] of Object.entries(def)) {
      rootNode.subcommands.push(traverse(cmd, subDef, subDef.description || ""));
    }
  }
  sortAndResolveAliases(rootNode.subcommands);
  return { root: rootNode };
};
