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

| Import | Purpose |
|---|---|
| `scanPath(input, baseDir?)` | Smart path/file scanner, auto-resolves directory prefix |
| `exec(cmd)` | Run a shell command (via shell), returns stdout string |
| `execFile(cmd, args)` | Execute a process directly (no shell overhead) |
| `readFile(path)` | Read a text file |
| `readJson(path)` | Read and parse a JSON file; returns `{}` on failure |
| `fetch(url, options?)` | HTTP request with configurable timeout (default 2000ms) |
| `ui.log/info/warn/error(...args)` | Write to Sugg logs |
| `cache.get(key, ttlMs?, fetcher?)` | Disk cache with TTL; see Cache section |
| `cache.delete(key)` | Delete a cache entry |

The `CompletionContext` object received by `dynamic` callbacks also provides context fields:

| Field | Purpose |
|---|---|
| `ctx.shell` | Current shell: `"bash" \| "zsh" \| "fish" \| "nushell" \| "powershell"` |
| `ctx.os` | Current OS: `"windows" \| "linux" \| "macos"` |
| `ctx.prefix` | The word currently being typed |
| `ctx.options` | Parsed options already present on the command line |
| `ctx.positionals` | Positional args already consumed on the **current** node (flat, reset on subcommand switch; excludes the in-progress `ctx.prefix`) |

---

## `dynamic` Callback

Dynamic completions must be wrapped in `dynamic(async (ctx) => { ... })`.

All of the following are valid:

```ts
// Inline
args: dynamic(async (ctx) => scanPath(ctx.prefix))

// Variable
const pathArgs = dynamic(async (ctx) => scanPath(ctx.prefix));
args: pathArgs

// Function
function getArgs() { return dynamic(async (ctx) => scanPath(ctx.prefix)); }
```

**Tip:** Extract shared logic into a plain `async function` and call it inside `dynamic`.

---

## Cache

`cache.get` avoids re-running expensive operations (e.g. `exec`, file scans) on every keystroke.

**Key types:**
- `string` — used as-is
- `string[]` — joined with `\0`
- `CompletionContext` — auto-key from `ctx.words` + `ctx.path`; ideal for sharing data across multiple completions in the same command context

```ts
// With fetcher: read cache or fetch and store
args: dynamic(async (ctx) => {
  const [scripts, bins] = await cache.get(ctx, 5000, () =>
    Promise.all([getScriptNames(), getBinNames()])
  );
  return [...scripts, ...bins];
})

// Without fetcher: read-only, returns undefined on miss
const cached = await cache.get("my-key");
```

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

The `args` field on both `OptionNode` and `CommandNode` accepts multiple forms, all unified under an `args_count` model. **No implicit behavior** — every form has explicit `args_count` semantics:

| Form | args_count | Use case |
|---|---|---|
| _omitted_ | `0` | Option is a bool flag, or command takes no positional args |
| `args: []` | `1` | Needs a value, no suggestions |
| `args: ["dev", "build"]` | `1` | Static suggestions (single value) |
| `args: dynamic(...)` | `1` | Dynamic suggestions (single value) |
| `args: { items: ["a", "b"] }` | `1` | Explicit single-value with static items |
| `args: { count: 3, items: [...] }` | `3` | Multi-value option (e.g. `--exclude a b c`) |
| `args: { count: Infinity, items: [...] }` | `u32::MAX` | **Unlimited** positional args (e.g. `npm install pkg1 pkg2 ...`, `git add f1 f2 ...`) |
| `args: { count: 0 }` | `0` | Explicit "no positional args" |

```ts
// Single value (default)
--mode: { items: ["dev", "build"] }

// Multi-value (total capacity 3)
--exclude: { count: 3, items: ["a", "b", "c"] }

// Multi-value with dynamic items
--exclude: { count: 3, items: dynamic(...) }

// Unlimited positional args (e.g. npm install, git add)
install: { count: Infinity, items: dynamic(async ctx => {
  if (ctx.positionals.length === 0) return ["react", "vue", "svelte"];
  return []; // subsequent positions: no per-position suggestion
}) }

// Command with 0 positional args
noPositional: { count: 0, commands: { only: { ... } } }
```

**Rule of thumb**: all forms are **strict** by default — omitting `count: Infinity` means the state machine consumes exactly the declared `count` (default 1), no implicit "unlimited" for dynamic nodes. If your command needs `git remote add <name> <url>` style multi-position completion, declare `count: 2` (or `count: Infinity` if positions are unbounded) explicitly.

The state machine consumes up to `count` tokens for options/commands. When the cap is reached, waiting auto-releases and subsequent tokens walk the normal path. With `count: Infinity` the cap is mathematically unreachable, so the node stays in "is_positional_mode" for the rest of input — dynamic functions should `return []` for positions beyond what they have suggestions for.

