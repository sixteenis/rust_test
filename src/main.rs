// =====================================================================
// main.rs
// ---------------------------------------------------------------------
// 📌 역할: 핀플 PC 에이전트 앱의 진입점(entry point)
//
// 🔧 담당 기능
//   1) 앱 부팅: 로그 초기화, eframe 윈도우 생성, 한글 폰트 로드
//   2) 화면 상태 머신: AppScreen::Login ↔ AppScreen::Main 전환
//   3) 자동 로그인 시도: 저장된 refresh_token 으로 서버 인증
//   4) 백그라운드 스레드 기동: Tracker(미사용 감지) + Sync(서버 전송)
//   5) UI 이벤트 처리: 로그인 / 로그아웃 / 수동 동기화 / 설정 등
//   6) 통계 캐시 갱신: 오늘 미사용 누적시간, 미사용 횟수
//
// 🔗 다른 모듈과의 관계
//   - ui: 화면 렌더링 호출 (render_login / render_status)
//   - tracker: 미사용 감지 스레드 시작
//   - sync: 서버 전송 스레드 시작
//   - api: 로그인 / refresh / logout 호출
//   - db: 인증 기록 / 이벤트 저장
//   - system: 디바이스 정보, 자동실행 등록
//   - permissions: macOS 권한 체크
// =====================================================================

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod api;
mod db;
mod events;
mod permissions;
mod sync;
mod system;
mod tracker;
mod ui;

use crate::api::{ApiClient, LoginRequest, LoginResponse, PolicyResponse};
use crate::db::{AuthRecord, Database, EventContext};
use crate::events::{AgentEvent, EventType};
use crate::tracker::{spawn_tracker, AgentStatus, TrackerState};
use crate::ui::{LoginAction, LoginViewState, StatusAction, StatusViewData};

use chrono::{DateTime, Local, Utc};
use directories::ProjectDirs;
use eframe::egui;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// 현재 표시 중인 화면 상태 (로그인 화면 / 메인 상태 화면)
enum AppScreen {
    Login(LoginViewState),
    Main,
}

/// 앱 전역 상태 객체
/// - eframe::App 트레이트를 구현하여 매 프레임 update() 가 호출됨
/// - 로컬 DB / 트래커 상태 / 인증 정보 / 통계 캐시를 모두 보관
struct PinPleApp {
    screen: AppScreen,
    db: Arc<Database>,
    tracker_state: TrackerState,
    event_rx: Receiver<AgentEvent>,
    event_tx: mpsc::Sender<AgentEvent>,

    // 인증 / 정책
    auth_record: Option<AuthRecord>,
    policy: PolicyResponse,
    login_response: Option<LoginResponse>,

    // 통계 (캐시)
    today_idle_seconds: i64,
    today_idle_count: i64,
    last_stats_refresh: Instant,
    last_sync_at: Option<DateTime<Local>>,
    server_online: bool,

    session_started_at: DateTime<Local>,
    last_perm_check: Instant,
    perm_acc: bool,
    perm_im: bool,

    sync_started: bool,
    tracker_started: bool,
}

impl PinPleApp {
    /// 앱 초기화
    /// - 한글 폰트 등록
    /// - SQLite 로컬 DB 오픈 (데이터 디렉토리에 자동 생성)
    /// - APP_STARTED 이벤트 기록
    /// - 자동로그인 가능 여부 판단 → 가능하면 main 화면, 아니면 login 화면
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        setup_fonts(&cc.egui_ctx);

        let db_path = data_dir().join("agent.db");
        let db = Arc::new(
            Database::open(&db_path).expect("로컬 DB를 열 수 없습니다"),
        );

        let tracker_state = TrackerState::new();
        let (event_tx, event_rx) = mpsc::channel();

        // APP_STARTED 이벤트 기록
        let _ = db.insert_event(
            &AgentEvent::new(EventType::AppStarted, None, None),
            &EventContext::default(),
        );

