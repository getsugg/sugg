type ShellName = "bash" | "zsh" | "fish" | "nushell" | "powershell";
type OsName = "windows" | "linux" | "macos";

interface CompletionContext {
  prefix: string;
  path: string;
  words: string[];

  /**
   * 引擎安全解析出的参数表（自动处理了传值和别名映射，支持多值收集）
   *
   * - 布尔型参数：如果出现，值为 true（不管输入多少次）
   * - 传值型参数：永远为 string[] 数组（出现一次是单元素数组，多次出现则包含所有值）
   *
   * 同一选项的所有别名都会被收集，脚本作者只需检查任意一个 key 即可覆盖所有别名输入。
   *
   * 例如：\
   * // 布尔选项：直接判断 \
   * ctx.options["-g"]  = true \
   * // 传值选项值为 string[]： \
   * ctx.options["--cwd"]  = ["mydir"] \
   * ctx.options["--exclude"]  = ["react", "vue"]
   */
  options: Record<string, true | string[]>;

  /** 当前 Shell 名称 */
  shell: ShellName;
  /** 当前操作系统 */
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
  /**
   * 在补全菜单中显示的主文本（必填）。
   */
  display: string;

  /**
   * 实际插入到命令行的值。
   * 少数情况下使用（当插入值与显示值不同时）。
   * 如果省略，则最终 value = display + 末尾空格。
   */
  value?: string;

  description?: string;

  /**
   * 别名列表：用户在命令行中输入这些字符串时，
   * 菜单里仍然显示这个建议，并且插入 value。
   * 别名本身不会作为插入值，除非 value 就是它。
   */
  aliases?: string[];
  style?: SuggestionStyle;
}

const DynamicBrand: unique symbol;
type DynamicCommand = { [DynamicBrand]: never };

type SuggestionResult = string[] | Suggestion[] | Promise<string[] | Suggestion[]>;

interface OptionNode {
  /** 选项别名列表，如 ['-v', '--verbose'] */
  labels: string[];
  description?: string;
  style?: SuggestionStyle;
  args?: string[] | Suggestion[] | DynamicCommand;
}

interface CommandNode {
  description?: string;
  aliases?: string[];
  style?: SuggestionStyle;
  options?: OptionNode[];
  /** 静态子命令映射 */
  commands?: Record<string, CommandNode>;
  args?: string[] | Suggestion[] | DynamicCommand;
}

function createCompletion(config: Record<string, CommandNode>): Record<string, CommandNode>;

/**
 * 标记动态补全回调。回调可返回 string[]、Suggestion[] 或其 Promise。
 */
function dynamic(callback: (ctx: CompletionContext) => SuggestionResult): DynamicCommand;

interface ScanDirItem {
  display: string;
  value: string;
  isDir: boolean;
  style?: SuggestionStyle;
}

/**
 * 大一统路径扫描 API。
 * - 传一个参数时，自动解析用户输入的路径片段，扫描当前目录下匹配项。
 * - 传两个参数时，baseDir 指定虚拟根目录（如 "node_modules/.bin"），input 为用户输入。
 * - 如果 input 以斜杠结尾（如 "src/"），则扫描该子目录并保留其前缀。
 * - 如果 input 包含斜杠（如 "src/com"），自动拆分目录和文件名前缀。
 * - 如果 input 是纯文件名片段（如 "te"），扫描当前目录。
 */
function scanPath(input: string, baseDir?: string): Promise<ScanDirItem[]>;
function readJson(path: string): Promise<any>;
function exec(cmd: string): Promise<string>;

/**
 * 极速直接进程拉起 API——不经过 Shell，零中间进程损耗。
 *
 * 与 `exec`（走 sh -c / cmd /C）不同，execFile 直接把
 * 可执行文件路径和参数数组传给操作系统内核，避免了：
 *
 * 1. **Shell 进程开销**：不 fork sh/cmd，每次省去几毫秒
 * 2. **命令注入风险**：参数以数组形式传递，而非拼接到字符串中
 * 3. **参数解析歧义**：不存在引号嵌套、转义字符的困扰
 *
 * 典型用法：
 * ```ts
 * // 获取 git 状态（纯命令，无需 Shell 特性）
 * const out = await execFile("git", ["status", "--porcelain"]);
 *
 * // 获取当前 git 分支
 * const branch = await execFile("git", ["rev-parse", "--abbrev-ref", "HEAD"]);
 * ```
 *
 * @param cmd  可执行文件路径（如 "git"、"node"、"/usr/bin/ls"）
 * @param args 参数数组（每个元素对应一个 argv 条目，无需手动转义）
 * @returns    命令标准输出（stdout）的字符串内容
 *
 * @see exec 需要管道 |、重定向 >、变量展开 $HOME 等 Shell 功能时用回 exec
 */
function execFile(cmd: string, args?: string[]): Promise<string>;
interface Ui {
  log(...args: any[]): void;
  info(...args: any[]): void;
  warn(...args: any[]): void;
  error(...args: any[]): void;
}
const ui: Ui;
declare module "virtual:i18n/*" {
  const value: any;
  export = value;
}