### `count` value validation (saturate rules)

The bundler cross-language adapter (`JS f64` → `Rust u32`) protects only two cases — everything else is **fail-loud** (user is responsible for typos):

| `count` value | bundler action | result |
|---|---|---|
| `Infinity` | saturate to `0xFFFFFFFF` | unlimited |
| `> 0xFFFFFFFF` (positive finite) | saturate to `0xFFFFFFFF` | unlimited |
| `NaN` / `-Infinity` / negative (`-5` etc.) | **pass through** | **entire root falls back to `CommandNode::default()`** (empty) |

The "pass through → root collapse" happens because `JSON.stringify(NaN / -Infinity) === "null"` and `JSON.stringify(-5) === "-5"`. serde_json then refuses to deserialize `u32` from those, `build.rs:11` `unwrap_or_default()` kicks in, and the whole completion script vanishes. This is intentional fail-loud: writing `NaN` is a user bug, not something the engine should silently paper over.

---

## Context-Aware Completions

When candidates come from multiple sources (scripts, binaries, files), merge them intelligently rather than dumping everything:

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

Switch data source based on flags:

```ts
args: dynamic(async (ctx) => {
  if (ctx.options["-g"] || ctx.options["--global"]) return getGlobalPackages();
  return getInstalledPackages();
})
```

Different completions per positional position (e.g. `git remote add <name> <url>`). Note `count: 2` is **required** — without it, the state machine only consumes 1 positional and `ctx.positionals.length` never reaches 1:

```ts
add: {
  args: {
    count: 2,  // explicit: 2 positionals (name, url)
    items: dynamic(async (ctx) => {
      if (ctx.positionals.length === 0) return getRemotes();      // first position: name
      if (ctx.positionals.length === 1) return getUrls(ctx.positionals[0]); // second: url, depends on prior
      return [];
    })
  }
}
```

---

## Per-Position Completions with `ctx.positionals`

`ctx.positionals` is the dedicated channel for "what was typed before the current word" on the **current** node. Use it when each positional position has a **different** completion source (e.g. `git remote add <name> <url>` — first position is a remote name, second depends on the name).

### Semantics

| Property | Behavior |
|---|---|
| Shape | `string[]`, flat (not nested) |
| Content | Positional words that the user has **already submitted** (typed a space or Enter) |
| Excluded | The in-progress `ctx.prefix` (the word the user is currently typing) |
| Reset | Cleared on subcommand switch; parent subcommand names are **not** present |
| Multi-value nodes | Repeated consumptions on a `count > 1` node are recorded in order |

### Pattern: position dispatch

```ts
{
  args: {
    count: 2,
    items: dynamic(async ctx => {
      if (ctx.positionals.length === 0) return getRemotes();           // 1st: names
      if (ctx.positionals.length === 1) return getUrls(ctx.positionals[0]); // 2nd: depends on 1st
      return [];                                                      // 3rd+: stop
    })
  }
}
```

### Pattern: build the next suggestion from the prior value

