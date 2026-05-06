// =====================================================================
// api.rs
// ---------------------------------------------------------------------
// 📌 역할: 핀플 서버 PC-Agent API 클라이언트 (기획 10, 11)
//
// 🔧 담당 기능
//   1) 로그인 / 토큰 갱신 / 로그아웃
//   2) 이벤트 일괄 전송 (배치)
//   3) 정책 조회 (idle threshold 등)
//   4) 요청 / 응답 DTO 정의
//
// ⚠️ 현재 상태: MOCK
//   - 실제 서버에 HTTP 요청을 보내지 않고 가짜 응답을 즉시 반환
//   - 운영 시 ureq::post 호출로 본문 교체 필요 (예시 주석 포함)
//
// 🔗 사용 위치
//   - main.rs: 로그인 / 자동로그인 / 로그아웃
//   - sync.rs: 이벤트 배치 전송
// =====================================================================

use crate::events::AgentEvent;
use serde::{Deserialize, Serialize};
use std::fmt;

// ⚠️ TODO: 운영 시 환경변수 PINPLE_API_BASE 또는 settings.json 에서 로드
pub const API_BASE_URL: &str = "https://api.pinple.example/api/pc-agent";

/// 로그인 요청 본문 (기획 10-1)
#[derive(Debug, Serialize)]
pub struct LoginRequest<'a> {
    pub login_id: &'a str,
    pub password: &'a str,
    pub device_id: &'a str,
    pub device_name: &'a str,
    pub os_type: &'a str,
    pub os_version: &'a str,
    pub app_version: &'a str,
}

/// 로그인 응답 (access/refresh 토큰 + 회사/직원/팀 정보 + 정책)
#[derive(Debug, Deserialize, Clone)]
pub struct LoginResponse {
    pub success: bool,
    pub access_token: String,
    pub refresh_token: String,
    pub company_id: i64,
    pub employee_id: i64,
    pub employee_name: String,
    pub company_name: String,
    pub team_id: i64,
    pub team_name: String,
    pub policy: PolicyResponse,
}

/// 관리자가 회사/팀/근로자 단위로 설정 가능한 정책 (기획 10-4)
#[derive(Debug, Deserialize, Clone)]
pub struct PolicyResponse {
    pub idle_threshold_seconds: i64,
    pub long_idle_threshold_seconds: i64,
    pub sync_interval_seconds: i64,
    pub allow_exit: bool,
    pub track_only_worktime: bool,
    pub exclude_breaktime: bool,
    pub track_lock_unlock: bool,
    pub track_after_work: bool,
}

impl Default for PolicyResponse {
    fn default() -> Self {
        // 기획서 3-4 기본값
        Self {
            idle_threshold_seconds: 300,      // 5분
            long_idle_threshold_seconds: 900, // 15분
            sync_interval_seconds: 60,
            allow_exit: true,
            track_only_worktime: false,
            exclude_breaktime: false,
            track_lock_unlock: true,
            track_after_work: true,
        }
    }
}

/// 이벤트 배치 전송 요청 (60초마다 누적된 PENDING 이벤트 묶음 전송)
#[derive(Debug, Serialize)]
pub struct EventBatchRequest<'a> {
    pub device_id: &'a str,
    pub employee_id: i64,
    pub company_id: i64,
    pub events: &'a [AgentEvent],
}

#[derive(Debug, Deserialize)]
pub struct EventBatchResponse {
    pub success: bool,
    pub received_count: i64,
}

/// HTTP API 호출을 캡슐화하는 클라이언트
/// - 운영 시 access_token / 재시도 / 타임아웃 등을 추가 관리
pub struct ApiClient;

impl ApiClient {
    pub fn new() -> Self {
        Self
    }

