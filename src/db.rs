// =====================================================================
// db.rs
// ---------------------------------------------------------------------
// 📌 역할: 로컬 SQLite DB 관리 (기획서 7. 로컬 DB 설계)
//
// 🔧 담당 기능
//   1) DB 파일 자동 생성 + 스키마 마이그레이션
//   2) auth 테이블: 인증 정보 + 토큰 보관 (단일 행)
//   3) events 테이블: 모든 이벤트 영구 기록 (sync_status 추적)
//   4) settings 테이블: 정책 / 설정값 캐시
//   5) 통계 쿼리: 오늘 미사용 누적시간 / 횟수
//
// 🔒 동시성
//   - Connection 을 Mutex 로 감싸 thread-safe 보장
//   - rusqlite 자체는 Sync 가 아니므로 외부 잠금 필수
//
// ⚠️ 보안 주의 (기획 3-1)
//   - access_token / refresh_token 은 평문 저장 금지
//   - TODO: 운영 시 DPAPI(Windows) / Keychain(macOS) 으로 암호화
// =====================================================================

use crate::events::{AgentEvent, SyncStatus};
use chrono::{DateTime, Utc};
use rusqlite::{params, Connection, Result as SqlResult};
use std::path::PathBuf;
use std::sync::Mutex;

/// SQLite DB 핸들 (앱 전역에서 Arc 로 공유)
pub struct Database {
    conn: Mutex<Connection>,
}

impl Database {
    /// DB 파일 오픈 (없으면 생성). 부모 디렉토리도 자동 생성.
    pub fn open(path: &PathBuf) -> SqlResult<Self> {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let conn = Connection::open(path)?;
        let db = Self {
            conn: Mutex::new(conn),
        };
        db.migrate()?;
        Ok(db)
    }

    /// 테이블 / 인덱스 자동 생성 (CREATE TABLE IF NOT EXISTS)
    /// - 새 버전 출시 시 ALTER TABLE 추가 마이그레이션 가능하도록 점진 확장
    fn migrate(&self) -> SqlResult<()> {
        let conn = self.conn.lock().unwrap();

        // auth 테이블 - 로그인 정보 + 토큰 보관
        // ⚠️ TODO: access_token / refresh_token 은 평문 저장 금지 (기획 3-1 보안 기준)
        //         실배포 시 Windows DPAPI 또는 Credential Manager로 암호화 후 저장 필요
        conn.execute(
            "CREATE TABLE IF NOT EXISTS auth (
                id INTEGER PRIMARY KEY,
                company_id INTEGER,
                employee_id INTEGER,
                employee_name TEXT,
                company_name TEXT,
                team_id INTEGER,
                team_name TEXT,
                access_token TEXT,
                refresh_token TEXT,
                device_id TEXT,
                device_name TEXT,
                auto_login INTEGER DEFAULT 0,
                last_login_at TEXT,
                created_at TEXT,
                updated_at TEXT
            )",
            [],
        )?;

