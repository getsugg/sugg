<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="assets/logo-horizontal.svg">
    <img src="assets/logo-horizontal.svg" width="420" alt="Sugg">
  </picture>
</p>

**English** | [简体中文](./README.zh-CN.md)

**Sugg** is a lightweight, high-performance shell completion engine written in Rust. It enables developers to author complex shell completion scripts in modern TypeScript/JavaScript, compiling them into precompiled bytecode with zero-copy (`rkyv`) caching to deliver near-zero latency completions in terminal environments.

## Features

- **Low-Latency Architecture**: Completely decoupled parsing and compilation. The lightweight runtime frontend (`sugg`) uses `rkyv` for zero-copy binary cache lookups, while dynamic callbacks are evaluated on `QuickJS` using precompiled bytecode.
- **Multi-Shell & Alias Resolution**: Author once, support Zsh, Fish, Bash, Nushell, and PowerShell. Native support for shell aliases (e.g., if a user configures `alias g=git`, Sugg correctly routes completion to the underlying `git` completion schema).
- **Context-Aware Caching**: Offers command-context-level caching APIs (`cache.get(ctx, ttl)`), ensuring that expensive external commands are not executed repeatedly during rapid keystrokes within the same command lifecycle.
- **Direct Native Execution**: Injects process spawning APIs (`execFile`) directly into the JS environment to bypass the overhead of shell fork routines and avoid escaping issues, alongside a fast, unified path scanning API (`scanPath`).
- **State & Flag Sensitivity**: Callbacks receive fully-resolved CLI option payloads (`ctx.options`), allowing the schema to dynamically toggle candidate data sources based on the presence of user flags.
- **AI-Driven Schema Generation**: Built-in AI-assisted helper workflows (`collect-cli-help.md`) designed to recursively extract raw `--help` outputs for LLMs to generate complex completion schemas.
- **In-Line UI Logging**: Safe logging interfaces (`ui.warn`, `ui.info`) that render debugging and warning info directly in-line near the terminal completion list, preventing command line corruption.
- **Native Localization (i18n)**: Out-of-the-box multilingual support backed by ICU4X, automatically serving fallback locale strings as completion descriptions.

## Developer Experience

Leveraging its embedded Oxc AST parser and Rolldown bundler, Sugg allows authors to write highly modular, context-aware completion scripts with type safety.

The following is an annotated snippet of the actual `bun` completion schema showcasing several core Sugg APIs:

```typescript
// completions/bun/index.ts
import * as t from "virtual:i18n/bun";
import { execFile, scanPath, ui, cache } from "sugg"; // ESM imports

// [2] Externalized Helpers: Direct process spawning via execFile (no shell overhead, secure arg arrays)
async function getScriptNames(): Promise<Suggestion[]> {
  const out = await execFile("bun", ["getcompletes", "z"]);
  const scripts = out.trim().split("\n").filter(l => l.includes("\t")).map(l => l.split("\t")[0]);
  return scripts.map(s => ({ display: s, description: t.suggestion_script }));
}

async function getBinNames(): Promise<Suggestion[]> {
  const out = await execFile("bun", ["getcompletes", "b"]);
  const bins = out.trim().split(/\s+/).filter(Boolean);
  return bins.map(b => ({ display: b, description: t.suggestion_package_bin }));
}

// [3] Smart Scanning: Fast filesystem scans with scanPath
async function getScriptFiles(ctx: CompletionContext) {
  const allFiles = await scanPath(ctx.prefix);
  return allFiles.filter(
    (f) => /\.(ts|js|tsx|jsx|mjs|cjs)$/i.test(f.display) || f.isDir
  );
}

export default createCompletion({
  bun: {
    description: t.description,
    options: commonGlobalOpts,
    commands: {
      run: {
        description: t.cmd_run_desc,
        args: dynamic(async (ctx) => {
          // [4] In-Line Logging: Write multiple variables directly to terminal completion frames
          ui.log("Fetching run candidates. Prefix:", ctx.prefix, "Shell:", ctx.shell);

          // [5] Parallel Caching: Cache scripts and bins under the current command context
          // Leave file checks un-cached since they are highly dependent on live, shifting prefix inputs
          let [[scripts, bins], files] = await Promise.all([
            cache.get(ctx, 5000, () => Promise.all([getScriptNames(), getBinNames()])),
            getScriptFiles(ctx),
          ]);

          // [6] Intelligent Filtering & Context Matching
          if (ctx.prefix) {
            scripts = scripts.filter((s) => s.display.startsWith(ctx.prefix));
            bins = bins.filter((b) => b.display.startsWith(ctx.prefix));
          }

          // Clean up files when semantic matches exist to maintain a readable UI
          if (scripts.length + bins.length > 0) {
            // Hide folders when no prefix is entered, preventing menu flooding
            if (ctx.prefix === "") {
              files = files.filter((f) => !f.isDir);
            }
            files = files.map((f) => ({
              ...f,
              description: t.suggestion_file,
            }));
          }

          return [...scripts, ...files, ...bins];
        }),
        options: [...commonGlobalOpts],
      },
    }
  }
});
```

