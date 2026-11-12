//! MCP 的权限判定。
//!
//! 三层，缺一不可：全局总开关 → 全局轴 → 每账号轴。每次调用现算、不缓存，
//! 因为用户可能刚在设置页改过开关。
//!
//! 越权和「不存在」返回逐字相同的错误：返回「无权限」本身就泄露了这个账号有邮件。

use crate::{Ctx, settings};
use rusqlite::OptionalExtension;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Axis {
    Read,
    Write,
    Send,
}

/// 库里 accounts 一行的三个开关
#[derive(Clone, Copy, Debug)]
pub struct AccountFlags {
    pub read: bool,
    pub write: bool,
    pub send: bool,
}

impl Axis {
    fn of(self, a: AccountFlags) -> bool {
        match self {
            Axis::Read => a.read,
            Axis::Write => a.write,
            Axis::Send => a.send,
        }
    }
    fn global(self, s: &settings::Settings) -> bool {
        match self {
            Axis::Read => s.mcp_read,
            Axis::Write => s.mcp_write,
            Axis::Send => s.mcp_send,
        }
    }
    /// 三层与运算，缺一不可。纯函数，便于穷举
    pub fn permits(self, s: &settings::Settings, a: AccountFlags) -> bool {
        s.mcp_enabled && self.global(s) && self.of(a)
    }
}

/// `None` = 不限制（前端全部走这条）；`Some(ids)` = 只允许这些账号；`Some([])` = 全拒绝
#[derive(Clone, Debug, Default)]
pub struct Scope(pub Option<Vec<i64>>);

impl Scope {
    pub fn unrestricted() -> Scope {
        Scope(None)
    }

    pub fn get(ctx: &Ctx, axis: Axis) -> Scope {
        let s = settings::load(ctx);
        if !s.mcp_enabled || !axis.global(&s) {
            return Scope(Some(vec![]));
        }
        let st = ctx.store.lock().unwrap();
        let Ok(mut stmt) = st.conn.prepare("SELECT id, mcp_read, mcp_write, mcp_send FROM accounts") else {
            return Scope(Some(vec![]));
        };
        let ids = stmt
            .query_map([], |r| {
                Ok((r.get::<_, i64>(0)?, AccountFlags { read: r.get(1)?, write: r.get(2)?, send: r.get(3)? }))
            })
            .map(|it| it.filter_map(|x| x.ok()).filter(|(_, a)| axis.permits(&s, *a)).map(|(id, _)| id).collect())
            .unwrap_or_default();
        Scope(Some(ids))
    }

    pub fn read(ctx: &Ctx) -> Scope {
        Scope::get(ctx, Axis::Read)
    }

    pub fn allows(&self, account_id: i64) -> bool {
        match &self.0 {
            None => true,
            Some(ids) => ids.contains(&account_id),
        }
    }

    /// 拼进 WHERE 的片段和参数。`col` 由调用方写死，不接受外部输入。
    /// 空名单给恒假条件，让 total 直接是 0，和「这个账号没有邮件」不可区分。
    pub fn frag(&self, col: &str) -> (String, Option<rusqlite::types::Value>) {
        match &self.0 {
            None => (String::new(), None),
            Some(ids) if ids.is_empty() => ("0".into(), None),
            Some(ids) => (
                format!("{col} IN (SELECT value FROM json_each(?))"),
                Some(rusqlite::types::Value::Text(serde_json::to_string(ids).unwrap_or_else(|_| "[]".into()))),
            ),
        }
    }

    /// 同样条件的字面量形式，给 assist 里那些直接拼 SQL、没有参数位的查询用。
    /// 账号 id 是库里的 i64，拼成十进制字面量，不可能带进注入。
    pub fn pred(&self, col: &str) -> String {
        match &self.0 {
            None => "1".into(),
            Some(ids) if ids.is_empty() => "0".into(),
            Some(ids) => format!("{col} IN ({})", ids.iter().map(i64::to_string).collect::<Vec<_>>().join(",")),
        }
    }
}

/// 越权和不存在用同一句话
pub fn gone() -> String {
    crate::i18n::t("这封邮件已经不在了", "This email no longer exists").to_string()
}

/// 判权限必须在拉正文之前：否则会替不可读的账号触发 IMAP 下载
pub fn check(ctx: &Ctx, msg_id: i64, axis: Axis) -> Result<i64, String> {
    let scope = Scope::get(ctx, axis);
    let account = {
        let st = ctx.store.lock().unwrap();
        st.conn
            .query_row("SELECT account_id FROM messages WHERE id = ?", [msg_id], |r| r.get::<_, i64>(0))
            .optional()
            .map_err(|e| e.to_string())?
    };
    match account {
        Some(a) if scope.allows(a) => Ok(a),
        _ => Err(gone()),
    }
}

