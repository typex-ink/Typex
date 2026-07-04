// Typex 入口：单实例 → 托盘 → 窗口 → 服务装配（07 §5.1 手工装配）
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    typex_lib::run();
}