## Installation

Run the install script, which automatically downloads the prebuilt binary matching your platform.

### Linux & macOS

```bash
curl -fsSL https://raw.githubusercontent.com/getsugg/sugg/main/scripts/install.sh | bash
```

### Windows (PowerShell / Nushell)

```powershell
irm https://raw.githubusercontent.com/getsugg/sugg/main/scripts/install.ps1 | iex
```

### Build from Source

```bash
cargo run -p sugg-deploy --release -- --add-path
```

## Quick Start

### 1. Initialize Shell Integration

Inject the engine script into your shell profile. For instance, in Zsh:

```bash
# Add this line to your ~/.zshrc
eval "$(sugg init zsh)"
```

You can run `sugg init <shell>` (supports `bash`, `zsh`, `fish`, `nushell`, `powershell`) to display integration guidelines.

### 2. Scaffold and Compile Completion Schemas

Step 1: Navigate to the default completions directory and initialize your development workspace (this generates type definitions and developer guidelines):
```bash
cd ~/.sugg/completions
sugg dev init
```

Step 2: The `.sugg/` directory now contains rich context files. If you are unfamiliar with Sugg's TS schema syntax, you can upload `.sugg/sugg-guide.md` and `.sugg/sugg.d.ts` to your AI assistant (such as ChatGPT or Claude) and send the following prompt:

> "Referencing the API design and principles in `.sugg/sugg-guide.md`, please write a high-performance completion schema for `[Replace with your CLI tool name, e.g., bun]`, and provide multilingual description translations via `en.json` and `zh.json` inside the `i18n/` directory."

Step 3: Once authored, bundle and compile your schemas into the local binary cache to apply the changes instantly:
```bash
sugg reload
```

## Architecture

Sugg decouples completion evaluations from compilation to maintain near-zero rendering latency:

- **`sugg` (Frontend CLI)**: A highly-optimized, small binary called on keystroke triggers. It maps current input contexts onto compiled cached graphs via memory mapping (`mmap`) to output shell-specific payloads instantly.
- **`sugg-engine` (Backend Engine)**：Houses larger compilers and bundlers (`Oxc` AST parser, `Rolldown` JS bundler). It only triggers on schema compilations via `sugg reload`, ensuring zero performance overhead during daily shell usage.

## Command Reference

- `sugg init <shell>`: Print shell integration script.
- `sugg reload`: Bundles TS/JS schemas into local binary cached graphs.
- `sugg commands`: Lists all cached top-level CLI command schemas.
- `sugg dev init`: Writes type declarations, guides, and `tsconfig.json` files.
- `sugg dev i18n`: Scans JSON locales to generate typed declarations for `virtual:i18n/*`.
- `sugg upgrade`: Automatically self-upgrades Sugg to the latest release on GitHub.

## License

This project is licensed under the [MIT License](LICENSE).
