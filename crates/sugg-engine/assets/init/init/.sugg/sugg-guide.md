# Sugg Completion Script Guide

Sugg completion scripts are written in TypeScript, placed in the `completions/` directory. The engine is built on QuickJS + Rolldown. Most global APIs are injected automatically — **no manual imports needed** (except i18n virtual modules).

For full type signatures (`CommandNode`, `OptionNode`, `Suggestion`, `DynamicCommand`, etc.), read `sugg.d.ts` directly.

---

## Core Rules

1. Every script must export `createCompletion({ commandName: { ... } })`. The top-level key is the command name (e.g. `git.ts` → `{ git: { ... } }`).
2. Use TypeScript freely — extract constants, helper functions, and reusable `dynamic` callbacks.
3. Prefer `Promise.all` over sequential `await` for independent async operations.

---

## Global API Reference

| Function / Object | Purpose |
|---|---|
| `scanPath(input, baseDir?)` | Smart path/file scanner, auto-resolves directory prefix |
| `exec(cmd)` | Run a shell command, returns stdout string |
| `execFile(cmd, args)` | Execute a process directly (no shell overhead) |
| `readFile(path)` | Read a text file |
| `readJson(path)` | Read and parse a JSON file; returns `{}` on failure |
| `ui.log/info/warn/error(...args)` | Write to Sugg logs |
| `cache.get(key, ttlMs?, fetcher?)` | Disk cache with TTL; see Cache section |
| `cache.delete(key)` | Delete a cache entry |
| `ctx.shell` | Current shell: `"bash" \| "zsh" \| "fish" \| "nushell" \| "powershell"` |
| `ctx.os` | Current OS: `"windows" \| "linux" \| "macos"` |
| `ctx.prefix` | The word currently being typed |
| `ctx.options` | Parsed options already present on the command line |

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

- Language is selected at build time via `sugg reload --lang zh`.
- Fallback order: specified lang → `en` → first available.
- Run `sugg dev i18n` to generate type declarations for `virtual:i18n/*`.

---

## Return Value Rules

- Return `string[]` or `Suggestion[]`.
- The engine filters by `ctx.prefix` automatically — **do not filter manually**.
- A trailing space is appended to `display` by default. To suppress it, provide an explicit `value`.
- Options that take a value but have no suggestions: use `args: []` — the engine will wait for free input.

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

## TypeScript Reuse Patterns

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

---

## Full Example

```ts
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