```ts
{
  args: {
    count: 2,
    items: dynamic(async ctx => {
      if (ctx.positionals.length === 0) return ["origin", "upstream", "mine"];
      if (ctx.positionals.length === 1) {
        const name = ctx.positionals[0];
        return [
          `git@github.com:me/${name}.git`,
          `https://github.com/me/${name}.git`,
        ];
      }
      return [];
    })
  }
}
```

### Pattern: stop after a fixed arity

A common shape: first N positions are interactive, anything after is free input. Returning `null` ends the dynamic completion and the shell falls back to whatever the user types. The runtime treats `null`/`undefined` as `[]` (silent, no `ERR` UI), and the TS type allows it.

```ts
{
  args: {
    count: 2,
    items: dynamic(async ctx => {
      if (ctx.positionals.length >= 2) return []; // no suggestion past 2 positions
      return ctx.positionals.length === 0 ? getRemotes() : getUrls(ctx.positionals[0]);
    })
  }
}
```

### Pattern: unlimited positionals with position-aware suggestions

For commands like `git add f1 f2 f3 ...` or `npm install react vue lodash` that take any number of positionals, use `count: Infinity`. The dynamic returns position-specific suggestions for the first few positions and `[]` for the rest (since listing all 100 files wouldn't be useful anyway):

```ts
add: {
  args: {
    count: Infinity,  // unlimited positionals (state machine never "closes" this node)
    items: dynamic(async ctx => {
      if (ctx.positionals.length === 0) return ["."];  // 1st: hint at "add all"
      return [];                                         // 2nd+: user types filenames directly
    })
  }
}
```

### When to use `ctx.positionals` vs `ctx.options`

| Need | Use |
|---|---|
| "I just need a flag/switch value" | `ctx.options["--my-opt"]` |
| "The previous positional is the seed for the next suggestion" | `ctx.positionals[N - 1]` |
| "I need the count of submitted positionals" | `ctx.positionals.length` |
| "I need the full command line for parsing" | `ctx.words` |

### Common mistake: trying to use `ctx.args`

`ctx.args` does **not** exist. The field is `ctx.positionals` to avoid clashing with JavaScript's function `arguments`. Authors coming from generic JS may reflexively write `ctx.args[0]` — that will silently be `undefined` and the dynamic will throw on `.length`. Use `ctx.positionals` from the start.

### `positionals` is empty for the in-progress word

If the user has typed `git remote add or` (no trailing space) and is about to type the rest of the first positional:

- `ctx.positionals` is `[]`
- `ctx.prefix` is `"or"`

The shell client (nushell/zsh/etc.) will filter the dynamic's return by `ctx.prefix`. So the right dynamic for that moment is still "return the first-position list" — the filter happens downstream.

### See also

- [CompletionContext table](#completioncontext) — full field reference
- [Context-Aware Completions](#context-aware-completions) — other context fields (`ctx.options`, `ctx.prefix`)

General template for merging multiple candidate types:

```ts
args: dynamic(async (ctx) => {
  // Fetch all types in parallel, share one cache entry per context
  const [typeA, typeB, typeC] = await cache.get(ctx, 5000, () =>
    Promise.all([getTypeA(ctx), getTypeB(ctx), getTypeC(ctx)])
  );

  const hasHighPriority = typeA.length > 0 || typeB.length > 0;
  if (hasHighPriority) {
    // Only surface low-priority items (typeC) once the user starts typing
    return ctx.prefix ? [...typeA, ...typeB, ...typeC] : [...typeA, ...typeB];
  }
  return [...typeA, ...typeB, ...typeC];
})
```

---

## TypeScript Reuse & Organization

Three tiers:

| Tier | Condition | Practice |
|------|-----------|----------|
| **Inline** | Small structure (~20 lines), used once | Keep it in-place |
| **Extract** | Large structure (20+ lines), even if used once | Extract for readability |
| **Must extract** | Shared across multiple references | Always extract |

```ts
// ⚠️ Small, used once → inline
// ❌ Unnecessary extraction
const fooArgs = dynamic(async (ctx) => scanPath(ctx.prefix));
cmd: { args: fooArgs }

// ✅ Inline
cmd: { args: dynamic(async (ctx) => scanPath(ctx.prefix)) }

// ⚠️ Large, used once → extract for readability
// bunCommands is 150+ lines — pulling it out keeps createCompletion readable
const bunCommands: Record<string, CommandNode> = { ... };
export default createCompletion({ bun: { commands: bunCommands } });
```

#### Extract shared options

```ts
const globalOpts: OptionNode[] = [
  { labels: ["-h", "--help"], description: "Show help" },
];
const installOpts: OptionNode[] = [
  ...globalOpts,
  { labels: ["-D", "--save-dev"], description: "Save as devDependency" },
];
```

#### Reuse command objects

```ts
const addCmd: CommandNode = {
  description: "Add a dependency",
  options: installOpts,
  args: dynamic(async () => { /* ... */ }),
};

export default createCompletion({
  bun: {
    commands: {
      add: addCmd,
      a: { ...addCmd, description: "Alias for add" },
    },
  },
});
```

#### Wrap shared logic in plain async functions

```ts
async function getScripts(): Promise<Suggestion[]> {
  const pkg = await readJson("package.json");
  return Object.keys(pkg.scripts || {}).map(s => ({ display: s }));
}

args: dynamic(async (ctx) => {
  const [scripts, files] = await Promise.all([
    cache.get(ctx, 5000, getScripts),
    scanPath(ctx.prefix),
  ]);
  return [...scripts, ...files];
})
```

---

## Common Mistakes

- ❌ Missing the top-level `{ commandName: ... }` wrapper
- ❌ Manually filtering by `ctx.prefix` inside `dynamic`
- ❌ Sequential `await` for independent async calls (use `Promise.all`)
- ❌ Manually splitting `scanPath` input (the engine handles directory boundaries)
- ❌ Omitting `args` on options that take a value (write `args: []` even with no suggestions)
- ❌ Using placeholders like `<file>` in `args` (use `args: []` instead)
- ❌ i18n keys containing `.`
- ❌ Unnecessary variable extraction of single-use small structures — inline ~20 lines or fewer; extract only large or shared logic

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