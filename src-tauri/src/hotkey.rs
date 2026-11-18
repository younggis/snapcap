//! 全局热键注册与分发（§M0）
//!
//! 用 `tauri-plugin-global-shortcut` 注册配置中的组合键；按下时映射为动作并
//! 发出 `hotkey:triggered` 事件，由 UI 决定调用哪个命令（overlay / 录制等）。

use crate::config::Settings;
use crate::state::AppState;
use anyhow::{anyhow, Context, Result};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, Runtime};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};

/// 热键动作（与 UI `types.ts` 的 snake_case 约定一致）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HotkeyAction {
    Region,
    Full,
    Window,
    Record,
    Pause,
    Annotate,
}

/// 收集 (组合键, 动作) 列表。
pub fn combos(settings: &Settings) -> Vec<(String, HotkeyAction)> {
    let h = &settings.hotkeys;
    vec![
        (h.region.clone(), HotkeyAction::Region),
        (h.full.clone(), HotkeyAction::Full),
        (h.window.clone(), HotkeyAction::Window),
        (h.record.clone(), HotkeyAction::Record),
        (h.pause.clone(), HotkeyAction::Pause),
        (h.annotate.clone(), HotkeyAction::Annotate),
    ]
}

/// 由组合键字符串反查动作（比较时忽略大小写和空格）。
pub fn action_for_combo(combo: &str, settings: &Settings) -> Option<HotkeyAction> {
    // Tauri/global-hotkey 把按键序列化为 "alt+KeyA" / "ctrl+Digit1"，
    // 而配置里写的是 "Alt+A"。统一去掉 Key/Digit 前缀再比较。
    let normalize = |s: &str| {
        let s: String = s.chars().filter(|c| !c.is_whitespace()).collect();
        s.split('+')
            .map(|part| {
                let p = part.trim().to_lowercase();
                if let Some(k) = p.strip_prefix("key") {
                    k.to_string()
                } else if let Some(d) = p.strip_prefix("digit") {
                    d.to_string()
                } else {
                    p
                }
            })
            .collect::<Vec<_>>()
            .join("+")
    };
    let target = normalize(combo);
    combos(settings)
        .into_iter()
        .find(|(c, _)| normalize(c) == target)
        .map(|(_, a)| a)
}

/// 安装全局按键处理器并注册配置中的所有组合键。
/// `on_shortcuts` 内部会完成注册，因此无需再单独调用 `register`。
pub fn install_handler<R: Runtime>(app: &AppHandle<R>, settings: &Settings) -> Result<()> {
    let shortcuts: Vec<Shortcut> = combos(settings)
        .into_iter()
        .filter_map(|(combo, _)| combo.parse::<Shortcut>().ok())
        .collect();

    let current = settings.clone();
    app.global_shortcut()
        .on_shortcuts(shortcuts, move |app, shortcut, event| {
            if event.state != ShortcutState::Pressed {
                return;
            }
            let action = {
                let state = app.state::<AppState>();
                let config = state.config.read();
                action_for_combo(&shortcut.to_string(), &config)
            };
            // 若状态中的配置尚未就绪，退回安装时的配置快照。
            let action = action.or_else(|| action_for_combo(&shortcut.to_string(), &current));
            if let Some(action) = action {
                let _ = app.emit("hotkey:triggered", serde_json::json!({ "action": action }));
            }
        })
        .map_err(|e| anyhow!("注册全局热键失败: {e}"))?;
    Ok(())
}

/// 兼容旧调用点：注册由 `install_handler` 完成，这里为空操作。
pub fn register_all<R: Runtime>(_app: &AppHandle<R>, _settings: &Settings) -> Result<()> {
    Ok(())
}

/// 注销所有已注册组合键（设置变更时先注销旧值）。
pub fn unregister_all<R: Runtime>(app: &AppHandle<R>) -> Result<()> {
    app.global_shortcut()
        .unregister_all()
        .map_err(|e| anyhow!("注销全局热键失败: {e}"))
}

/// 设置变更后刷新热键。
pub fn refresh<R: Runtime>(app: &AppHandle<R>, _old: &Settings, new: &Settings) -> Result<()> {
    unregister_all(app)?;
    install_handler(app, new)
}

/// 解析单个组合键（供命令/测试复用）。
pub fn parse_combo(combo: &str) -> Result<Shortcut> {
    combo
        .parse::<Shortcut>()
        .with_context(|| format!("无法解析组合键: {combo}"))
        .map_err(|e| anyhow!("{e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_and_map_all_default_combos() {
        let s = Settings::default();
        let expected = [
            ("Alt+A", HotkeyAction::Region),
            ("Alt+S", HotkeyAction::Full),
            ("Alt+W", HotkeyAction::Window),
            ("Alt+R", HotkeyAction::Record),
            ("Alt+P", HotkeyAction::Pause),
            ("Alt+C", HotkeyAction::Annotate),
        ];
        for (combo, want) in expected {
            let parsed = parse_combo(combo).unwrap();
            let got = action_for_combo(&parsed.to_string(), &s)
                .unwrap_or_else(|| panic!("{combo} -> {} 未映射", parsed));
            assert_eq!(got as i32, want as i32, "{combo}");
        }
    }

    #[test]
    fn plugin_key_prefix_is_normalized() {
        let s = Settings::default();
        // global-hotkey 把 Alt+A 序列化为 alt+KeyA。
        assert_eq!(
            action_for_combo("alt+KeyA", &s).map(|a| a as i32),
            Some(HotkeyAction::Region as i32)
        );
        assert_eq!(
            action_for_combo("ALT + KEYR", &s).map(|a| a as i32),
            Some(HotkeyAction::Record as i32)
        );
    }
}