        // 자동 로그인 시도
        let auth_record = db.load_auth().ok().flatten();
        let mut screen = AppScreen::Login(LoginViewState::default());
        let mut policy = PolicyResponse::default();
        let mut login_response: Option<LoginResponse> = None;

        if let Some(record) = &auth_record {
            if record.auto_login {
                let api = ApiClient::new();
                let token = record.refresh_token.clone().unwrap_or_default();
                let device_id = record.device_id.clone().unwrap_or_default();
                match api.refresh(&token, &device_id) {
                    Ok(resp) => {
                        log::info!("자동로그인 성공: {}", resp.employee_name);
                        let _ = db.insert_event(
                            &AgentEvent::new(EventType::AutoLoginSuccess, None, None),
                            &EventContext::default(),
                        );
                        policy = resp.policy.clone();
                        login_response = Some(resp);
                        screen = AppScreen::Main;
                    }
                    Err(e) => {
                        log::warn!("자동로그인 실패: {}", e);
                        let _ = db.insert_event(
                            &AgentEvent::new(EventType::AutoLoginFailed, None, None),
                            &EventContext::default(),
                        );
                    }
                }
            }
        }

        Self {
            screen,
            db,
            tracker_state,
            event_rx,
            event_tx,
            auth_record,
            policy,
            login_response,
            today_idle_seconds: 0,
            today_idle_count: 0,
            last_stats_refresh: Instant::now() - Duration::from_secs(10),
            last_sync_at: None,
            server_online: true,
            session_started_at: Local::now(),
            last_perm_check: Instant::now(),
            perm_acc: permissions::check_accessibility(),
            perm_im: permissions::check_input_monitoring(),
            sync_started: false,
            tracker_started: false,
        }
    }

    /// 이벤트 저장 시 함께 기록할 컨텍스트(회사/직원/팀/디바이스 ID) 생성
    /// - 로그인 응답이 있으면 그 값을, 없으면 DB에 저장된 마지막 값을 사용
    fn current_event_context(&self) -> EventContext {
        if let Some(login) = &self.login_response {
            EventContext {
                company_id: Some(login.company_id),
                employee_id: Some(login.employee_id),
                team_id: Some(login.team_id),
                device_id: Some(system::device_id()),
            }
        } else if let Some(record) = &self.auth_record {
            EventContext {
                company_id: record.company_id,
                employee_id: record.employee_id,
                team_id: record.team_id,
                device_id: record.device_id.clone(),
            }
        } else {
            EventContext::default()
        }
    }

    /// 로그인 시도 처리 (기획 3-1)
    /// 1) 디바이스 정보 수집 → API 로그인 요청
    /// 2) 성공 시: auth 테이블에 저장 + LOGIN_SUCCESS 이벤트 기록
    /// 3) 자동로그인 체크 시 OS 자동실행 등록
    /// 4) 실패 시: LOGIN_FAILED 이벤트 + 에러 메시지 반환
    fn handle_login(&mut self, login_id: &str, password: &str, auto_login: bool) -> Result<(), String> {
        let api = ApiClient::new();
        let device_id = system::device_id();
        let device_name = system::device_name();
        let req = LoginRequest {
            login_id,
            password,
            device_id: &device_id,
            device_name: &device_name,
            os_type: system::os_type(),
            os_version: &system::os_version(),
            app_version: env!("CARGO_PKG_VERSION"),
        };

        match api.login(&req) {
            Ok(resp) => {
                let now = Utc::now();
                let record = AuthRecord {
                    company_id: Some(resp.company_id),
                    employee_id: Some(resp.employee_id),
                    employee_name: Some(resp.employee_name.clone()),
                    company_name: Some(resp.company_name.clone()),
                    team_id: Some(resp.team_id),
                    team_name: Some(resp.team_name.clone()),
                    access_token: Some(resp.access_token.clone()),
                    refresh_token: Some(resp.refresh_token.clone()),
                    device_id: Some(device_id),
                    device_name: Some(device_name),
                    auto_login,
                    last_login_at: now,
                };
                self.db
                    .save_auth(&record)
                    .map_err(|e| format!("로컬 DB 저장 실패: {}", e))?;

                // 이벤트 기록
                let ctx = EventContext {
                    company_id: Some(resp.company_id),
                    employee_id: Some(resp.employee_id),
                    team_id: Some(resp.team_id),
                    device_id: record.device_id.clone(),
                };
                let _ = self.db.insert_event(
                    &AgentEvent::new(EventType::LoginSuccess, None, None),
                    &ctx,
                );

                self.policy = resp.policy.clone();
                self.auth_record = Some(record);
                self.login_response = Some(resp);
                self.screen = AppScreen::Main;
                self.session_started_at = Local::now();

                // ⚠️ TODO: 자동실행 등록 (auto_login 체크 시)
                if auto_login {
                    let _ = system::register_autostart();
                }

                Ok(())
            }
            Err(e) => {
                let _ = self.db.insert_event(
                    &AgentEvent::new(EventType::LoginFailed, None, None),
                    &EventContext::default(),
                );
                Err(e.to_string())
            }
        }
    }

    /// 로그아웃 처리
    /// - LOGOUT 이벤트 기록 → 서버 logout API 호출 → 로컬 auth 테이블 비움
    /// - 자동실행 등록 해제 → 화면을 로그인 화면으로 전환
    fn handle_logout(&mut self) {
        let ctx = self.current_event_context();
        let _ = self.db.insert_event(
            &AgentEvent::new(EventType::Logout, None, None),
            &ctx,
        );
        if let Some(login) = &self.login_response {
            let _ = ApiClient::new()
                .logout(&system::device_id(), login.employee_id);
        }
        let _ = self.db.clear_auth();
        let _ = system::unregister_autostart();
        self.auth_record = None;
        self.login_response = None;
        self.screen = AppScreen::Login(LoginViewState::default());
    }

    /// 메인 화면 진입 시 백그라운드 스레드를 1회만 기동
    /// - Tracker 스레드: 5초마다 입력 폴링 + 미사용 감지
    /// - Sync 스레드: 60초마다 PENDING 이벤트 서버 전송
    /// - Lock Monitor: PC 잠금/잠금해제 감지 (TODO)
    fn ensure_background_threads(&mut self, ctx: &egui::Context) {
        if !matches!(self.screen, AppScreen::Main) {
            return;
        }
        let event_ctx = self.current_event_context();

        // 이벤트 추적 스레드 시작
        if !self.tracker_started {
            let tx = self.event_tx.clone();
            let state = self.tracker_state.clone();
            let threshold = self.policy.idle_threshold_seconds;
            // 폴링 주기 100ms → 키보드 입력(보통 50~200ms 유지)을 놓치지 않도록 빠르게 폴링
            // (기획서 "5초마다 마지막 입력 시간 확인" 은 Windows GetLastInputInfo 기준이며,
            //  device_query 폴링 방식은 더 짧은 주기가 필요)
            spawn_tracker(state, threshold, Duration::from_millis(100), tx);
            // ⚠️ TODO: 잠금/잠금해제 모니터 활성화
            tracker::spawn_lock_monitor(self.event_tx.clone());
            self.tracker_started = true;
            log::info!("Tracker 스레드 시작 (threshold={}s)", threshold);
        }

        // 동기화 스레드 시작
        if !self.sync_started {
            let interval = Duration::from_secs(self.policy.sync_interval_seconds.max(10) as u64);
            sync::spawn_sync_thread(Arc::clone(&self.db), interval, event_ctx);
            self.sync_started = true;
            log::info!("Sync 스레드 시작 (interval={:?})", interval);
        }

        // 60ms마다 UI 갱신
        let _ = ctx;
    }

    /// Tracker 스레드가 채널로 보낸 이벤트를 한 번에 비우면서 DB에 영구 저장
    /// - 매 프레임마다 호출 (UI 스레드)
    fn drain_events(&mut self) {
        let ctx = self.current_event_context();
        while let Ok(event) = self.event_rx.try_recv() {
            let _ = self.db.insert_event(&event, &ctx);
        }
    }

    /// 메인 화면에 표시할 오늘 통계 캐시를 2초에 한 번씩 갱신
    /// - DB 쿼리 비용을 매 프레임마다 발생시키지 않기 위함
    fn refresh_stats_if_needed(&mut self) {
        if self.last_stats_refresh.elapsed() < Duration::from_secs(2) {
            return;
        }
        self.last_stats_refresh = Instant::now();

        if let Ok(idle_seconds) = self.db.sum_today_idle_seconds() {
            self.today_idle_seconds = idle_seconds;
        }
        if let Ok(count) = self.db.count_today_events("IDLE_STARTED") {
            self.today_idle_count = count;
        }
    }
}

