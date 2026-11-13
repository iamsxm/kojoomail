//! 自动更新（tauri-plugin-updater）：启动 30 秒后查一次，之后每天一次。
//! 有新版只发 update-available 事件，用户点了才下载安装。fork 更新包由 Actions Secret 中的私钥签名，公钥在 tauri.conf.json，
//! 检查更新的地址和公钥在 tauri.conf.json 的 plugins.updater

use serde::Serialize;
use std::time::Duration;
use tauri::{AppHandle, Emitter};
use tauri_plugin_updater::UpdaterExt;

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct UpdateInfo {
    version: String,
    notes: String,
}

async fn check(app: &AppHandle) -> Result<Option<UpdateInfo>, String> {
    let u = app.updater().map_err(|e| e.to_string())?.check().await.map_err(|e| tr!("检查更新失败：{e}", "Couldn't check for updates: {e}"))?;
    Ok(u.map(|u| UpdateInfo { version: u.version.clone(), notes: u.body.clone().unwrap_or_default() }))
}

/// 后台定时检查（调试版和测试实例不查）
pub fn spawn(app: AppHandle) {
    if cfg!(debug_assertions) || std::env::var_os("KOJOOMAIL_DATA").is_some_and(|v| !v.is_empty()) {
        return;
    }
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(Duration::from_secs(30)).await;
        loop {
            match check(&app).await {
                Ok(Some(i)) => {
                    crate::applog::info(&format!("发现新版本 {}", i.version));
                    let _ = app.emit("update-available", i);
                }
                Ok(None) => {}
                Err(e) => crate::applog::warn(&e),
            }
            tokio::time::sleep(Duration::from_secs(24 * 3600)).await;
        }
    });
}

#[tauri::command]
pub async fn check_update(app: AppHandle) -> Result<Option<UpdateInfo>, String> {
    check(&app).await
}

/// 下载、验签、运行安装包（安装时程序会退出），装完重启
#[tauri::command]
pub async fn install_update(app: AppHandle) -> Result<(), String> {
    let u = app.updater().map_err(|e| e.to_string())?.check().await.map_err(|e| tr!("检查更新失败：{e}", "Couldn't check for updates: {e}"))?.ok_or(crate::i18n::t("已经是最新版", "You're on the latest version"))?;
    crate::applog::info(&format!("开始更新到 {}", u.version));
    u.download_and_install(|_, _| {}, || {}).await.map_err(|e| {
        crate::applog::warn(&tr!("更新失败：{e}", "Update failed: {e}"));
        tr!("更新失败：{e}", "Update failed: {e}")
    })?;
    app.restart();
}
