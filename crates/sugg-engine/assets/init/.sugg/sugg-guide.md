# Sugg Completion Script Guide

Sugg completion scripts are written in TypeScript, placed in the `completions/` directory. The engine is built on QuickJS + Rolldown.

For full type signatures (`CommandNode`, `OptionNode`, `Suggestion`, `DynamicCommand`, etc.), read `sugg.d.ts` directly.

---

## Core Rules

1. Every script must export `createCompletion({ commandName: { ... } })`. The top-level key is the command name (e.g. `git.ts` → `{ git: { ... } }`).
2. Use TypeScript freely — extract constants, helper functions, and reusable `dynamic` callbacks.
3. Prefer `Promise.all` over sequential `await` for independent async operations.

---

## ESM Module API

```ts
import { exec, execFile, scanPath, readFile, readJson, ui, cache, fetch } from "sugg";
// createCompletion and dynamic are built-in (no import needed)
```

All signatures and pitfalls are in `sugg.d.ts`; this guide only covers usage patterns.

---

## `dynamic` Callback

`dynamic(...)` marks a callback as a dynamic completion:

```ts
// Sync
args: dynamic((ctx) => ["a", "b", "c"])

// Async
args: dynamic(async (ctx) => (await fetch(url)).json())

// Extracted helper
async function getScripts(): Promise<Suggestion[]> {
  const pkg = await readJson("package.json");
  return Object.keys(pkg.scripts || {}).map(s => ({ display: s }));
}

args: dynamic(getScripts)
```

---

## Cache

`cache.get` memoizes expensive calls (e.g. `exec`, file scans) per context:

```ts
args: dynamic(async (ctx) => {
  // ctx-keyed: "bun run b" and "bun run bu" share one entry
  const [scripts, bins] = await cache.get(ctx, 5000, () =>
    Promise.all([getScriptNames(), getBinNames()])
  );
  return [...scripts, ...bins];
})
```

Without a fetcher it's read-only and returns `undefined` on miss. Keys also accept `string` or `string[]` — see `sugg.d.ts`.

---

## i18n

Translation files go in `i18n/` inside the command folder:

```
completions/
  git/
    i18n/
      en.json
      zh.json
    index.ts
```

JSON values are flat key-value pairs. **Keys must not contain `.`** — use underscores or camelCase.

Import the virtual module in your script:

```ts
import * as t from 'virtual:i18n/git';

export default createCompletion({
  git: { description: t.git_desc, commands: { push: { description: t.push_desc } } }
});
```

- Language resolution order: `--lang` CLI flag → system locale (via `sys-locale`) → `en`.
- Fallback chain follows ICU4X BCP47 (e.g. `zh-Hans-CN` → `[en, zh, zh-CN]`); `en` is always last-resort.
- Run `sugg dev i18n` to generate type declarations for `virtual:i18n/*`.

---

## Return Value Rules

- Return `string[]` or `Suggestion[]`.
- The engine filters by `ctx.prefix` automatically — **do not filter manually**.
- A trailing space is appended to `display` by default. To suppress it, provide an explicit `value`.
- Options that take a value but have no suggestions: use `args: []` — the engine will wait for free input.
- To express "no suggestion for this position", return `[]`. The TS type does not allow `null | undefined`; doing so is a type error. The runtime does tolerate `null`/`undefined` silently (treated as `[]`) as a defensive measure, but the contract is `[]`.

---

## `args` Forms

Both `OptionNode` and `CommandNode` accept the same `args` forms, unified under an `args_count` model:

| Form | args_count | Use case |
|---|---|---|
| _omitted_ | `0` | Bool flag, or command with no positionals |
| `args: []` | `1` | Needs a value, no suggestions |
| `args: ["dev", "build"]` | `1` | Static suggestions (single value) |
| `args: dynamic(...)` | `1` | Dynamic suggestions (single value) |
| `args: { items: ["a", "b"] }` | `1` | Explicit single-value with static items |
| `args: { count: 3, items: [...] }` | `3` | Multi-value option (e.g. `--exclude a b c`) |
| `args: { count: Infinity, items: [...] }` | unlimited | `npm install pkg1 pkg2 ...`, `git add f1 f2 ...` |
| `args: { count: 0 }` | `0` | Explicit "no positional args" |

```ts
// Single value (default)
--mode: { items: ["dev", "build"] }

// Multi-value (total capacity 3)
--exclude: { count: 3, items: ["a", "b", "c"] }

// Unlimited positional args
install: { count: Infinity, items: dynamic(async ctx => {
  if (ctx.positionals.length === 0) return ["react", "vue", "svelte"];
  return []; // subsequent positions: no per-position suggestion
}) }

// Command with 0 positional args
noPositional: { count: 0, commands: { only: { ... } } }
```

**Strict by default**: omitting `count: Infinity` means the state machine consumes exactly the declared `count` (default 1). For `git remote add <name> <url>` style multi-position completion, declare `count: 2` (or `Infinity` if unbounded) explicitly. Use a positive integer or `Infinity` for `count` — `NaN` / negative numbers collapse the entire root to empty at bundle time (fail-loud).

---

## Context-Aware Completions

Merge candidates from multiple sources intelligently:

