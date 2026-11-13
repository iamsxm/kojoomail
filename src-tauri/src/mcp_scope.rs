//! MCP 的权限判定。
//!
//! 三层，缺一不可：全局总开关 → 全局轴 → 每账号轴。每次调用现算、不缓存，
//! 因为用户可能刚在设置页改过开关。
//!
//! 越权和「不存在」返回逐字相同的错误：返回「无权限」本身就泄露了这个账号有邮件。

use crate::store::Store;
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

    pub fn write(ctx: &Ctx) -> Scope {
        Scope::get(ctx, Axis::Write)
    }

    pub fn send(ctx: &Ctx) -> Scope {
        Scope::get(ctx, Axis::Send)
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
    let st = ctx.store.lock().unwrap();
    check_in(&st, msg_id, &scope)
}

pub(crate) fn check_in(st: &Store, msg_id: i64, scope: &Scope) -> Result<i64, String> {
    let account = st
        .conn
        .query_row("SELECT account_id FROM messages WHERE id = ?", [msg_id], |r| r.get::<_, i64>(0))
        .optional()
        .map_err(|e| e.to_string())?;
    match account {
        Some(a) if scope.allows(a) => Ok(a),
        _ => Err(gone()),
    }
}

pub fn readable(ctx: &Ctx, msg_id: i64) -> Result<i64, String> {
    check(ctx, msg_id, Axis::Read)
}

pub fn writable(ctx: &Ctx, msg_id: i64) -> Result<i64, String> {
    check(ctx, msg_id, Axis::Write)
}

/// 批量操作：全部 id 都存在且都过权限才执行，否则整批拒绝。
/// 不做部分执行——「改了 3 封、另外 2 封没权限」会让 agent 以为整批都成功了。
pub fn check_all(ctx: &Ctx, ids: &[i64], axis: Axis) -> Result<Vec<i64>, String> {
    let scope = Scope::get(ctx, axis);
    let st = ctx.store.lock().unwrap();
    check_all_in(&st, ids, &scope)
}

pub(crate) fn check_all_in(st: &Store, ids: &[i64], scope: &Scope) -> Result<Vec<i64>, String> {
    if ids.is_empty() {
        return Err(gone());
    }
    let json = serde_json::to_string(ids).map_err(|e| e.to_string())?;
    let mut q = st
        .conn
        .prepare("SELECT id, account_id FROM messages WHERE id IN (SELECT value FROM json_each(?))")
        .map_err(|e| e.to_string())?;
    let rows: Vec<(i64, i64)> = q
        .query_map([&json], |r| Ok((r.get(0)?, r.get(1)?)))
        .map_err(|e| e.to_string())?
        .filter_map(|x| x.ok())
        .collect();
    let found: std::collections::HashSet<i64> = rows.iter().map(|(id, _)| *id).collect();
    if ids.iter().any(|id| !found.contains(id)) || rows.iter().any(|(_, acc)| !scope.allows(*acc)) {
        return Err(gone());
    }
    Ok(ids.to_vec())
}

/// 待办：自己的账号要可写，来源邮件所属账号也要可写。
/// 账号为空的待办不在任何账号的授权范围里，MCP 看不见也改不了（和读路径一致）。
pub fn todo_writable(ctx: &Ctx, todo_id: i64) -> Result<(), String> {
    let scope = Scope::write(ctx);
    let st = ctx.store.lock().unwrap();
    todo_writable_in(&st, todo_id, &scope)
}

pub(crate) fn todo_writable_in(st: &Store, todo_id: i64, scope: &Scope) -> Result<(), String> {
    let row: Option<(Option<i64>, Option<i64>)> = st
        .conn
        .query_row("SELECT account_id, msg_id FROM todos WHERE id=?", [todo_id], |r| Ok((r.get(0)?, r.get(1)?)))
        .optional()
        .map_err(|e| e.to_string())?;
    let Some((acc, msg)) = row else { return Err(gone()) };
    let ok = acc.is_some_and(|a| scope.allows(a))
        && match msg {
            None => true,
            Some(m) => st
                .conn
                .query_row("SELECT account_id FROM messages WHERE id=?", [m], |r| r.get::<_, i64>(0))
                .optional()
                .map_err(|e| e.to_string())?
                .is_some_and(|a| scope.allows(a)),
        };
    if ok { Ok(()) } else { Err(gone()) }
}

/// 改草稿：草稿现在属于哪个账号，就得是哪个可发的账号。
/// 只判目标账号不够——`UPDATE drafts SET account_id=? WHERE id=?` 会连同归属一起改掉，
/// 借一个别账号的草稿 id 就能把那封草稿搬到自己名下。
pub fn draft_sendable(ctx: &Ctx, draft_id: i64) -> Result<(), String> {
    let scope = Scope::send(ctx);
    let st = ctx.store.lock().unwrap();
    draft_sendable_in(&st, draft_id, &scope)
}

pub(crate) fn draft_sendable_in(st: &Store, draft_id: i64, scope: &Scope) -> Result<(), String> {
    let acc = st
        .conn
        .query_row("SELECT account_id FROM drafts WHERE id=?", [draft_id], |r| r.get::<_, Option<i64>>(0))
        .optional()
        .map_err(|e| e.to_string())?;
    match acc {
        Some(Some(a)) if scope.allows(a) => Ok(()),
        _ => Err(gone()),
    }
}

/// 账号本身是否可写 / 可发（新建草稿、发新信时没有原邮件可查）
pub fn account_permits(ctx: &Ctx, account_id: i64, axis: Axis) -> bool {
    Scope::get(ctx, axis).allows(account_id)
}