pub fn readable(ctx: &Ctx, msg_id: i64) -> Result<i64, String> {
    check(ctx, msg_id, Axis::Read)
}

/// 附件文件名加固：去掉路径分隔符和控制字符，收尾的点和空格也去掉
/// （Windows 会静默丢掉它们，`a.` 和 `a` 会撞名）。落点必须还在下载目录里。
pub fn safe_file_name(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| if r#"\/:*?"<>|"#.contains(c) || c.is_control() { '_' } else { c })
        .collect();
    let cleaned = cleaned.trim().trim_end_matches(['.', ' ']).trim();
    let short: String = cleaned.chars().take(120).collect();
    if short.is_empty() || short == "." || short == ".." { "attachment".into() } else { short }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_names_cannot_escape_the_download_folder() {
        for raw in ["../../evil.exe", "..", ".", "a.", "  ", "x\u{0}y.exe", "a/b\\c:d*e?f\"g<h>i|j", "..."] {
            let n = safe_file_name(raw);
            assert!(!n.is_empty(), "{raw:?}");
            assert!(!n.contains(['/', '\\', ':']), "{raw:?} -> {n:?}");
            assert!(!n.chars().any(|c| c.is_control()), "{raw:?} -> {n:?}");
            assert_ne!(n, ".");
            assert_ne!(n, "..");
            let p = std::path::Path::new(&n);
            assert_eq!(p.file_name().map(|s| s.to_string_lossy().into_owned()), Some(n.clone()), "{raw:?}");
        }
        assert_eq!(safe_file_name("report.pdf"), "report.pdf");
    }

    #[test]
    fn empty_scope_rejects_everything() {
        let none = Scope(Some(vec![]));
        assert!(!none.allows(1));
        assert_eq!(none.frag("m.account_id").0, "0");
        assert!(none.frag("m.account_id").1.is_none());
    }

    #[test]
    fn unrestricted_scope_adds_no_condition() {
        let all = Scope::unrestricted();
        assert!(all.allows(42));
        let (sql, arg) = all.frag("m.account_id");
        assert!(sql.is_empty());
        assert!(arg.is_none());
    }

    #[test]
    fn scoped_fragment_is_one_bound_parameter() {
        let s = Scope(Some(vec![1, 2]));
        assert!(s.allows(1));
        assert!(!s.allows(3));
        let (sql, arg) = s.frag("m.account_id");
        assert_eq!(sql, "m.account_id IN (SELECT value FROM json_each(?))");
        assert_eq!(arg, Some(rusqlite::types::Value::Text("[1,2]".into())));
    }

    /// 三层与运算的全部 2(总开关) × 2(全局轴) × 2(账号轴) × 3(轴) = 24 种组合
    #[test]
    fn permission_matrix_needs_all_three_layers() {
        for enabled in [false, true] {
            for global in [false, true] {
                for per_account in [false, true] {
                    for axis in [Axis::Read, Axis::Write, Axis::Send] {
                        let mut s = settings::Settings::default();
                        s.mcp_enabled = enabled;
                        s.mcp_read = global;
                        s.mcp_write = global;
                        s.mcp_send = global;
                        let a = AccountFlags { read: per_account, write: per_account, send: per_account };
                        assert_eq!(
                            axis.permits(&s, a),
                            enabled && global && per_account,
                            "{axis:?} enabled={enabled} global={global} account={per_account}"
                        );
                    }
                }
            }
        }
    }

    /// 轴之间互不牵连：开了读不等于开了写或发
    #[test]
    fn axes_are_independent() {
        let mut s = settings::Settings::default();
        s.mcp_enabled = true;
        s.mcp_read = true;
        let a = AccountFlags { read: true, write: true, send: true };
        assert!(Axis::Read.permits(&s, a));
        assert!(!Axis::Write.permits(&s, a));
        assert!(!Axis::Send.permits(&s, a));
    }

    /// 默认配置：MCP 关着；就算打开，写和发也还得逐账号再开一次
    #[test]
    fn defaults_are_read_only_and_off() {
        let mut s = settings::Settings::default();
        let a = AccountFlags { read: true, write: false, send: false };
        assert!(!Axis::Read.permits(&s, a));
        s.mcp_enabled = true;
        assert!(Axis::Read.permits(&s, a));
        assert!(!Axis::Write.permits(&s, AccountFlags { read: true, write: true, send: true }));
        assert!(!Axis::Send.permits(&s, AccountFlags { read: true, write: true, send: true }));
    }
}