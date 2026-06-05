# Sugg

[English](./README.md) | **简体中文**

Sugg 是一个基于 Rust 编写的 Shell 补全引擎。它允许开发者使用 TypeScript/JavaScript 来编写复杂的补全逻辑，并通过预编译字节码和内存零拷贝（Zero-copy）技术，提供低延迟的终端交互体验。

## 核心特性

- **低延迟架构**：解析与构建完全解耦。运行时前端 (`sugg`) 通过 `rkyv` 零拷贝读取二进制缓存，动态逻辑使用 `QuickJS` 执行预编译字节码。
- **多 Shell 与原生别名支持**：一套补全脚本即可支持 Zsh、Fish、Bash、Nushell 和 PowerShell。内置别名解析机制（如用户配置 `alias g=git`，引擎仍能正确提供补全）。
- **上下文感知缓存**：提供精确到命令行上下文的缓存 API (`cache.get(ctx, ttl)`)，确保在同一条命令中连续触发补全时，不重复调用耗时的外部命令。
- **高性能原生 API**：为 JS 注入了绕过 Shell Fork 开销的进程直调 API（`execFile`），以及极速的智能路径扫描 API（`scanPath`）。
- **参数与状态感知**：补全回调可实时获取引擎安全解析的输入参数（`ctx.options`），根据是否携带特定标志（Flag）返回不同数据。
- **AI 辅助工作流**：内置基于 AI 的命令行文档提取流 (`collect-cli-help.md`)，帮助开发者半自动生成繁杂的 CLI 补全树。
- **终端内联 UI 日志**：提供 `ui.warn`、`ui.info` 等接口，在补全执行异常或需要调试时，日志会安全地渲染在补全菜单周围，不破坏终端当前画面。
- **国际化（i18n）**：基于 ICU4X 实现按需回退的多语言支持，可为补全项提供本地化描述。

## 强大的开发体验

得益于内置的 Oxc 和 Rolldown 打包器，你可以使用现代 TypeScript 编写高度模块化、上下文感知的补全逻辑。

以下为 `bun` 补全脚本的真实核心片段，展示了 Sugg 的多项高级特性：

```typescript
// completions/bun/index.ts
import * as t from "virtual:i18n/bun";
import { execFile, scanPath, ui, cache } from "sugg"; // ESM 显式导入

// [2] 辅函数外置：利用 execFile 原生进程直调（无 Shell 开销，参数安全传递）
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

// [3] 原生路径扫描：利用 scanPath 大一统路径扫描 API
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
          // [4] 内联 UI 日志：在终端补全菜单上方安全打印多变量调试信息
          ui.log("Fetching run candidates. Prefix:", ctx.prefix, "Shell:", ctx.shell);

          // [5] 上下文缓存与并行：对 scripts/bins 做毫秒级上下文缓存（按 Tab 键时秒开）
          // 涉及当前实时输入的 prefix 动态变化的文件扫描（getScriptFiles），则不予缓存
          let [[scripts, bins], files] = await Promise.all([
            cache.get(ctx, 5000, () => Promise.all([getScriptNames(), getBinNames()])),
            getScriptFiles(ctx),
          ]);

          // [6] 上下文匹配过滤与 UX 决策
          if (ctx.prefix) {
            scripts = scripts.filter((s) => s.display.startsWith(ctx.prefix));
            bins = bins.filter((b) => b.display.startsWith(ctx.prefix));
          }

          // 当存在匹配的脚本时，智能过滤并标记文件列表，防止首屏被乱七八糟的子目录填满
          if (scripts.length + bins.length > 0) {
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

## 安装

Sugg 提供了安装脚本，自动拉取对应平台的最新二进制文件。

### Linux & macOS

```bash
curl -fsSL https://raw.githubusercontent.com/axuj/sugg/main/scripts/install.sh | bash
```

### Windows (PowerShell / Nushell)

```powershell
irm https://raw.githubusercontent.com/axuj/sugg/main/scripts/install.ps1 | iex
```

### 从源码编译安装

```bash
cargo run -p sugg-deploy --release -- --add-path
```

## 快速开始

### 1. 注入 Shell 集成

安装完成后，需要将 Sugg 注入到你的 Shell 配置文件中。以 `zsh` 为例：

```bash
# 将此行添加到 ~/.zshrc
eval "$(sugg init zsh)"
```

可以直接运行 `sugg init <shell>`（支持 `bash`, `zsh`, `fish`, `nushell`, `powershell`）查看对应的配置指引。

### 2. 编写与编译补全脚本

第一步：进入默认补全配置目录，并初始化开发工作区（生成类型定义、开发指南与 AI 辅助文档）：
```bash
cd ~/.sugg/completions
sugg dev init
```

第二步：此时 `.sugg/` 目录下已生成完整的上下文文档。如果你对编写规范不熟悉，可以直接将 `.sugg/sugg-guide.md` 和 `.sugg/sugg.d.ts` 作为上下文附件上传给 AI（如 ChatGPT, Claude 等），并发送以下 Prompt：

> "请参考 `.sugg/sugg-guide.md` 中的 API 规范和设计原则，为 `[请替换为你想编写的命令行工具名称，如: bun]` 编写一个高性能的补全脚本，并在 `i18n/` 目录下分别提供 `en.json` 与 `zh.json` 的国际化翻译文件。"

第三步：编写完成后，编译所有脚本并写入本地二进制缓存以立即使其生效：
```bash
sugg reload
```

## 架构说明

Sugg 将补全的解析阶段与构建阶段严格分离：

- **`sugg` (前端 CLI)**：极轻量级可执行文件。在用户按下 Tab 键时触发，解析输入上下文，通过内存映射（`mmap`）瞬时查询二进制缓存树并输出对应 Shell 协议的补全项。
- **`sugg-engine` (后端引擎)**：包含 `Oxc` (AST分析)、`Rolldown` (JS打包) 等依赖。仅在执行 `sugg reload` 编译脚本时运行，彻底消除日常终端补全的性能损耗。

## 命令参考

- `sugg init <shell>`: 打印指定 Shell 的集成脚本。
- `sugg reload`: 重新打包补全脚本并生成二进制缓存。
- `sugg commands`: 打印当前已缓存的所有顶级命令。
- `sugg dev init`: 生成开发环境文件（类型声明、指南及 `tsconfig.json`）。
- `sugg dev i18n`: 扫描目录并从 JSON 翻译文件生成 `i18n.d.ts` 类型声明。
- `sugg upgrade`: 自动检查并升级 Sugg 到 GitHub 上的最新版本。

## 许可证

本项目基于 [MIT License](LICENSE) 许可协议开源。
