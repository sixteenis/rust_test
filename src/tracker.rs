// =====================================================================
// tracker.rs
// ---------------------------------------------------------------------
// 📌 역할: PC 사용/미사용 감지 (기획 3-4, 3-5, 3-6)
//
// 🔧 담당 기능
//   1) 백그라운드 스레드에서 device_query 로 5초마다 입력 폴링
//   2) 직전 입력 이후 경과시간 계산 → 기준값 초과 시 IDLE_STARTED 발행
//   3) 다시 입력 발생 시 IDLE_ENDED 발행 (idle_seconds 포함)
//   4) 디버그 로그: 입력 종류 / 경과시간 / 상태 전환
//   5) PC 잠금/잠금해제 모니터 stub (TODO)
//
// 🔒 개인정보 원칙 (기획 4-2)
//   - 절대 키 입력 내용 저장 X
//   - get_keys() 결과는 변화 감지용으로만 비교 후 폐기
//   - 마우스 좌표도 변화 여부 판단용 (저장 X)
//
// 🔗 사용 위치
//   - main.rs::ensure_background_threads 에서 spawn_tracker 호출
// =====================================================================

use crate::events::{AgentEvent, EventType};
use device_query::{DeviceQuery, DeviceState, Keycode};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

/// 에이전트 현재 상태 (메인 화면 상단 인디케이터에 표시)
/// - Active: 정상 사용 중
/// - IdlePending: 미사용 후보 (현재는 미사용)
/// - Idle: 미사용 기록 중
/// - Locked: PC 잠금 상태
/// - Offline: 서버 연결 불가
/// - Error: 일반 오류
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentStatus {
    Active,
    IdlePending,
    Idle,
    Locked,
    Offline,
    Error,
}

impl AgentStatus {
    pub fn label_kr(&self) -> &'static str {
        match self {
            AgentStatus::Active => "사용 중",
            AgentStatus::IdlePending => "미사용 후보",
            AgentStatus::Idle => "미사용",
            AgentStatus::Locked => "PC 잠금",
            AgentStatus::Offline => "서버 연결 대기",
            AgentStatus::Error => "오류",
        }
    }
}

/// Tracker 스레드와 UI 스레드가 공유하는 상태 (Arc<Mutex<...>> 로 thread-safe)
/// - status: 현재 사용/미사용 상태
/// - last_input_at: 마지막 입력 시각 (Instant)
/// - idle_started_at: 미사용 진입 시점 (None = 사용 중)
/// - last_activity_kind: 최근 입력 종류 (디버그/UI 표시용)
#[derive(Clone)]
pub struct TrackerState {
    pub status: Arc<Mutex<AgentStatus>>,
    pub last_input_at: Arc<Mutex<Instant>>,
    pub idle_started_at: Arc<Mutex<Option<Instant>>>,
    pub last_activity_kind: Arc<Mutex<&'static str>>,
}

impl TrackerState {
    pub fn new() -> Self {
        Self {
            status: Arc::new(Mutex::new(AgentStatus::Active)),
            last_input_at: Arc::new(Mutex::new(Instant::now())),
            idle_started_at: Arc::new(Mutex::new(None)),
            last_activity_kind: Arc::new(Mutex::new("none")),
        }
    }

    pub fn elapsed_since_last_input(&self) -> u64 {
        self.last_input_at.lock().unwrap().elapsed().as_secs()
    }
}

