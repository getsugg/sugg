def run_task [name: string, block: closure] {
    print $"(ansi cyan_bold)====================================================(ansi reset)"
    print $"(ansi yellow_bold) 运行任务: ($name)(ansi reset)"
    print $"(ansi cyan_bold)----------------------------------------------------(ansi reset)"
    
    let start = (date now)
    # 直接运行 block，让其自然流式输出到终端
    do $block
    let duration = (date now) - $start
    
    print $"(ansi cyan_bold)----------------------------------------------------(ansi reset)"
    print $"(ansi green) 完成! 耗时: ($duration)(ansi reset)\n"
    
    return { task: $name, duration: $duration }
}

# 任务列表定义
let tasks = [
    { name: "Reload",       cmd: {|| C:/Users/Sun-Q/AppData/Roaming/sugg/bin/sugg.exe reload } }
    { name: "PNPM Install", cmd: {|| C:/Users/Sun-Q/AppData/Roaming/sugg/bin/sugg.exe complete nushell -- "pnpm install -"} }
    { name: "PNPM Install Path", cmd: {|| sugg complete nushell -- "pnpm install -"} }
    
    { name: "Bun I",      cmd: {|| C:/Users/Sun-Q/AppData/Roaming/sugg/bin/sugg.exe complete nushell -- "bun i" } }
    { name: "Bun Run",      cmd: {|| C:/Users/Sun-Q/AppData/Roaming/sugg/bin/sugg.exe complete nushell -- "bun run " } }
    { name: "Bun -c",      cmd: {|| C:/Users/Sun-Q/AppData/Roaming/sugg/bin/sugg.exe complete nushell -- "bun a --c" } }

    # { name: "Release Test", cmd: { ./target/release/sugg completions "pnpm run d" } }
]

# 执行并收集统计数据
let stats = ($tasks | each {|it| run_task $it.name $it.cmd })

# 最后打印一个整齐的统计表
print $"(ansi purple_bold)全部任务执行完毕，性能汇总：(ansi reset)"
$stats | table