pub fn account_email(ctx: &Ctx, account_id: i64) -> String {
    ctx.store
        .lock()
        .unwrap()
        .conn
        .query_row("SELECT email FROM accounts WHERE id=?", [account_id], |r| r.get::<_, String>(0))
        .unwrap_or_default()
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

    /// 两个账号各 2 封；账号 2 不在授权范围里
    fn two_accounts(name: &str) -> (std::path::PathBuf, Store) {
        let d = std::env::temp_dir().join(format!("kjm-scope-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        let st = Store::open(&d).unwrap();
        for a in [1, 2] {
            st.conn
                .execute(
                    "INSERT INTO accounts (id, email, username, imap_host, imap_port, smtp_host, smtp_port, created) VALUES (?1, ?2, ?2, 'h', 993, 'h', 465, 0)",
                    rusqlite::params![a, format!("a{a}@example.com")],
                )
                .unwrap();
            st.conn
                .execute("INSERT INTO folders (id, account_id, path, name, role) VALUES (?1, ?1, 'INBOX', 'Inbox', 'inbox')", [a])
                .unwrap();
            for n in 0..2 {
                st.conn
                    .execute(
                        "INSERT INTO messages (id, account_id, folder_id, uid, subject, date) VALUES (?1, ?2, ?2, ?3, ?4, 0)",
                        rusqlite::params![a * 10 + n, a, n + 1, format!("m{a}-{n}")],
                    )
                    .unwrap();
            }
        }
        (d, st)
    }

    /// 批量写要么全过要么全不动：混进一个不可写账号，整批拒绝，可写的那封也不许改
    #[test]
    fn a_batch_with_one_foreign_id_is_rejected_whole() {
        let (d, st) = two_accounts("batch");
        let scope = Scope(Some(vec![1]));
        assert_eq!(check_all_in(&st, &[10, 11], &scope).unwrap(), vec![10, 11]);
        for ids in [vec![10, 20], vec![20, 10], vec![20]] {
            assert_eq!(check_all_in(&st, &ids, &scope).unwrap_err(), gone(), "{ids:?}");
        }
        drop(st);
        let _ = std::fs::remove_dir_all(&d);
    }

    /// 不存在的 id 和越权的 id 说同一句话，且都让整批失败
    #[test]
    fn a_missing_id_rejects_the_batch_with_the_same_words() {
        let (d, st) = two_accounts("missing");
        let scope = Scope(Some(vec![1]));
        let missing = check_all_in(&st, &[10, 999999], &scope).unwrap_err();
        let foreign = check_all_in(&st, &[10, 20], &scope).unwrap_err();
        assert_eq!(missing, foreign);
        assert_eq!(check_all_in(&st, &[], &scope).unwrap_err(), gone());
        drop(st);
        let _ = std::fs::remove_dir_all(&d);
    }

    /// 单封的越权读和「查无此信」必须逐字相同
    #[test]
    fn reading_a_foreign_message_says_the_same_as_a_missing_one() {
        let (d, st) = two_accounts("gone");
        let scope = Scope(Some(vec![1]));
        assert_eq!(check_in(&st, 10, &scope).unwrap(), 1);
        assert_eq!(check_in(&st, 20, &scope).unwrap_err(), check_in(&st, 999999, &scope).unwrap_err());
        drop(st);
        let _ = std::fs::remove_dir_all(&d);
    }

    /// 改草稿会连 account_id 一起改掉，所以归属按「草稿现在属于谁」判，不是按目标账号判。
    /// 账号为空的草稿和待办一样不在任何授权范围里。
    #[test]
    fn a_draft_can_only_be_rewritten_by_its_own_sendable_account() {
        let (d, st) = two_accounts("draft");
        let scope = Scope(Some(vec![1]));
        for (id, acc) in [(1, Some(1)), (2, Some(2)), (3, None)] {
            st.conn
                .execute("INSERT INTO drafts (id, account_id, subject, updated) VALUES (?1, ?2, 's', 0)", rusqlite::params![id, acc])
                .unwrap();
        }
        assert!(draft_sendable_in(&st, 1, &scope).is_ok());
        for id in [2, 3, 999] {
            assert_eq!(draft_sendable_in(&st, id, &scope).unwrap_err(), gone(), "draft {id}");
        }
        assert_eq!(draft_sendable_in(&st, 1, &Scope(Some(vec![]))).unwrap_err(), gone());
        drop(st);
        let _ = std::fs::remove_dir_all(&d);
    }

    /// 待办的账号为空、或来源邮件属于别的账号，都不许改
    #[test]
    fn todos_are_scoped_by_both_their_own_account_and_their_source() {
        let (d, st) = two_accounts("todo");
        let scope = Scope(Some(vec![1]));
        let todo = |id: i64, acc: Option<i64>, msg: Option<i64>| {
            st.conn
                .execute("INSERT INTO todos (id, account_id, msg_id, text, created) VALUES (?1, ?2, ?3, 't', 0)", rusqlite::params![id, acc, msg])
                .unwrap();
        };
        todo(1, Some(1), None);
        todo(2, Some(1), Some(10));
        todo(3, Some(1), Some(20));
        todo(4, Some(2), None);
        todo(5, None, Some(10));
        todo(6, None, None);
        assert!(todo_writable_in(&st, 1, &scope).is_ok());
        assert!(todo_writable_in(&st, 2, &scope).is_ok());
        for id in [3, 4, 5, 6, 999] {
            assert_eq!(todo_writable_in(&st, id, &scope).unwrap_err(), gone(), "todo {id}");
        }
        drop(st);
        let _ = std::fs::remove_dir_all(&d);
    }
}