/// Tracker 백그라운드 스레드 시작
/// - 파라미터:
///     state             : UI 와 공유할 상태 핸들
///     threshold_seconds : 미사용 판단 기준(초). 기본 300(5분)
///     poll_interval     : 폴링 주기. 기본 5초 (기획 3-4)
///     tx                : 발생 이벤트를 main 스레드로 보내는 채널
pub fn spawn_tracker(
    state: TrackerState,
    threshold_seconds: i64,
    poll_interval: Duration,
    tx: Sender<AgentEvent>,
) {
    log::info!(
        "📡 Tracker 시작 - 미사용 판단 기준: {}초, 폴링 주기: {:?}",
        threshold_seconds, poll_interval
    );

    thread::spawn(move || {
        let device_state = DeviceState::new();
        let mut prev_keys: Vec<Keycode> = Vec::new();
        let mut prev_pos: (i32, i32) = (i32::MIN, i32::MIN);
        let mut prev_buttons: Vec<bool> = Vec::new();
        let mut last_idle_log: Option<Instant> = None;
        let mut last_mouse_move_log: Option<Instant> = None;

        loop {
            let now = Instant::now();
            // 우선순위: 키 입력 > 마우스 클릭/휠 > 마우스 이동
            // (한 사이클에서 여러 종류가 잡히면 더 의미 있는 쪽으로 표시)
            let mut input_kind: Option<&'static str> = None;
            let mut is_mouse_move_only = false;

            // 키 변화 감지 (내용은 저장하지 않음 - 길이만 비교)
            let keys = device_state.get_keys();
            if keys != prev_keys {
                let kind = if keys.len() > prev_keys.len() {
                    "키 누름"
                } else if keys.len() < prev_keys.len() {
                    "키 뗌"
                } else {
                    "키 변화" // 동시 누름/뗌
                };
                input_kind = Some(kind);
                prev_keys = keys;
            }

            // 마우스 버튼 변화 (클릭은 키보다 우선순위 낮지만 키보다 자주 발생하지 않음)
            let mouse = device_state.get_mouse();
            if mouse.button_pressed != prev_buttons {
                let pressed_count = mouse.button_pressed.iter().filter(|&&b| b).count();
                let prev_count = prev_buttons.iter().filter(|&&b| b).count();
                input_kind = Some(if pressed_count > prev_count {
                    "마우스 클릭"
                } else {
                    "마우스 클릭 해제"
                });
                prev_buttons = mouse.button_pressed.clone();
            }

            // 마우스 좌표 변화 (가장 흔하게 발생 - 다른 입력 없을 때만 표시)
            if mouse.coords != prev_pos {
                if input_kind.is_none() {
                    input_kind = Some("마우스 이동");
                    is_mouse_move_only = true;
                }
                prev_pos = mouse.coords;
            }

            if let Some(kind) = input_kind {
                // 🟢 입력 감지 로그
                // 마우스 이동은 너무 자주 발생하므로 500ms 에 한 번만 로그 출력 (콘솔 가독성)
                // 키 입력 / 클릭은 항상 즉시 로그
                let should_log = if is_mouse_move_only {
                    match last_mouse_move_log {
                        Some(t) => t.elapsed() >= Duration::from_millis(500),
                        None => true,
                    }
                } else {
                    true
                };

                let was_idle = {
                    let st = state.status.lock().unwrap();
                    matches!(*st, AgentStatus::Idle | AgentStatus::IdlePending)
                };

                if should_log {
                    let elapsed = state.last_input_at.lock().unwrap().elapsed().as_secs();
                    log::info!(
                        "🟢 [입력 감지] {} (직전 입력 후 {}초 경과)",
                        kind, elapsed
                    );
                    if is_mouse_move_only {
                        last_mouse_move_log = Some(now);
                    }
                }

                if was_idle {
                    // 🔄 IDLE_ENDED - 복귀 이벤트 발행
                    let started = state.idle_started_at.lock().unwrap().take();
                    if let Some(start) = started {
                        let idle_secs = start.elapsed().as_secs() as i64;
                        let _ = tx.send(AgentEvent::new(
                            EventType::IdleEnded,
                            Some(idle_secs),
                            Some(threshold_seconds),
                        ));
                        log::info!(
                            "🔄 [IDLE_ENDED] 복귀! 미사용 시간 = {}초 ({})",
                            idle_secs,
                            format_duration(idle_secs)
                        );
                    }
                    *state.status.lock().unwrap() = AgentStatus::Active;
                    last_idle_log = None;
                }

                *state.last_input_at.lock().unwrap() = now;
                *state.last_activity_kind.lock().unwrap() = kind;
            } else {
                // 입력 없음 - 경과시간 체크
                let elapsed = state.last_input_at.lock().unwrap().elapsed().as_secs() as i64;

                // ⏱ 매 폴링 사이클마다 경과시간 로그 출력
                let should_log = match last_idle_log {
                    Some(t) => t.elapsed() >= Duration::from_secs(5),
                    None => true,
                };
                if should_log {
                    log::info!(
                        "⏱  [대기] 마지막 입력 후 {}초 경과 (기준: {}초)",
                        elapsed, threshold_seconds
                    );
                    last_idle_log = Some(Instant::now());
                }

                let mut status = state.status.lock().unwrap();
                if elapsed >= threshold_seconds && *status == AgentStatus::Active {
                    // 🟡 기준 시간 초과 -> IDLE_STARTED
                    *status = AgentStatus::Idle;
                    drop(status);
                    *state.idle_started_at.lock().unwrap() = Some(
                        Instant::now() - Duration::from_secs(threshold_seconds as u64),
                    );
                    let _ = tx.send(AgentEvent::new(
                        EventType::IdleStarted,
                        Some(threshold_seconds),
                        Some(threshold_seconds),
                    ));
                    log::info!(
                        "🟡 [IDLE_STARTED] 미사용 상태 진입 - 기준 {}초 초과",
                        threshold_seconds
                    );
                }
            }

            thread::sleep(poll_interval);
        }
    });
}

/// 초 단위 시간을 한국어 "X시간 Y분 Z초" 형식으로 변환 (로그 가독성용)
fn format_duration(seconds: i64) -> String {
    let s = seconds.max(0);
    let h = s / 3600;
    let m = (s % 3600) / 60;
    let sec = s % 60;
    if h > 0 {
        format!("{}시간 {}분 {}초", h, m, sec)
    } else if m > 0 {
        format!("{}분 {}초", m, sec)
    } else {
        format!("{}초", sec)
    }
}

// 🔒 PC 잠금/잠금해제 감지 (기획 3-6)
//
// ⚠️ TODO: Windows 환경에서 WTSRegisterSessionNotification 또는
//          WM_WTSSESSION_CHANGE 메시지를 받아 PC_LOCKED / PC_UNLOCKED 이벤트 발행 필요.
//          windows-rs 크레이트로 구현 가능.
//          macOS에서는 NSDistributedNotificationCenter 의 com.apple.screenIsLocked
//          / com.apple.screenIsUnlocked 알림 구독.
//
// 현재는 stub 으로 처리 - 추후 OS별 구현 필요.
#[allow(dead_code)]
pub fn spawn_lock_monitor(_tx: Sender<AgentEvent>) {
    // ⚠️ TODO: 실제 OS API 연동
    log::warn!("🔒 PC 잠금/잠금해제 감지: TODO (OS별 구현 필요)");
}
