// Typex 入口：单实例 → 托盘 → 窗口 → 服务装配（06 §5.1 手工装配）
#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

fn main() {
    #[cfg(all(target_os = "windows", debug_assertions))]
    typex_lib::platform::windows::attach_parent_console();

    typex_lib::run();
}
