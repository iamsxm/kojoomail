//! 本地数据库（SQLite，单文件）。所有邮件元数据、正文缓存、AI 结果、待办都在这里。
//! 原始邮件（含附件）另存为 .eml 文件，数据库只放文本，保持库小、查询快。

use anyhow::Result;
use rusqlite::{params, Connection, OptionalExtension};
use std::path::{Path, PathBuf};

pub struct Store {
    pub conn: Connection,
    pub dir: PathBuf,
}

const SCHEMA: &str = r#"
PRAGMA journal_mode = WAL;
PRAGMA synchronous = NORMAL;
PRAGMA foreign_keys = ON;
CREATE TABLE IF NOT EXISTS accounts (
  id INTEGER PRIMARY KEY,
  email TEXT NOT NULL UNIQUE,
  display_name TEXT NOT NULL DEFAULT '',
  label TEXT NOT NULL DEFAULT '',
  color TEXT NOT NULL DEFAULT '#1B4A80',
  provider TEXT NOT NULL DEFAULT 'imap',
  username TEXT NOT NULL,
  imap_host TEXT NOT NULL, imap_port INTEGER NOT NULL, imap_tls INTEGER NOT NULL DEFAULT 1,
  smtp_host TEXT NOT NULL, smtp_port INTEGER NOT NULL, smtp_tls INTEGER NOT NULL DEFAULT 1,
  save_sent INTEGER NOT NULL DEFAULT 1,
  ai_allowed INTEGER NOT NULL DEFAULT 1,
  signature TEXT NOT NULL DEFAULT '',
  sort INTEGER NOT NULL DEFAULT 0,
  created INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS folders (
  id INTEGER PRIMARY KEY,
  account_id INTEGER NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
  path TEXT NOT NULL,
  name TEXT NOT NULL,
  role TEXT NOT NULL DEFAULT '',
  uidvalidity INTEGER NOT NULL DEFAULT 0,
  unread INTEGER NOT NULL DEFAULT 0,
  total INTEGER NOT NULL DEFAULT 0,
  last_sync INTEGER NOT NULL DEFAULT 0,
  UNIQUE(account_id, path)
);
CREATE TABLE IF NOT EXISTS messages (
  id INTEGER PRIMARY KEY,
  account_id INTEGER NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
  folder_id INTEGER NOT NULL REFERENCES folders(id) ON DELETE CASCADE,
  uid INTEGER NOT NULL,
  msgid TEXT NOT NULL DEFAULT '',
  in_reply_to TEXT NOT NULL DEFAULT '',
  refs TEXT NOT NULL DEFAULT '',
  from_name TEXT NOT NULL DEFAULT '',
  from_addr TEXT NOT NULL DEFAULT '',
  to_json TEXT NOT NULL DEFAULT '[]',
  cc_json TEXT NOT NULL DEFAULT '[]',
  reply_to TEXT NOT NULL DEFAULT '',
  subject TEXT NOT NULL DEFAULT '',
  date INTEGER NOT NULL DEFAULT 0,
  seen INTEGER NOT NULL DEFAULT 0,
  flagged INTEGER NOT NULL DEFAULT 0,
  answered INTEGER NOT NULL DEFAULT 0,
  has_att INTEGER NOT NULL DEFAULT 0,
  bulk INTEGER NOT NULL DEFAULT 0,
  size INTEGER NOT NULL DEFAULT 0,
  snippet TEXT NOT NULL DEFAULT '',
  has_body INTEGER NOT NULL DEFAULT 0,
  body_text TEXT NOT NULL DEFAULT '',
  body_html TEXT NOT NULL DEFAULT '',
  att_json TEXT NOT NULL DEFAULT '[]',
  later_until INTEGER,
  moving INTEGER NOT NULL DEFAULT 0,
  UNIQUE(folder_id, uid)
);
CREATE INDEX IF NOT EXISTS idx_msg_folder_date ON messages(folder_id, date DESC);
CREATE INDEX IF NOT EXISTS idx_msg_acc_date ON messages(account_id, date DESC);
CREATE INDEX IF NOT EXISTS idx_msg_flagged ON messages(flagged) WHERE flagged = 1;
CREATE VIRTUAL TABLE IF NOT EXISTS msg_fts USING fts5(subject, people, body, content='', contentless_delete=1, tokenize='trigram');
CREATE TABLE IF NOT EXISTS ai (
  msg_id INTEGER PRIMARY KEY REFERENCES messages(id) ON DELETE CASCADE,
  state INTEGER NOT NULL DEFAULT 0,
  summary TEXT NOT NULL DEFAULT '',
  category TEXT NOT NULL DEFAULT '',
  priority TEXT NOT NULL DEFAULT '',
  reason TEXT NOT NULL DEFAULT '',
  facts_json TEXT NOT NULL DEFAULT '[]',
  reply TEXT NOT NULL DEFAULT '',
  reply_lang TEXT NOT NULL DEFAULT '',
  model TEXT NOT NULL DEFAULT '',
  error TEXT NOT NULL DEFAULT '',
  created INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE IF NOT EXISTS todos (
  id INTEGER PRIMARY KEY,
  msg_id INTEGER REFERENCES messages(id) ON DELETE SET NULL,
  account_id INTEGER,
  text TEXT NOT NULL,
  due INTEGER,
  priority TEXT NOT NULL DEFAULT 'normal',
  done INTEGER NOT NULL DEFAULT 0,
  done_at INTEGER,
  source TEXT NOT NULL DEFAULT 'manual',
  src_subject TEXT NOT NULL DEFAULT '',
  src_from TEXT NOT NULL DEFAULT '',
  created INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS drafts (
  id INTEGER PRIMARY KEY,
  account_id INTEGER,
  to_json TEXT NOT NULL DEFAULT '[]',
  cc_json TEXT NOT NULL DEFAULT '[]',
  bcc_json TEXT NOT NULL DEFAULT '[]',
  subject TEXT NOT NULL DEFAULT '',
  body TEXT NOT NULL DEFAULT '',
  reply_msg INTEGER,
  mode TEXT NOT NULL DEFAULT 'new',
  updated INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS settings (key TEXT PRIMARY KEY, value TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS ai_usage (day TEXT PRIMARY KEY, count INTEGER NOT NULL DEFAULT 0);
"#;

/// 数据库升级步骤：第 i 个（从 0 数）把数据版本从 i+1 升到 i+2。
/// 规则：只能在末尾追加；发出去的步骤不能再改（用户的库已经按它升级过了）；步骤里不要提交事务
type Step = fn(&rusqlite::Transaction) -> rusqlite::Result<()>;
/// 数据版本 2：邮件译文缓存。一行一段：src 是原文、t 是译文，按（邮件, 语言, 原文）逐条存，翻完一批写一批
fn v2_translations(tx: &rusqlite::Transaction) -> rusqlite::Result<()> {
    tx.execute_batch(
        "CREATE TABLE IF NOT EXISTS translations (\n\
           msg_id INTEGER NOT NULL REFERENCES messages(id) ON DELETE CASCADE,\n\
           lang TEXT NOT NULL,\n\
           src TEXT NOT NULL,\n\
           t TEXT NOT NULL,\n\
           model TEXT NOT NULL DEFAULT '',\n\
           created INTEGER NOT NULL,\n\
           PRIMARY KEY (msg_id, lang, src)\n\
         );",
    )
}
/// 数据版本 3：草稿分别保存 Markdown 正文、签名和原文引用；旧草稿的 body 保持可读。
fn v3_draft_content(tx: &rusqlite::Transaction) -> rusqlite::Result<()> {
    tx.execute_batch("ALTER TABLE drafts ADD COLUMN content_json TEXT;")
}
const MIGRATIONS: &[Step] = &[v2_translations, v3_draft_content];
/// 当前代码对应的数据版本
const LATEST: i64 = MIGRATIONS.len() as i64 + 1;

/// 表里没有这一列才加
fn add_column(conn: &Connection, table: &str, col: &str, def: &str) -> Result<()> {
    let has = conn.prepare(&format!("SELECT 1 FROM pragma_table_info('{table}') WHERE name=?"))?.exists([col])?;
    if !has {
        conn.execute(&format!("ALTER TABLE {table} ADD COLUMN {col} {def}"), [])?;
    }
    Ok(())
}

impl Store {
    pub fn open(dir: &Path) -> Result<Store> {
        let denied = |e: &dyn std::fmt::Display| anyhow::anyhow!(tr!("打不开数据目录 {}：{e}。可能是没有写入权限，或文件被杀毒软件占用。", "Can't open the data folder {}: {e}. It may not be writable, or antivirus software may be locking the files.", dir.display()));
        std::fs::create_dir_all(dir.join("mail")).map_err(|e| denied(&e))?;
        let conn = Connection::open(dir.join("kojoomail.db")).map_err(|e| denied(&e))?;
        let ver: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0)).map_err(|e| denied(&e))?;
        if ver > LATEST {
            anyhow::bail!(tr!("这份数据是更新版本的 Kojoo Mail 建的（数据版本 {ver}），请安装最新版再打开", "This data was created by a newer version of Kojoo Mail (data version {ver}). Please install the latest version."));
        }
        conn.execute_batch(SCHEMA)?;
        // 早期版本（数据版本 0、1）陆续加的列：没有才加。加列失败要报出来，不然之后到处「no such column」
        add_column(&conn, "messages", "orig_folder", "INTEGER")?;
        add_column(&conn, "messages", "orig_uid", "INTEGER")?;
        add_column(&conn, "drafts", "att_json", "TEXT NOT NULL DEFAULT '[]'")?;
        // 登录方式：password（密码/授权码）或 oauth（一键登录，凭据管理器里存的是长期授权）
        add_column(&conn, "accounts", "auth", "TEXT NOT NULL DEFAULT 'password'")?;
        // 列表上方的筛选页签（JSON 数组，AI 分类名）；AI 处理程度 full（完整批注）/ classify（只分类）
        add_column(&conn, "accounts", "focus", "TEXT NOT NULL DEFAULT '[\"待办\"]'")?;
        add_column(&conn, "accounts", "ai_level", "TEXT NOT NULL DEFAULT 'full'")?;
        // 待跟进：用户点了「不用跟进」的已发送邮件；两个索引给「对方回了没有」查询用
        add_column(&conn, "messages", "followup_off", "INTEGER NOT NULL DEFAULT 0")?;
        // MCP 每账号授权：读默认开（开 MCP 就是为了读），写和发必须逐个账号显式打开
        add_column(&conn, "accounts", "mcp_read", "INTEGER NOT NULL DEFAULT 1")?;
        add_column(&conn, "accounts", "mcp_write", "INTEGER NOT NULL DEFAULT 0")?;
        add_column(&conn, "accounts", "mcp_send", "INTEGER NOT NULL DEFAULT 0")?;
        conn.execute("CREATE INDEX IF NOT EXISTS idx_msg_irt ON messages(in_reply_to) WHERE in_reply_to != ''", [])?;
        conn.execute("CREATE INDEX IF NOT EXISTS idx_msg_acc_from ON messages(account_id, lower(from_addr), date)", [])?;
        conn.execute("CREATE INDEX IF NOT EXISTS idx_msg_placeholder ON messages(folder_id, msgid) WHERE uid < 0", [])?;
        let mut st = Store { conn, dir: dir.to_path_buf() };
        if ver < 1 {
            // 一次性：区分订阅和通知之前收的邮件，用存着的原文重新判断（没下载原文的保持「通知」）
            st.reclassify_bulk();
            st.conn.execute_batch("PRAGMA user_version = 1")?;
        }
        // 全新的库（版本 0）没东西可备份
        st.migrate(ver.max(1), MIGRATIONS, ver > 0)?;
        Ok(st)
    }

    /// 数据版本 1 之后的升级：按顺序执行 MIGRATIONS，每步一个事务，版本号和改动一起提交。
    /// 动手前先把整个库快照成 kojoomail.db.bak-v{旧版本}，升级出错时能找回；只留最近一份。
    /// 升级期间关掉外键：重建表（建新表、拷数据、删旧表、改名）时 DROP 会触发级联删除，把 AI 批注、待办关联清掉
    fn migrate(&mut self, from: i64, steps: &[Step], backup: bool) -> Result<()> {
        if from >= steps.len() as i64 + 1 {
            return Ok(());
        }
        let bak = self.dir.join(format!("kojoomail.db.bak-v{from}"));
        if backup {
            let _ = std::fs::remove_file(&bak);
            crate::applog::info(&format!("数据升级前备份到 {}", bak.display()));
            self.conn.execute("VACUUM INTO ?", [bak.to_string_lossy()])?;
        }
        self.conn.execute_batch("PRAGMA foreign_keys = OFF")?;
        let r = self.run_steps(from, steps, &bak);
        self.conn.execute_batch("PRAGMA foreign_keys = ON")?;
        r?;
        // 升级成功：更早的备份没用了（每份都和整个库一样大）
        if let Ok(rd) = std::fs::read_dir(&self.dir) {
            for f in rd.flatten() {
                let n = f.file_name().to_string_lossy().into_owned();
                if n.starts_with("kojoomail.db.bak-v") && f.path() != bak {
                    let _ = std::fs::remove_file(f.path());
                }
            }
        }
        Ok(())
    }

    fn run_steps(&mut self, from: i64, steps: &[Step], bak: &Path) -> Result<()> {
        for (i, step) in steps.iter().enumerate().skip((from - 1) as usize) {
            let to = i as i64 + 2;
            let fail = |e: &dyn std::fmt::Display| anyhow::anyhow!(tr!("数据升级到版本 {to} 失败：{e}。升级前的备份在 {}，请把「设置 → 关于 → 导出诊断信息」的文件发给开发者", "Upgrading the data to version {to} failed: {e}. A backup from before the upgrade is at {}. Please send the file from Settings → About → Export diagnostics to the developer.", bak.display()));
            let tx = self.conn.transaction()?;
            step(&tx).map_err(|e| fail(&e))?;
            // 关了外键自动检查，提交前手动查一遍有没有断掉的关联
            let broken: i64 = tx.query_row("SELECT COUNT(*) FROM pragma_foreign_key_check", [], |r| r.get(0))?;
            if broken > 0 {
                return Err(fail(&tr!("{broken} 处关联对不上", "{broken} broken references")));
            }
            tx.execute_batch(&format!("PRAGMA user_version = {to}"))?;
            tx.commit()?;
            crate::applog::info(&format!("数据已升级到版本 {to}"));
        }
        Ok(())
    }

    fn reclassify_bulk(&self) {
        let ids: Vec<i64> = match self.conn.prepare("SELECT id FROM messages WHERE bulk > 0 AND has_body = 1") {
            Ok(mut q) => q.query_map([], |r| r.get(0)).map(|it| it.filter_map(|x| x.ok()).collect()).unwrap_or_default(),
            Err(_) => return,
        };
        for id in ids {
            let Ok(raw) = std::fs::read(self.eml_path(id)) else { continue };
            let h = crate::mime::parse_head(&raw);
            let kind = if h.subscription { 2 } else { 1 };
            let _ = self.conn.execute("UPDATE messages SET bulk=? WHERE id=?", params![kind, id]);
            let _ = self.conn.execute("UPDATE ai SET category=? WHERE msg_id=? AND state=3", params![if kind == 2 { "订阅" } else { "通知" }, id]);
        }
    }

    pub fn eml_path(&self, msg_id: i64) -> PathBuf {
        self.dir.join("mail").join(format!("{msg_id}.eml"))
    }

    pub fn setting(&self, key: &str) -> Option<String> {
        self.conn
            .query_row("SELECT value FROM settings WHERE key=?", [key], |r| r.get(0))
            .optional()
            .ok()
            .flatten()
    }

    pub fn set_setting(&self, key: &str, value: &str) -> Result<()> {
        self.conn.execute(
            "INSERT INTO settings(key,value) VALUES(?,?) ON CONFLICT(key) DO UPDATE SET value=excluded.value",
            params![key, value],
        )?;
        Ok(())
    }

    /// 重新统计各文件夹的未读数和总数（只统计本地已同步的部分）
    pub fn refresh_counts(&self, account_id: i64) -> Result<()> {
        self.conn.execute(
            "UPDATE folders SET
               unread = (SELECT COUNT(*) FROM messages m WHERE m.folder_id = folders.id AND m.seen = 0 AND m.moving = 0),
               total = (SELECT COUNT(*) FROM messages m WHERE m.folder_id = folders.id AND m.moving = 0)
             WHERE account_id = ?",
            [account_id],
        )?;
        Ok(())
    }

    /// 移动失败后恢复原位置，同时恢复可见性；正文及关联的 AI、待办继续使用原邮件 id。
    pub fn rollback_moves(&self, account_id: i64, ids: &[i64]) -> Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        for id in ids {
            tx.execute(
                "UPDATE messages SET folder_id=orig_folder, uid=orig_uid, moving=0, orig_folder=NULL, orig_uid=NULL
                 WHERE id=? AND account_id=? AND uid<0 AND orig_folder IS NOT NULL",
                params![id, account_id],
            )?;
        }
        self.refresh_counts(account_id)?;
        tx.commit()?;
        Ok(())
    }

    pub fn fts_put(&self, msg_id: i64, subject: &str, people: &str, body: &str) -> Result<()> {
        self.conn.execute("DELETE FROM msg_fts WHERE rowid=?", [msg_id])?;
        self.conn.execute(
            "INSERT INTO msg_fts(rowid, subject, people, body) VALUES(?,?,?,?)",
            params![msg_id, subject, people, body],
        )?;
        Ok(())
    }

    pub fn delete_messages(&self, ids: &[i64]) -> Result<()> {
        // 一个事务里删完（几万行时比逐行自动提交快得多）；外层已经开了事务就沿用
        let own = self.conn.is_autocommit();
        if own {
            self.conn.execute_batch("BEGIN")?;
        }
        let r = (|| -> Result<()> {
            for id in ids {
                self.conn.execute("DELETE FROM msg_fts WHERE rowid=?", [id])?;
                self.conn.execute("DELETE FROM messages WHERE id=?", [id])?;
            }
            Ok(())
        })();
        if own {
            self.conn.execute_batch(if r.is_ok() { "COMMIT" } else { "ROLLBACK" })?;
        }
        r?;
        for id in ids {
            let _ = std::fs::remove_file(self.eml_path(*id));
        }
        Ok(())
    }

    pub fn ai_used_today(&self) -> i64 {
        let day = chrono::Local::now().format("%Y-%m-%d").to_string();
        self.conn
            .query_row("SELECT count FROM ai_usage WHERE day=?", [day], |r| r.get(0))
            .unwrap_or(0)
    }

    pub fn ai_count_up(&self) -> Result<()> {
        let day = chrono::Local::now().format("%Y-%m-%d").to_string();
        self.conn.execute(
            "INSERT INTO ai_usage(day,count) VALUES(?,1) ON CONFLICT(day) DO UPDATE SET count=count+1",
            [day],
        )?;
        Ok(())
    }
}

pub fn now() -> i64 {
    chrono::Utc::now().timestamp()
}

#[cfg(test)]
mod migrate_tests {
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("kjm-migrate-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    #[test]
    fn opens_fresh_and_reopens() {
        let d = tmp("fresh");
        let st = Store::open(&d).unwrap();
        let v: i64 = st.conn.query_row("PRAGMA user_version", [], |r| r.get(0)).unwrap();
        assert_eq!(v, LATEST);
        drop(st);
        Store::open(&d).unwrap();
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn moving_counts_match_visible_rows_and_rollback_restores_original_message() {
        let d = tmp("moving-counts");
        let st = Store::open(&d).unwrap();
        st.conn.execute("INSERT INTO accounts(id,email,username,imap_host,imap_port,smtp_host,smtp_port,created) VALUES(1,'a@example.com','a','h',993,'h',465,0)", []).unwrap();
        st.conn.execute("INSERT INTO folders(id,account_id,path,name,role) VALUES(1,1,'INBOX','Inbox','inbox'),(5,1,'Junk E-mail','Junk','junk')", []).unwrap();
        st.conn.execute("INSERT INTO messages(id,account_id,folder_id,uid,moving,seen,body_text,has_body) VALUES(10,1,5,-10,1,0,'缓存正文',1),(11,1,5,20,0,1,'',0)", []).unwrap();
        st.conn.execute("UPDATE messages SET orig_folder=1,orig_uid=7 WHERE id=10", []).unwrap();
        st.conn.execute("INSERT INTO ai(msg_id,summary) VALUES(10,'保留批注')", []).unwrap();
        st.conn.execute("INSERT INTO todos(msg_id,text,created) VALUES(10,'保留待办',0)", []).unwrap();
        let counts = |id| st.conn.query_row("SELECT unread,total FROM folders WHERE id=?", [id], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?))).unwrap();
        st.refresh_counts(1).unwrap();
        assert_eq!(counts(5), (0, 1), "隐藏的 moving 行不能计入侧栏");
        // 其他账号不能回滚本账号的移动。
        st.rollback_moves(2, &[10]).unwrap();
        assert_eq!(counts(1), (0, 0));
        st.rollback_moves(1, &[10]).unwrap();
        assert_eq!(counts(1), (1, 1));
        assert_eq!(counts(5), (0, 1));
        let row: (i64, i64, bool, String, Option<i64>, Option<i64>) = st.conn.query_row(
            "SELECT folder_id,uid,moving,body_text,orig_folder,orig_uid FROM messages WHERE id=10", [],
            |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?)),
        ).unwrap();
        assert_eq!(row, (1,7,false,"缓存正文".into(),None,None));
        assert_eq!(st.conn.query_row("SELECT summary FROM ai WHERE msg_id=10", [], |r| r.get::<_, String>(0)).unwrap(), "保留批注");
        assert_eq!(st.conn.query_row("SELECT text FROM todos WHERE msg_id=10", [], |r| r.get::<_, String>(0)).unwrap(), "保留待办");
        st.rollback_moves(1, &[10]).unwrap();
        assert_eq!(counts(1), (1, 1), "重复回滚应为空操作");
        drop(st);
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn adds_missing_legacy_columns() {
        let d = tmp("legacy");
        std::fs::create_dir_all(&d).unwrap();
        {
            // 最早期的库：accounts 没有 auth / focus / ai_level
            let c = Connection::open(d.join("kojoomail.db")).unwrap();
            c.execute_batch(SCHEMA).unwrap();
        }
        let st = Store::open(&d).unwrap();
        st.conn.query_row("SELECT auth, focus, ai_level FROM accounts LIMIT 1", [], |_| Ok(())).optional().unwrap();
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn structured_draft_migration_preserves_version_two_drafts() {
        let d = tmp("draft-content");
        std::fs::create_dir_all(&d).unwrap();
        {
            let c = Connection::open(d.join("kojoomail.db")).unwrap();
            c.execute_batch(SCHEMA).unwrap();
            c.execute_batch("PRAGMA user_version = 2").unwrap();
            c.execute("INSERT INTO drafts (body, mode, updated) VALUES (?, 'reply', 0)", ["正文\n\n-- \n签名\n\nOn 2026-11-13, A wrote:\n> 原文"]).unwrap();
        }
        let st = Store::open(&d).unwrap();
        let (body, content): (String, Option<String>) = st.conn.query_row("SELECT body,content_json FROM drafts", [], |r| Ok((r.get(0)?, r.get(1)?))).unwrap();
        assert!(body.contains("原文"));
        assert!(content.is_none());
        assert!(d.join("kojoomail.db.bak-v2").exists());
        assert_eq!(crate::html::restore_legacy(&body, "签名").body, "正文");
        drop(st);
        let _ = std::fs::remove_dir_all(d);
    }

    /// 老账号升级后：读默认开，写和发默认关，必须逐账号显式授权
    #[test]
    fn existing_accounts_get_read_only_mcp_defaults() {
        let d = tmp("mcpcols");
        std::fs::create_dir_all(&d).unwrap();
        {
            let c = Connection::open(d.join("kojoomail.db")).unwrap();
            c.execute_batch(SCHEMA).unwrap();
            c.execute(
                "INSERT INTO accounts (id, email, username, imap_host, imap_port, smtp_host, smtp_port, created) VALUES (1, 'old@example.com', 'old@example.com', 'h', 993, 'h', 465, 0)",
                [],
            ).unwrap();
        }
        let st = Store::open(&d).unwrap();
        let (r, w, s): (i64, i64, i64) = st
            .conn
            .query_row("SELECT mcp_read, mcp_write, mcp_send FROM accounts WHERE id = 1", [], |x| Ok((x.get(0)?, x.get(1)?, x.get(2)?)))
            .unwrap();
        assert_eq!((r, w, s), (1, 0, 0));
        drop(st);
        let _ = std::fs::remove_dir_all(&d);
    }

    fn add_t(tx: &rusqlite::Transaction) -> rusqlite::Result<()> {
        tx.execute_batch("CREATE TABLE t2(x INTEGER)")
    }
    fn add_t4(tx: &rusqlite::Transaction) -> rusqlite::Result<()> {
        tx.execute_batch("CREATE TABLE t4(x INTEGER)")
    }
    fn add_t_then_fail(tx: &rusqlite::Transaction) -> rusqlite::Result<()> {
        tx.execute_batch("CREATE TABLE t3(x INTEGER)")?;
        tx.execute_batch("SELECT * FROM no_such_table")
    }

    #[test]
    fn runs_steps_in_order_with_backup_and_rollback() {
        let d = tmp("steps");
        let mut st = Store::open(&d).unwrap();
        let ver = |st: &Store| st.conn.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0)).unwrap();
        st.migrate(1, &[add_t], true).unwrap();
        assert_eq!(ver(&st), 2);
        assert!(d.join("kojoomail.db.bak-v1").exists());
        // 已经是 2：同一步不会再跑（再跑会因表已存在而失败）
        st.migrate(2, &[add_t], true).unwrap();
        // 第二步失败：整步回滚，版本停在 2，表没建
        let err = st.migrate(2, &[add_t, add_t_then_fail], true).unwrap_err().to_string();
        assert!(err.contains("版本 3"), "{err}");
        assert_eq!(ver(&st), 2);
        let t3: i64 = st.conn.query_row("SELECT COUNT(*) FROM sqlite_master WHERE name='t3'", [], |r| r.get(0)).unwrap();
        assert_eq!(t3, 0);
        // 失败后外键要恢复成开
        let fk: i64 = st.conn.query_row("PRAGMA foreign_keys", [], |r| r.get(0)).unwrap();
        assert_eq!(fk, 1);
        // 再升一次成功：版本 1 的旧备份被清掉，只留这次的
        std::fs::write(d.join("kojoomail.db.bak-v0"), b"old").unwrap();
        st.migrate(2, &[add_t, add_t4], true).unwrap();
        assert!(d.join("kojoomail.db.bak-v2").exists());
        assert!(!d.join("kojoomail.db.bak-v1").exists() && !d.join("kojoomail.db.bak-v0").exists());
        drop(st);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn refuses_newer_database() {
        let d = tmp("newer");
        std::fs::create_dir_all(&d).unwrap();
        Connection::open(d.join("kojoomail.db")).unwrap().execute_batch(&format!("PRAGMA user_version = {}", LATEST + 1)).unwrap();
        let err = Store::open(&d).err().expect("应该拒绝").to_string();
        assert!(err.contains("更新版本"), "{err}");
        let _ = std::fs::remove_dir_all(&d);
    }
}
