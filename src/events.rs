// =====================================================================
// events.rs
// ---------------------------------------------------------------------
// 📌 역할: 에이전트가 발생시키는 모든 이벤트의 타입과 데이터 구조 정의
//
// 🔧 담당 기능
//   1) EventType: 기획서 9. 이벤트 코드 정의 (15종) 의 enum 표현
//   2) SyncStatus: 서버 전송 상태 (PENDING / SUCCESS / FAILED)
//   3) AgentEvent: DB / 서버로 전달되는 이벤트 페이로드
//
// 🔗 사용 위치
//   - tracker.rs: IDLE_STARTED / IDLE_ENDED / USER_ACTIVE 등 발생
//   - main.rs: APP_STARTED / LOGIN_* / LOGOUT 발생
//   - db.rs: events 테이블 저장 시 사용
//   - api.rs: 서버 전송 페이로드로 직렬화
// =====================================================================

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// 에이전트 이벤트 종류 (기획서 9 - 총 15종)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EventType {
    AppStarted,
    AppStopped,
    LoginSuccess,
    LoginFailed,
    AutoLoginSuccess,
    AutoLoginFailed,
    UserActive,
    IdleStarted,
    IdleContinued,
    IdleEnded,
    PcLocked,
    PcUnlocked,
    SyncSuccess,
    SyncFailed,
    Logout,
}

impl EventType {
    /// enum 값을 서버/DB 에서 사용하는 대문자 스네이크 코드로 변환
    pub fn as_code(&self) -> &'static str {
        match self {
            EventType::AppStarted => "APP_STARTED",
            EventType::AppStopped => "APP_STOPPED",
            EventType::LoginSuccess => "LOGIN_SUCCESS",
            EventType::LoginFailed => "LOGIN_FAILED",
            EventType::AutoLoginSuccess => "AUTO_LOGIN_SUCCESS",
            EventType::AutoLoginFailed => "AUTO_LOGIN_FAILED",
            EventType::UserActive => "USER_ACTIVE",
            EventType::IdleStarted => "IDLE_STARTED",
            EventType::IdleContinued => "IDLE_CONTINUED",
            EventType::IdleEnded => "IDLE_ENDED",
            EventType::PcLocked => "PC_LOCKED",
            EventType::PcUnlocked => "PC_UNLOCKED",
            EventType::SyncSuccess => "SYNC_SUCCESS",
            EventType::SyncFailed => "SYNC_FAILED",
            EventType::Logout => "LOGOUT",
        }
    }

    /// 코드 문자열에서 enum 값으로 역변환 (DB 조회 시 사용)
    pub fn from_code(code: &str) -> Option<Self> {
        Some(match code {
            "APP_STARTED" => EventType::AppStarted,
            "APP_STOPPED" => EventType::AppStopped,
            "LOGIN_SUCCESS" => EventType::LoginSuccess,
            "LOGIN_FAILED" => EventType::LoginFailed,
            "AUTO_LOGIN_SUCCESS" => EventType::AutoLoginSuccess,
            "AUTO_LOGIN_FAILED" => EventType::AutoLoginFailed,
            "USER_ACTIVE" => EventType::UserActive,
            "IDLE_STARTED" => EventType::IdleStarted,
            "IDLE_CONTINUED" => EventType::IdleContinued,
            "IDLE_ENDED" => EventType::IdleEnded,
            "PC_LOCKED" => EventType::PcLocked,
            "PC_UNLOCKED" => EventType::PcUnlocked,
            "SYNC_SUCCESS" => EventType::SyncSuccess,
            "SYNC_FAILED" => EventType::SyncFailed,
            "LOGOUT" => EventType::Logout,
            _ => return None,
        })
    }
}

/// 서버 전송 상태 (events 테이블 sync_status 컬럼에 저장)
/// - Pending: 미전송 (서버 sync 스레드가 처리 대기)
/// - Success: 전송 완료
/// - Failed: 일정 횟수 이상 실패 (영구 실패)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyncStatus {
    Pending,
    Success,
    Failed,
}

impl SyncStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            SyncStatus::Pending => "PENDING",
            SyncStatus::Success => "SUCCESS",
            SyncStatus::Failed => "FAILED",
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s {
            "SUCCESS" => SyncStatus::Success,
            "FAILED" => SyncStatus::Failed,
            _ => SyncStatus::Pending,
        }
    }
}

/// 에이전트 → 서버 / 로컬 DB 로 전송되는 이벤트 한 건의 페이로드
/// - event_id: UUID v4 (중복 방지용 unique key)
/// - event_type: EventType.as_code() 결과를 문자열로 보관 (직렬화 편의)
/// - idle_seconds: IDLE_ENDED 일 때만 의미 있는 미사용 누적 시간(초)
/// - threshold_seconds: 발생 시점의 미사용 판정 기준값
/// - app_version: 클라이언트 버전 추적용
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentEvent {
    pub event_id: String,
    pub event_type: String,
    pub event_time: DateTime<Utc>,
    pub idle_seconds: Option<i64>,
    pub threshold_seconds: Option<i64>,
    pub app_version: String,
}

impl AgentEvent {
    /// 신규 이벤트 생성 (UUID + 현재 UTC 시각 자동 부여)
    pub fn new(
        event_type: EventType,
        idle_seconds: Option<i64>,
        threshold_seconds: Option<i64>,
    ) -> Self {
        Self {
            event_id: uuid::Uuid::new_v4().to_string(),
            event_type: event_type.as_code().to_string(),
            event_time: Utc::now(),
            idle_seconds,
            threshold_seconds,
            app_version: env!("CARGO_PKG_VERSION").to_string(),
        }
    }
}