```ts
args: dynamic(async (ctx) => {
  // Scripts and bins share one cache entry — same context key, one Promise.all
  let [[scripts, bins], files] = await Promise.all([
    cache.get(ctx, 5000, () => Promise.all([getScriptNames(), getBinNames()])),
    getScriptFiles(ctx), // needs live prefix, cannot be cached
  ]);

  const hasSemanticMatches = scripts.length + bins.length > 0;
  if (hasSemanticMatches) {
    // Hide directories when there are semantic matches and no prefix typed yet
    if (ctx.prefix === "") files = files.filter(f => !f.isDir);
    // Label remaining files so users can distinguish them from scripts
    files = files.map(f => ({ ...f, description: "file" }));
  }

  // Order implies priority: scripts > files > bins
  return [...scripts, ...files, ...bins];
})
```

Switch by flags:

```ts
args: dynamic(async (ctx) => {
  if (ctx.options["-g"] || ctx.options["--global"]) return getGlobalPackages();
  return getInstalledPackages();
})
```

---

## Per-Position Completions with `ctx.positionals`

`ctx.positionals` is the dedicated channel for "what was typed before the current word" on the current node. The in-progress `ctx.prefix` is **not** included — only submitted words. `positionals.length` is the count of submitted positionals; `positionals[N - 1]` reads the previous one.

**`count` is required** for multi-position patterns. Without `count: 2`, the state machine consumes only 1 positional and `ctx.positionals.length` never reaches 1:

```ts
add: {
  args: {
    count: 2,  // explicit: 2 positionals (name, url)
    items: dynamic(async (ctx) => {
      if (ctx.positionals.length === 0) return getRemotes();           // 1st: names
      if (ctx.positionals.length === 1) return getUrls(ctx.positionals[0]); // 2nd: depends on 1st
      return [];                                                      // 3rd+: stop
    })
  }
}
```

Build the next suggestion from the prior value:

```ts
args: {
  count: 2,
  items: dynamic(async ctx => {
    if (ctx.positionals.length === 0) return ["origin", "upstream", "mine"];
    if (ctx.positionals.length === 1) {
      const name = ctx.positionals[0];
      return [`git@github.com:me/${name}.git`, `https://github.com/me/${name}.git`];
    }
    return [];
  })
}
```

Unlimited positionals (`git add f1 f2 ...`, `npm install react vue lodash`). The dynamic returns suggestions for the first few positions and `[]` for the rest — listing every file at once isn't useful:

```ts
add: {
  args: {
    count: Infinity,  // state machine never "closes" this node
    items: dynamic(async ctx => {
      if (ctx.positionals.length === 0) return ["."];  // 1st: hint at "add all"
      return [];                                         // 2nd+: user types filenames directly
    })
  }
}
```

### Choosing the right context field

| Need | Use |
|---|---|
| Flag/switch value | `ctx.options["--my-opt"]` |
| Previous positional as seed | `ctx.positionals[N - 1]` |
| Count of submitted positionals | `ctx.positionals.length` |
| Full command line for parsing | `ctx.words` |

### Common pitfalls

- **`ctx.args` does not exist** — the field is `ctx.positionals` (renamed to avoid clashing with `arguments`). `ctx.args[0]` silently returns `undefined` and the dynamic throws on `.length`.
- **In-progress word stays in `ctx.prefix`**: if the user typed `git remote add or` (no trailing space), `ctx.positionals` is `[]` and `ctx.prefix` is `"or"`. Return the first-position list anyway — the shell client filters by `ctx.prefix` downstream.

---

## Common Mistakes

- ❌ Missing the top-level `{ commandName: ... }` wrapper
- ❌ Manually filtering by `ctx.prefix` inside `dynamic`
- ❌ Sequential `await` for independent async calls (use `Promise.all`)
- ❌ Manually splitting `scanPath` input (the engine handles directory boundaries)
- ❌ Omitting `args` on options that take a value (write `args: []` even with no suggestions)
- ❌ Using placeholders like `<file>` in `args` (use `args: []` instead)
- ❌ i18n keys containing `.`
- ❌ Unnecessary variable extraction of single-use small structures — inline ~20 lines or fewer

---

## Full Example

```ts
import { readJson } from "sugg";

const commonOpts: OptionNode[] = [
  { labels: ["-h", "--help"] },
  { labels: ["-v", "--version"] },
];

export default createCompletion({
  pnpm: {
    description: "Fast, disk space efficient package manager",
    options: commonOpts,
    commands: {
      run: {
        description: "Run a package script",
        args: dynamic(async () => {
          const pkg = await readJson("package.json");
          return Object.keys(pkg.scripts || {});
        }),
      },
      install: {
        aliases: ["i"],
        description: "Install all dependencies",
        options: [...commonOpts, { labels: ["-D"], description: "Save as devDependency" }],
      },
    },
  },
});
```


---

## When to Use `collect-cli-help.md`

Before writing completions for an unfamiliar CLI tool, collect its full help output first. This gives you the exact subcommand names, option labels, and argument descriptions to work from — no guessing.

See `.sugg/collect-cli-help.md` for the step-by-step workflow and ready-to-run shell scripts.