        // events 테이블 - 모든 이벤트 로그
        conn.execute(
            "CREATE TABLE IF NOT EXISTS events (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                event_id TEXT UNIQUE NOT NULL,
                company_id INTEGER,
                employee_id INTEGER,
                team_id INTEGER,
                device_id TEXT,
                event_type TEXT NOT NULL,
                event_time TEXT NOT NULL,
                idle_seconds INTEGER,
                threshold_seconds INTEGER,
                app_version TEXT,
                sync_status TEXT NOT NULL DEFAULT 'PENDING',
                retry_count INTEGER NOT NULL DEFAULT 0,
                created_at TEXT NOT NULL,
                synced_at TEXT
            )",
            [],
        )?;

        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_events_sync ON events(sync_status, created_at)",
            [],
        )?;

        // settings 테이블 - 설정값 (정책)
        conn.execute(
            "CREATE TABLE IF NOT EXISTS settings (
                key TEXT PRIMARY KEY,
                value TEXT,
                updated_at TEXT
            )",
            [],
        )?;

        Ok(())
    }

    // ===== auth (단일 행 보관 - id=1) =====

    /// 인증 정보 저장 또는 업데이트 (UPSERT)
    /// - 로그인 성공 / 토큰 갱신 시 호출
    pub fn save_auth(&self, auth: &AuthRecord) -> SqlResult<()> {
        let conn = self.conn.lock().unwrap();
        let now = Utc::now().to_rfc3339();
        conn.execute(
            "INSERT OR REPLACE INTO auth
             (id, company_id, employee_id, employee_name, company_name, team_id, team_name,
              access_token, refresh_token, device_id, device_name, auto_login, last_login_at, created_at, updated_at)
             VALUES (1, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, COALESCE((SELECT created_at FROM auth WHERE id=1), ?), ?)",
            params![
                auth.company_id, auth.employee_id, auth.employee_name, auth.company_name,
                auth.team_id, auth.team_name, auth.access_token, auth.refresh_token,
                auth.device_id, auth.device_name, auth.auto_login as i32,
                auth.last_login_at.to_rfc3339(), now, now,
            ],
        )?;
        Ok(())
    }

    /// 저장된 인증 정보 조회 (자동로그인 시도용)
    /// - None 반환 시 → 로그인 화면 표시 필요
    pub fn load_auth(&self) -> SqlResult<Option<AuthRecord>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT company_id, employee_id, employee_name, company_name, team_id, team_name,
                    access_token, refresh_token, device_id, device_name, auto_login, last_login_at
             FROM auth WHERE id = 1",
        )?;
        let mut rows = stmt.query([])?;
        if let Some(row) = rows.next()? {
            Ok(Some(AuthRecord {
                company_id: row.get(0)?,
                employee_id: row.get(1)?,
                employee_name: row.get(2)?,
                company_name: row.get(3)?,
                team_id: row.get(4)?,
                team_name: row.get(5)?,
                access_token: row.get(6)?,
                refresh_token: row.get(7)?,
                device_id: row.get(8)?,
                device_name: row.get(9)?,
                auto_login: row.get::<_, i32>(10)? != 0,
                last_login_at: DateTime::parse_from_rfc3339(&row.get::<_, String>(11)?)
                    .map(|d| d.with_timezone(&Utc))
                    .unwrap_or_else(|_| Utc::now()),
            }))
        } else {
            Ok(None)
        }
    }

    /// 인증 정보 전부 삭제 (로그아웃 시)
    pub fn clear_auth(&self) -> SqlResult<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM auth", [])?;
        Ok(())
    }

    // ===== events =====

    /// 이벤트 한 건 저장 (sync_status = PENDING 으로 시작)
    /// - tracker / 로그인 / 로그아웃 등 모든 발생 지점에서 호출
    pub fn insert_event(&self, event: &AgentEvent, ctx: &EventContext) -> SqlResult<()> {
        let conn = self.conn.lock().unwrap();
        let now = Utc::now().to_rfc3339();
        conn.execute(
            "INSERT INTO events
             (event_id, company_id, employee_id, team_id, device_id, event_type, event_time,
              idle_seconds, threshold_seconds, app_version, sync_status, retry_count, created_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 'PENDING', 0, ?)",
            params![
                event.event_id, ctx.company_id, ctx.employee_id, ctx.team_id, ctx.device_id,
                event.event_type, event.event_time.to_rfc3339(),
                event.idle_seconds, event.threshold_seconds, event.app_version, now,
            ],
        )?;
        Ok(())
    }

    /// 미전송(PENDING) 이벤트 일괄 조회
    /// - sync.rs 의 백그라운드 스레드가 60초마다 호출
    /// - 반환값: (DB row id, 이벤트) 쌍 → mark_events 에 row id 전달용
    pub fn pending_events(&self, limit: usize) -> SqlResult<Vec<(i64, AgentEvent)>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, event_id, event_type, event_time, idle_seconds, threshold_seconds, app_version
             FROM events WHERE sync_status = 'PENDING' ORDER BY id ASC LIMIT ?",
        )?;
        let rows = stmt.query_map(params![limit as i64], |row| {
            let event_time_str: String = row.get(3)?;
            let event_time = DateTime::parse_from_rfc3339(&event_time_str)
                .map(|d| d.with_timezone(&Utc))
                .unwrap_or_else(|_| Utc::now());
            Ok((
                row.get::<_, i64>(0)?,
                AgentEvent {
                    event_id: row.get(1)?,
                    event_type: row.get(2)?,
                    event_time,
                    idle_seconds: row.get(4)?,
                    threshold_seconds: row.get(5)?,
                    app_version: row.get(6)?,
                },
            ))
        })?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    /// 여러 이벤트의 sync_status 일괄 업데이트
    /// - 서버 전송 결과(SUCCESS/FAILED)에 따라 호출
    /// - FAILED 시 retry_count 자동 증가 (재시도 정책 판단용)
    pub fn mark_events(&self, ids: &[i64], status: SyncStatus) -> SqlResult<()> {
        if ids.is_empty() {
            return Ok(());
        }
        let conn = self.conn.lock().unwrap();
        let now = Utc::now().to_rfc3339();
        let placeholders = ids.iter().map(|_| "?").collect::<Vec<_>>().join(",");
        let sql = format!(
            "UPDATE events SET sync_status = ?, synced_at = ?, retry_count = retry_count + CASE WHEN ? = 'FAILED' THEN 1 ELSE 0 END
             WHERE id IN ({})",
            placeholders
        );
        let mut params_vec: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
        params_vec.push(Box::new(status.as_str().to_string()));
        params_vec.push(Box::new(now));
        params_vec.push(Box::new(status.as_str().to_string()));
        for id in ids {
            params_vec.push(Box::new(*id));
        }
        let refs: Vec<&dyn rusqlite::ToSql> = params_vec.iter().map(|p| p.as_ref()).collect();
        conn.execute(&sql, refs.as_slice())?;
        Ok(())
    }

    /// 오늘(로컬 자정 ~ 현재) 발생한 특정 타입 이벤트 개수
    /// - 메인 화면 통계 카드: "오늘 미사용 횟수" 표시용
    pub fn count_today_events(&self, event_type_code: &str) -> SqlResult<i64> {
        let conn = self.conn.lock().unwrap();
        let today_start = chrono::Local::now()
            .date_naive()
            .and_hms_opt(0, 0, 0)
            .unwrap()
            .and_local_timezone(chrono::Local)
            .unwrap()
            .with_timezone(&Utc);
        let mut stmt = conn.prepare(
            "SELECT COUNT(*) FROM events WHERE event_type = ? AND event_time >= ?",
        )?;
        let count: i64 = stmt.query_row(
            params![event_type_code, today_start.to_rfc3339()],
            |r| r.get(0),
        )?;
        Ok(count)
    }

    /// 오늘 누적 미사용 시간 합계(초)
    /// - IDLE_ENDED 이벤트의 idle_seconds 합산
    /// - 메인 화면 통계 카드: "오늘 미사용 누적" 표시용
    pub fn sum_today_idle_seconds(&self) -> SqlResult<i64> {
        let conn = self.conn.lock().unwrap();
        let today_start = chrono::Local::now()
            .date_naive()
            .and_hms_opt(0, 0, 0)
            .unwrap()
            .and_local_timezone(chrono::Local)
            .unwrap()
            .with_timezone(&Utc);
        let mut stmt = conn.prepare(
            "SELECT COALESCE(SUM(idle_seconds), 0) FROM events
             WHERE event_type = 'IDLE_ENDED' AND event_time >= ?",
        )?;
        let total: i64 = stmt.query_row(params![today_start.to_rfc3339()], |r| r.get(0))?;
        Ok(total)
    }
}

/// auth 테이블 한 행을 메모리에서 다루는 DTO
#[derive(Debug, Clone)]
pub struct AuthRecord {
    pub company_id: Option<i64>,
    pub employee_id: Option<i64>,
    pub employee_name: Option<String>,
    pub company_name: Option<String>,
    pub team_id: Option<i64>,
    pub team_name: Option<String>,
    pub access_token: Option<String>,
    pub refresh_token: Option<String>,
    pub device_id: Option<String>,
    pub device_name: Option<String>,
    pub auto_login: bool,
    pub last_login_at: DateTime<Utc>,
}

/// 이벤트 저장 시 동봉되는 컨텍스트 (회사/직원/팀/디바이스)
/// - 모든 이벤트는 이 컨텍스트와 함께 저장되어 서버 통계 집계 가능
#[derive(Debug, Clone, Default)]
pub struct EventContext {
    pub company_id: Option<i64>,
    pub employee_id: Option<i64>,
    pub team_id: Option<i64>,
    pub device_id: Option<String>,
}
