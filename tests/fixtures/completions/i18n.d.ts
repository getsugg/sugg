declare module "virtual:i18n/docker" {
  /**
   * - 🚩 **en**: Running container
   * - **zh**: 运行中的容器
   */
  export const container_desc: string;
  /**
   * - 🚩 **en**: List containers
   * - **zh**: 列出容器
   */
  export const ps_desc: string;
  /**
   * - 🚩 **en**: Run a container
   * - **zh**: 运行容器
   */
  export const run_desc: string;
  /**
   * - 🚩 **en**: Stop a container
   * - **zh**: 停止容器
   */
  export const stop_desc: string;
}

declare module "virtual:i18n/greet" {
  /**
   * - 🚩 **en**: Greet command description
   * - **zh**: 问候命令描述
   */
  export const description: string;
  /**
   * - 🚩 **en**: Install dependencies
   * - **zh**: 安装依赖
   */
  export const install: string;
  /**
   * - 🚩 **en**: Run a script
   * - **zh**: 运行脚本
   */
  export const run: string;
}

declare module "virtual:i18n/greet_dynamic" {
  /**
   * - 🚩 **en**: Bye description
   * - **zh**: 再见描述
   */
  export const bye_desc: string;
  /**
   * - 🚩 **en**: Hello description
   * - **zh**: 你好描述
   */
  export const hello_desc: string;
}
