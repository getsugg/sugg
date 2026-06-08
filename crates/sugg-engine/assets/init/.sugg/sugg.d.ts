type ShellName = "bash" | "zsh" | "fish" | "nushell" | "powershell";
type OsName = "windows" | "linux" | "macos";

interface CompletionContext {
  prefix: string;
  path: string;
  words: string[];

  /**
   * Parsed options. All aliases are merged into one key, so check any label.
   * - Bool flag: `true` (regardless of repetition)
   * - Value-taking: always `string[]` (one occurrence → `[v]`, repeated → all values)
   */
  options: Record<string, true | string[]>;

  /**
   * Submitted positional args on the current node (flat, reset on subcommand switch).
   * - Excludes the in-progress `prefix`
   * - Resets when walking into a subcommand
   * - Repeated consumptions on `args_count > 1` nodes are recorded in order
   */
  positionals: string[];

  /** Current shell */
  shell: ShellName;
  /** Current OS */
  os: OsName;
}

type Color =
  | "black"
  | "red"
  | "green"
  | "yellow"
  | "blue"
  | "magenta"
  | "cyan"
  | "white"
  | "bright_black"
  | "bright_red"
  | "bright_green"
  | "bright_yellow"
  | "bright_blue"
  | "bright_magenta"
  | "bright_cyan"
  | "bright_white";

interface SuggestionStyle {
  fg?: Color;
  bg?: Color;
  attr?: ("bold" | "italic" | "underline" | "dim")[];
}

interface Suggestion {
  /** Displayed text in the completion menu (required). */
  display: string;

  /**
   * Value actually inserted into the command line.
   * Omit when display == value; otherwise the inserted string differs from what the user sees.
   * A trailing space is appended by default.
   */
  value?: string;

  description?: string;

  /**
   * Extra input strings that should also accept this suggestion and insert `value`.
   * The aliases themselves are not inserted unless they equal `value`.
   */
  aliases?: string[];
  style?: SuggestionStyle;
}

const DynamicBrand: unique symbol;
type DynamicCommand = { [DynamicBrand]: never };

type SuggestionResult = string[] | Suggestion[] | Promise<string[] | Suggestion[]>;

interface ArgsSpec {
  /**
   * Total tokens this node consumes.
   * - `0`: consumes none (bool flag / command with no positionals)
   * - `1`: single value (default)
   * - `N`: fixed N values
   * - `Infinity`: unlimited (internally `u32::MAX`)
   *
   * Use a positive integer or `Infinity`. NaN / negative numbers will collapse the
   * entire root to empty at bundle time (fail-loud).
   */
  count?: number;
  /** Suggestions (static or dynamic). Omit to require a value with no suggestions. */
  items?: string[] | Suggestion[] | DynamicCommand;
}

interface OptionNode {
  /** Option labels, e.g. `['-v', '--verbose']`. */
  labels: string[];
  description?: string;
  style?: SuggestionStyle;
  args?: string[] | Suggestion[] | DynamicCommand | ArgsSpec;
}

interface CommandNode {
  description?: string;
  aliases?: string[];
  style?: SuggestionStyle;
  options?: OptionNode[];
  /** Static subcommand map. */
  commands?: Record<string, CommandNode>;
  args?: string[] | Suggestion[] | DynamicCommand | ArgsSpec;
}

function createCompletion(config: Record<string, CommandNode>): Record<string, CommandNode>;

/**
 * Mark a dynamic completion callback. May return `string[]`, `Suggestion[]`, or a Promise thereof.
 * Returning `[]` means "no suggestions" (silent, no error UI).
 */
function dynamic(callback: (ctx: CompletionContext) => SuggestionResult): DynamicCommand;

declare module "sugg" {
  export interface FetchOptions {
    method?: "GET" | "POST" | "PUT" | "DELETE" | "PATCH";
    headers?: Record<string, string>;
    body?: string;
    timeout?: number;
  }

  export interface FetchResponse {
    ok: boolean;
    status: number;
    statusText: string;
    headers: { get(name: string): string | null };
    text(): Promise<string>;
    json(): Promise<any>;
  }

  /**
   * Lightweight HTTP request with a configurable timeout (default 2000ms) to avoid
   * hanging the terminal during completion.
   */
  export function fetch(url: string, options?: FetchOptions): Promise<FetchResponse>;

  /** Run a command through a shell (sh -c / cmd /C). Use for pipes, redirects, `$VAR` expansion. */
  export function exec(cmd: string): Promise<string>;

  /**
   * Spawn a process directly (no shell). Faster than `exec` and immune to shell-quoting issues.
   * @param cmd  Executable path (e.g. `"git"`, `"node"`, `"/usr/bin/ls"`).
   * @param args Argument vector; no manual escaping required.
   * @returns    Standard output as a string.
   */
  export function execFile(cmd: string, args?: string[]): Promise<string>;

  export interface ScanDirItem {
    display: string;
    value: string;
    isDir: boolean;
    style?: SuggestionStyle;
  }

  /**
   * Unified path scanner. With one arg, scans the current directory matching the input fragment.
   * With two args, `baseDir` sets a virtual root (e.g. `"node_modules/.bin"`).
   * Handles directory prefixes transparently:
   * - trailing `/` (e.g. `"src/"`) keeps the prefix and lists the subdirectory
   * - mid slash (e.g. `"src/com"`) splits dir and file prefix
   * - bare fragment (e.g. `"te"`) scans the current dir
   */
  export function scanPath(input: string, baseDir?: string): Promise<ScanDirItem[]>;
  export function readJson(path: string): Promise<any>;
  export function readFile(path: string): Promise<string>;

  export interface Ui {
    log(...args: any[]): void;
    info(...args: any[]): void;
    warn(...args: any[]): void;
    error(...args: any[]): void;
  }

  /** Inline terminal log, surfaces colored output in the completion menu. */
  export const ui: Ui;

  interface CacheHelper {
    /**
     * Read-through cache. Keys are isolated per script file — two scripts
     * using the same key string will not collide.
     *
     * `key` accepts:
     * - `string` — used as-is (automatically namespaced per script — no manual prefix needed)
     * - `string[]` — joined as the cache key
     * - `CompletionContext` — auto-keyed from `ctx.words` (minus the in-progress prefix) + `ctx.path`,
     *   ideal for sharing data across multiple completions in the same command context.
     *
     * @example
     * // Both "bun run b" and "bun run bu" hit the same cache entry:
     * const [scripts, bins] = await cache.get(ctx, 5000, () =>
     *   Promise.all([getScriptNames(), getBinNames()])
     * );
     */
    get<T>(
      key: string | string[] | CompletionContext,
      ttlMs: number,
      fetcher: () => T | Promise<T>,
    ): Promise<T>;
    get<T>(key: string | string[] | CompletionContext): Promise<T | undefined>;
    delete(key: string | string[] | CompletionContext): void;
  }

  export const cache: CacheHelper;
}

declare module "virtual:i18n/*" {
  const value: any;
  export = value;
}