impl eframe::App for PinPleApp {
    /// eframe 메인 루프 - 매 프레임 호출됨
    /// 1) 권한 재확인 (감지 미시작 시)
    /// 2) 백그라운드 스레드 보장
    /// 3) 채널의 신규 이벤트를 DB로 flush
    /// 4) 통계 갱신
    /// 5) 권한 부족 시 상단 경고 패널 표시
    /// 6) 현재 화면(Login/Main) 렌더링 + 액션 처리
    fn update(&mut self, ctx: &egui::Context, _: &mut eframe::Frame) {
        // 권한 주기적 재확인 (감지 미시작 시)
        if !self.tracker_started && self.last_perm_check.elapsed() >= Duration::from_secs(2) {
            self.perm_acc = permissions::check_accessibility();
            self.perm_im = permissions::check_input_monitoring();
            self.last_perm_check = Instant::now();
        }

        self.ensure_background_threads(ctx);
        self.drain_events();
        self.refresh_stats_if_needed();

        // 권한 부족 경고 (전역)
        if !self.perm_acc || !self.perm_im {
            egui::TopBottomPanel::top("perm_warn").show(ctx, |ui| {
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    ui.colored_label(
                        egui::Color32::from_rgb(255, 200, 80),
                        "⚠ 권한 부족 - 키보드/마우스 감지를 위해 권한 부여 필요",
                    );
                    if ui.small_button("입력 모니터링 설정").clicked() {
                        permissions::open_input_monitoring_settings();
                    }
                    if ui.small_button("손쉬운 사용 설정").clicked() {
                        permissions::open_accessibility_settings();
                    }
                    if ui.small_button("재확인").clicked() {
                        self.perm_acc = permissions::check_accessibility();
                        self.perm_im = permissions::check_input_monitoring();
                    }
                });
                ui.add_space(4.0);
            });
        }

        egui::CentralPanel::default().show(ctx, |ui| {
            // 현재 화면 분기
            let screen_action = match &mut self.screen {
                AppScreen::Login(state) => {
                    let action = ui::render_login(ui, state);
                    if matches!(action, LoginAction::Submit) {
                        let id = state.login_id.clone();
                        let pw = state.password.clone();
                        let auto = state.auto_login;
                        if id.trim().is_empty() || pw.is_empty() {
                            state.error_message = Some("아이디와 비밀번호를 입력하세요.".into());
                        } else {
                            state.login_in_progress = true;
                            state.error_message = None;
                            // 동기 호출 (mock 이라 즉시 반환)
                            let result = self.handle_login(&id, &pw, auto);
                            // self.screen 이 변경되었을 수 있어서 재참조
                            if let AppScreen::Login(s) = &mut self.screen {
                                s.login_in_progress = false;
                                if let Err(e) = result {
                                    s.error_message = Some(e);
                                }
                            }
                        }
                    }
                    None
                }
                AppScreen::Main => {
                    let login = self.login_response.clone();
                    let auth = self.auth_record.clone();
                    let employee_name = login
                        .as_ref()
                        .map(|l| l.employee_name.as_str())
                        .or_else(|| auth.as_ref().and_then(|a| a.employee_name.as_deref()))
                        .unwrap_or("-");
                    let company_name = login
                        .as_ref()
                        .map(|l| l.company_name.as_str())
                        .or_else(|| auth.as_ref().and_then(|a| a.company_name.as_deref()))
                        .unwrap_or("-");
                    let team_name = login
                        .as_ref()
                        .map(|l| l.team_name.as_str())
                        .or_else(|| auth.as_ref().and_then(|a| a.team_name.as_deref()))
                        .unwrap_or("-");

                    let status = *self.tracker_state.status.lock().unwrap();
                    let data = StatusViewData {
                        employee_name,
                        company_name,
                        team_name,
                        status,
                        today_idle_seconds: self.today_idle_seconds,
                        today_idle_count: self.today_idle_count,
                        last_sync_at: self.last_sync_at,
                        server_online: self.server_online,
                        policy: &self.policy,
                        session_started_at: self.session_started_at,
                    };
                    Some(ui::render_status(ui, &data))
                }
            };

            if let Some(action) = screen_action {
                match action {
                    StatusAction::SyncNow => {
                        log::info!("수동 동기화 트리거 (TODO: 즉시 sync 호출)");
                        // ⚠️ TODO: sync 스레드에 즉시 실행 신호 보내기
                        self.last_sync_at = Some(Local::now());
                    }
                    StatusAction::Logout => {
                        self.handle_logout();
                    }
                    StatusAction::OpenSettings => {
                        // ⚠️ TODO: 설정 화면 열기 (기획 5-4)
                        log::info!("설정 화면 - TODO");
                    }
                    StatusAction::None => {}
                }
            }
        });

        ctx.request_repaint_after(Duration::from_millis(500));
    }
}

