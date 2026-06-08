export const createCompletion = (obj) => obj;

export const readJson = async (path) => {
  const content = await globalThis.__readFile(path);
  if (!content) return {};
  try {
    return JSON.parse(content);
  } catch {
    return {};
  }
};

export const exec = globalThis.__exec;
export const execFile = globalThis.__execFile;
export const scanPath = globalThis.__scanPath;
export const readFile = globalThis.__readFile;
export const ui = globalThis.__ui;

export const fetch = async (url, options = {}) => {
  const timeout = typeof options.timeout === 'number' ? options.timeout : 2000;
  const rawResponseStr = await globalThis.__fetch_raw({
    url: url,
    method: options.method || "GET",
    headers: options.headers || {},
    body: options.body || "",
    timeout: timeout
  });
  if (!rawResponseStr) {
    throw new Error(`Request timed out after ${timeout}ms or failed.`);
  }
  const raw = JSON.parse(rawResponseStr);
  return {
    ok: raw.status >= 200 && raw.status < 300,
    status: raw.status,
    statusText: raw.statusText,
    headers: { get: (name) => raw.headers[name.toLowerCase()] || null },
    text: async () => raw.body,
    json: async () => JSON.parse(raw.body),
  };
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

  // 把用户写在 args 上的"项"转换为 StaticSuggestion 数组
  function toStaticArgs(items) {
    if (!Array.isArray(items)) return null;
    return items.map((item) => {
      if (typeof item === "string") {
        return { value: item + " ", display: item, description: "", style: null };
      }
      return {
        value: item.value !== undefined ? item.value : item.display + " ",
        display: item.display,
        description: item.description || "",
        style: item.style || null,
      };
    });
  }

  // 解析节点的 args 字段：返回 { args_count, dynamic_func, static_args }
  // 支持四种形式：
  //   undefined                       → count=0（不接：command 不接位置参数 / option bool）
  //   string[]                        → count=1（默认向后兼容）
  //   DynamicObj                      → count=1（默认向后兼容）
  //   { count, items? }               → count=count（显式多值；count=Infinity → 无限）
  // 无限值用 JS 字面量 Infinity，bundler 映射为 u32::MAX；省略时严格按 count 消耗（默认 1），不隐式无限
  //
  // Saturate 规则：只保护 Infinity 和 > u32::MAX 的正有限数 → 0xFFFFFFFF
  // 不保护 NaN / -Infinity / 负数：JSON.stringify 会把 NaN / -Infinity 变 "null"，
  // 负数变 "-5"，serde 解析 u32 字段会 Err → unwrap_or_default() 让整个 root 变空。
  // 这是 fail-loud：用户手贱写 NaN / 负数，崩 root 是用户的责任，引擎不替用户擦屁股。
  const UNLIMITED = 0xFFFFFFFF;  // u32::MAX，CLI step 4 看到这个值不检查 remaining
  function resolveArgs(args) {
    if (args == null) {
      return { args_count: 0, dynamic_func: null, static_args: null };
    }
    if (Array.isArray(args)) {
      return { args_count: 1, dynamic_func: null, static_args: toStaticArgs(args) };
    }
    if (args.__is_dynamic) {
      return { args_count: 1, dynamic_func: args.id, static_args: null };
    }
    if (typeof args === "object" && args.count !== undefined) {
      const items = args.items;
      // Saturate：Infinity 和 > 0xFFFFFFFF 的正有限数 → UNLIMITED
      // 实测 QuickJS 比较：
      //   Infinity    > 0xFFFFFFFF → true   (saturate)
      //   0xFFFFFFFF > 0xFFFFFFFF → false  (合法值，0xFFFFFFFF == u32::MAX)
      //   NaN         > 0xFFFFFFFF → false  (NaN 比较永远 false，fail-loud)
      //   -Infinity   > 0xFFFFFFFF → false  (fail-loud)
      //   -5          > 0xFFFFFFFF → false  (fail-loud)
      const count = args.count > UNLIMITED ? UNLIMITED : args.count;
      if (items == null) {
        return { args_count: count, dynamic_func: null, static_args: null };
      }
      if (Array.isArray(items)) {
        return { args_count: count, dynamic_func: null, static_args: toStaticArgs(items) };
      }
      if (items.__is_dynamic) {
        return { args_count: count, dynamic_func: items.id, static_args: null };
      }
    }
    return { args_count: 0, dynamic_func: null, static_args: null };
  }

  function traverse(name, node, description = "") {
    let cmdNode = {
      name,
      description,
      style: node?.style ?? null,
      target: null,
      subcommands: [],
      options: [],
      args_count: 0,
      dynamic_func: null,
      static_args: null,
    };
    if (!node) return cmdNode;
    if (node.options) {
      for (let opt of node.options) {
        let labels = opt.labels || [];
        const resolved = resolveArgs(opt.args);
        cmdNode.options.push({
          labels,
          description: opt.description || "",
          style: opt.style ?? null,
          args_count: resolved.args_count,
          dynamic_func: resolved.dynamic_func,
          static_args: resolved.static_args,
        });
      }
    }
    // command 节点本身的 args
    {
      const resolved = resolveArgs(node.args);
      cmdNode.args_count = resolved.args_count;
      cmdNode.dynamic_func = resolved.dynamic_func;
      cmdNode.static_args = resolved.static_args;
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
            args_count: 0,
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
    args_count: 0,
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

const resolveKey = (key) => {
  let result;
  if (Array.isArray(key)) result = key.join("\0");
  else if (key && typeof key === "object") {
    if (key.words != null && key.path != null)
      result = [...key.words].slice(0, -1).concat(key.path).join("\0");
    else if (typeof key.join === "function") result = key.join("\0");
    else result = String(key);
  }
  else result = key;
  return __SCRIPT_STEM ? __SCRIPT_STEM + "\0" + result : result;
};

export const cache = {
  get: async (key, ttlMs, fetcher) => {
    const k = resolveKey(key);
    const cached = globalThis.__cache.get(k);
    if (cached !== undefined && cached !== "") {
      try { return JSON.parse(cached); } catch { return cached; }
    }
    if (!fetcher) return undefined;
    const fresh = await fetcher();
    if (fresh !== undefined && fresh !== null)
      globalThis.__cache.set(k, typeof fresh === "string" ? fresh : JSON.stringify(fresh), ttlMs);
    return fresh;
  },
  delete: (key) => globalThis.__cache.delete(resolveKey(key)),
};