//! 应用设置（存在 settings 表的 "app" 键里，一个 JSON）

use crate::Ctx;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    /// system | light | dark
    pub theme: String,
    pub font_size: f32,
    /// compact | standard | cozy
    pub density: String,
    pub serif_titles: bool,
    pub autostart: bool,
    pub close_to_tray: bool,
    pub default_account: Option<i64>,
    /// important | all | none
    pub notify: String,
    /// 通知里显示主题和 AI 摘要；关掉只显示发件人
    pub notify_preview: bool,
    /// ask | always
    pub remote_images: String,
    pub undo_send_secs: u32,
    /// 设置页选的服务商（deepseek / qwen / kimi / openai / newapi / custom）；只用于界面显示，空=按地址猜
    pub ai_provider: String,
    pub ai_base: String,
    pub ai_model: String,
    pub ai_model_fine: String,
    pub ai_model_backup: String,
    pub ai_auto: bool,
    /// skip_bulk | all | important
    pub ai_scope: String,
    pub ai_daily_limit: i64,
    /// 界面语言：auto（跟随系统）| zh | en
    pub language: String,
    /// MCP 总开关。关着的时候连端口都不 bind，没有任何后台任务
    pub mcp_enabled: bool,
    /// 全局三轴：读（看邮件）/ 写（标记、移动、待办）/ 发（草稿与发信）
    pub mcp_read: bool,
    pub mcp_write: bool,
    pub mcp_send: bool,
    /// draft（只存草稿）| confirm（每次弹桌面确认）。没有无人值守直发这一档
    pub mcp_send_mode: String,
    /// 0＝随机端口
    pub mcp_port: u16,
    pub mcp_body_max_chars: i64,
    /// 是否允许 MCP 调用会烧 AI 额度的工具
    pub mcp_allow_ai: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            theme: "system".into(),
            font_size: 13.0,
            density: "standard".into(),
            serif_titles: true,
            autostart: false,
            close_to_tray: true,
            default_account: None,
            notify: "important".into(),
            notify_preview: true,
            remote_images: "ask".into(),
            undo_send_secs: 5,
            // 默认不接任何服务：用户在设置里选服务商、填密钥后才启用
            ai_provider: String::new(),
            ai_base: String::new(),
            ai_model: String::new(),
            ai_model_fine: String::new(),
            ai_model_backup: String::new(),
            ai_auto: true,
            ai_scope: "skip_bulk".into(),
            ai_daily_limit: 200,
            language: "auto".into(),
            mcp_enabled: false,
            mcp_read: true,
            mcp_write: false,
            mcp_send: false,
            mcp_send_mode: "draft".into(),
            // 测试实例（KOJOOMAIL_DATA）默认随机端口，别抢正式版的
            mcp_port: if std::env::var_os("KOJOOMAIL_DATA").is_some() { 0 } else { 47821 },
            mcp_body_max_chars: 8000,
            mcp_allow_ai: false,
        }
    }
}

pub fn load(ctx: &Ctx) -> Settings {
    if let Some(s) = ctx.settings_cache.lock().unwrap().clone() {
        return s;
    }
    let s: Settings = {
        let st = ctx.store.lock().unwrap();
        st.setting("app").and_then(|v| serde_json::from_str(&v).ok()).unwrap_or_default()
    };
    crate::i18n::apply(&s.language);
    *ctx.settings_cache.lock().unwrap() = Some(s.clone());
    s
}

pub fn save(ctx: &Ctx, s: &Settings) -> anyhow::Result<()> {
    let mut s = s.clone();
    clamp(&mut s);
    {
        let st = ctx.store.lock().unwrap();
        st.set_setting("app", &serde_json::to_string(&s)?)?;
    }
    crate::i18n::apply(&s.language);
    *ctx.settings_cache.lock().unwrap() = Some(s);
    Ok(())
}

fn clamp(s: &mut Settings) {
    s.font_size = s.font_size.clamp(12.0, 16.0);
    s.ai_daily_limit = s.ai_daily_limit.clamp(0, 5000);
    s.undo_send_secs = s.undo_send_secs.min(30);
    if !["auto", "zh", "en"].contains(&s.language.as_str()) {
        s.language = "auto".into();
    }
    s.mcp_body_max_chars = s.mcp_body_max_chars.clamp(200, 50000);
    if !["draft", "confirm"].contains(&s.mcp_send_mode.as_str()) {
        s.mcp_send_mode = "draft".into();
    }
    // 发信总开关关着时不留「每次确认」档，免得再打开时意外带上发信能力
    if !s.mcp_send {
        s.mcp_send_mode = "draft".into();
    }
}

/// 早期版本的默认接口写死在代码里、没存进设置。已经填过密钥的老用户：把当时的默认值存下来，升级后照常可用
pub fn keep_legacy_ai(ctx: &Ctx) {
    let (saved, accounts) = {
        let st = ctx.store.lock().unwrap();
        (st.setting("app"), st.conn.query_row("SELECT COUNT(*) FROM accounts", [], |r| r.get::<_, i64>(0)).unwrap_or(0))
    };
    // 全新安装（库里没账号）时凭据管理器里残留的旧密钥不算：让用户自己选服务商、看到确认
    if accounts == 0 || saved.as_deref().is_some_and(|v| v.contains("\"aiBase\"")) || crate::secret::get(crate::secret::AI_KEY).is_none() {
        return;
    }
    let mut s = load(ctx);
    s.ai_base = "https://s.lconai.com/v1".into();
    s.ai_model = "claude-haiku-5-5".into();
    s.ai_model_fine = "claude-opus-5-5".into();
    s.ai_model_backup = "deepseek-v4-flash".into();
    if let Err(e) = save(ctx, &s) {
        crate::applog::warn(&format!("保存旧版 AI 设置失败：{e}"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_send_mode_falls_back_to_draft() {
        for bad in ["", "auto", "send", "confirm ", "DRAFT"] {
            let mut s = Settings { mcp_send: true, mcp_send_mode: bad.into(), ..Default::default() };
            clamp(&mut s);
            assert_eq!(s.mcp_send_mode, "draft", "{bad:?}");
        }
    }

    #[test]
    fn confirm_survives_only_while_send_is_on() {
        let mut s = Settings { mcp_send: true, mcp_send_mode: "confirm".into(), ..Default::default() };
        clamp(&mut s);
        assert_eq!(s.mcp_send_mode, "confirm");
        s.mcp_send = false;
        clamp(&mut s);
        assert_eq!(s.mcp_send_mode, "draft");
    }

    #[test]
    fn body_limit_is_clamped() {
        for (raw, want) in [(0, 200), (199, 200), (8000, 8000), (50001, 50000), (-1, 200)] {
            let mut s = Settings { mcp_body_max_chars: raw, ..Default::default() };
            clamp(&mut s);
            assert_eq!(s.mcp_body_max_chars, want);
        }
    }

    /// 老配置里没有 mcp 字段，反序列化后必须是「关着、只读、只草稿」
    #[test]
    fn old_config_without_mcp_keys_gets_safe_defaults() {
        let s: Settings = serde_json::from_str(r#"{"theme":"dark","aiBase":"https://x/v1"}"#).unwrap();
        assert!(!s.mcp_enabled);
        assert!(s.mcp_read);
        assert!(!s.mcp_write);
        assert!(!s.mcp_send);
        assert_eq!(s.mcp_send_mode, "draft");
        assert!(!s.mcp_allow_ai);
    }
}
