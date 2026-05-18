declare const i18n: {
  readonly docker: {
    /**
     * - 🚩 **en**: Running container
     * - **zh**: 运行中的容器
    */
    readonly container_desc: string;
    /**
     * - 🚩 **en**: List containers
     * - **zh**: 列出容器
    */
    readonly ps_desc: string;
    /**
     * - 🚩 **en**: Run a container
     * - **zh**: 运行容器
    */
    readonly run_desc: string;
    /**
     * - 🚩 **en**: Stop a container
     * - **zh**: 停止容器
    */
    readonly stop_desc: string;
  };
  readonly greet: {
    /**
     * - 🚩 **en**: Install dependencies
     * - **zh**: 安装依赖
    */
    readonly install: string;
    /**
     * - 🚩 **en**: Run a script
     * - **zh**: 运行脚本
    */
    readonly run: string;
  };
  readonly greet_dynamic: {
    /**
     * - 🚩 **en**: Bye description
     * - **zh**: 再见描述
    */
    readonly bye_desc: string;
    /**
     * - 🚩 **en**: Hello description
     * - **zh**: 你好描述
    */
    readonly hello_desc: string;
  };
  readonly [key: string]: any;
};