    /// 기획 10-1 POST /api/pc-agent/login
    /// ⚠️ TODO: 실제 서버 연동 - 현재는 mock 응답 반환
    pub fn login(&self, req: &LoginRequest) -> Result<LoginResponse, ApiError> {
        log::info!("[MOCK] login: id={}, device={}", req.login_id, req.device_id);

        // ⚠️ TODO: 운영 코드로 교체
        // let resp: LoginResponse = ureq::post(&format!("{}/login", API_BASE_URL))
        //     .send_json(req)
        //     .map_err(|e| ApiError::Network(e.to_string()))?
        //     .into_json()
        //     .map_err(|e| ApiError::Parse(e.to_string()))?;
        // Ok(resp)

        if req.login_id.is_empty() || req.password.is_empty() {
            return Err(ApiError::AuthFailed("아이디/비밀번호를 입력하세요.".into()));
        }

        Ok(LoginResponse {
            success: true,
            access_token: format!("mock_access_{}", uuid::Uuid::new_v4()),
            refresh_token: format!("mock_refresh_{}", uuid::Uuid::new_v4()),
            company_id: 1001,
            employee_id: 2001,
            employee_name: req.login_id.to_string(),
            company_name: "주식회사 예시".to_string(),
            team_id: 10,
            team_name: "영업팀".to_string(),
            policy: PolicyResponse::default(),
        })
    }

    /// 기획 10-2 POST /api/pc-agent/refresh
    /// ⚠️ TODO: 실제 서버 연동
    pub fn refresh(
        &self,
        refresh_token: &str,
        device_id: &str,
    ) -> Result<LoginResponse, ApiError> {
        log::info!("[MOCK] refresh: device={}", device_id);

        if refresh_token.is_empty() {
            return Err(ApiError::AuthFailed("토큰 만료".into()));
        }
        Ok(LoginResponse {
            success: true,
            access_token: format!("mock_access_{}", uuid::Uuid::new_v4()),
            refresh_token: refresh_token.to_string(),
            company_id: 1001,
            employee_id: 2001,
            employee_name: "사용자".to_string(),
            company_name: "주식회사 예시".to_string(),
            team_id: 10,
            team_name: "영업팀".to_string(),
            policy: PolicyResponse::default(),
        })
    }

    /// 기획 10-3 POST /api/pc-agent/events
    /// ⚠️ TODO: 실제 서버 연동
    pub fn send_events(
        &self,
        req: &EventBatchRequest,
    ) -> Result<EventBatchResponse, ApiError> {
        log::info!("[MOCK] send_events: count={}", req.events.len());

        // ⚠️ TODO: 실제 호출 (실패 시 ApiError::Network 반환)
        // ureq::post(&format!("{}/events", API_BASE_URL))
        //     .set("Authorization", &format!("Bearer {}", access_token))
        //     .send_json(req)?
        //     .into_json()?
        Ok(EventBatchResponse {
            success: true,
            received_count: req.events.len() as i64,
        })
    }

    /// 기획 10-5 POST /api/pc-agent/logout
    /// ⚠️ TODO: 실제 서버 연동
    pub fn logout(&self, device_id: &str, employee_id: i64) -> Result<(), ApiError> {
        log::info!("[MOCK] logout: device={}, emp={}", device_id, employee_id);
        Ok(())
    }
}

/// API 호출 실패 종류
/// - Network: 인터넷 끊김 / 타임아웃 / DNS 등
/// - Parse: 응답 JSON 파싱 실패 (서버 스펙 변경 시 발생 가능)
/// - AuthFailed: 401/403 또는 자격 증명 오류
#[derive(Debug)]
pub enum ApiError {
    Network(String),
    Parse(String),
    AuthFailed(String),
}

impl fmt::Display for ApiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ApiError::Network(s) => write!(f, "네트워크 오류: {}", s),
            ApiError::Parse(s) => write!(f, "응답 파싱 오류: {}", s),
            ApiError::AuthFailed(s) => write!(f, "인증 실패: {}", s),
        }
    }
}

impl std::error::Error for ApiError {}