/// 한글 폰트를 시스템 경로에서 자동 로드하여 egui 에 등록
/// - macOS: AppleSDGothicNeo / AppleGothic
/// - Windows: 맑은 고딕(malgun.ttf)
/// - Linux: 나눔고딕 / Noto Sans CJK
fn setup_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    let korean: Option<Vec<u8>> = {
        #[cfg(target_os = "macos")]
        {
            std::fs::read("/System/Library/Fonts/AppleSDGothicNeo.ttc")
                .or_else(|_| std::fs::read("/Library/Fonts/AppleGothic.ttf"))
                .ok()
        }
        #[cfg(target_os = "windows")]
        {
            std::fs::read("C:\\Windows\\Fonts\\malgun.ttf").ok()
        }
        #[cfg(target_os = "linux")]
        {
            std::fs::read("/usr/share/fonts/truetype/nanum/NanumGothic.ttf")
                .or_else(|_| std::fs::read("/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc"))
                .ok()
        }
    };

    if let Some(data) = korean {
        fonts
            .font_data
            .insert("korean".to_owned(), egui::FontData::from_owned(data));
        fonts
            .families
            .entry(egui::FontFamily::Proportional)
            .or_default()
            .insert(0, "korean".to_owned());
        fonts
            .families
            .entry(egui::FontFamily::Monospace)
            .or_default()
            .push("korean".to_owned());
    }
    ctx.set_fonts(fonts);
}

/// OS별 표준 앱 데이터 디렉토리 경로 반환
/// - macOS: ~/Library/Application Support/com.PinPle.PCAgent
/// - Windows: %APPDATA%\PinPle\PCAgent\data
/// - Linux: ~/.local/share/PinPle/PCAgent
fn data_dir() -> PathBuf {
    if let Some(dirs) = ProjectDirs::from("com", "PinPle", "PCAgent") {
        dirs.data_local_dir().to_path_buf()
    } else {
        PathBuf::from(".")
    }
}

/// 프로그램 진입점
/// - env_logger 초기화 (RUST_LOG 환경변수로 레벨 조정 가능)
/// - eframe 윈도우 옵션 설정 후 PinPleApp 인스턴스 실행
fn main() -> eframe::Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
        .init();

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([720.0, 640.0])
            .with_min_inner_size([520.0, 480.0])
            .with_title("핀플 PC 에이전트"),
        ..Default::default()
    };

    eframe::run_native(
        "PinPle PC Agent",
        options,
        Box::new(|cc| Ok(Box::new(PinPleApp::new(cc)))),
    )
}
