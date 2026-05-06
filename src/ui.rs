// =====================================================================
// ui.rs
// ---------------------------------------------------------------------
// 📌 역할: 화면 렌더링 함수 모음 (egui 기반)
//
// 🔧 담당 기능
//   1) 로그인 화면 렌더링 (기획 5-1)
//        - 아이디/비밀번호/자동로그인 체크박스
//        - Submit 액션 반환 → main.rs 가 인증 처리
//   2) 메인 상태 화면 렌더링 (기획 5-2)
//        - 사용자 정보 / 현재 상태 인디케이터
//        - 통계 카드 (오늘 사용 시작 / 미사용 누적 / 미사용 횟수)
//        - 동기화 / 로그아웃 / 설정 버튼
//        - 정책 정보 (접을 수 있는 패널)
//   3) 헬퍼: stat_card, format_duration
//
// 🔗 사용 위치
//   - main.rs::PinPleApp::update 가 매 프레임 호출
// =====================================================================

use crate::api::PolicyResponse;
use crate::tracker::AgentStatus;
use chrono::{DateTime, Local};
use eframe::egui;

/// 로그인 화면 상태 (입력값 / 에러 메시지 / 진행 플래그)
/// - 매 프레임 동일 인스턴스가 사용됨 (TextEdit 의 mutable borrow 대상)
#[derive(Default)]
pub struct LoginViewState {
    pub login_id: String,
    pub password: String,
    pub auto_login: bool,
    pub error_message: Option<String>,
    pub login_in_progress: bool,
}

/// 로그인 화면에서 발생한 사용자 액션
/// - main.rs 가 이 값을 받아 실제 인증 처리 수행
pub enum LoginAction {
    None,
    Submit,
}

/// 로그인 화면을 그린다.
/// - 사용자가 [로그인] 버튼 클릭 또는 Enter 입력 시 LoginAction::Submit 반환
/// - main.rs 가 이 액션을 받아 ApiClient::login 호출 후 결과 처리
pub fn render_login(ui: &mut egui::Ui, state: &mut LoginViewState) -> LoginAction {
    let mut action = LoginAction::None;

    ui.add_space(40.0);

    // 핀플 로고 자리
    ui.vertical_centered(|ui| {
        ui.heading(
            egui::RichText::new("🎯 핀플 PC 에이전트")
                .size(28.0)
                .color(egui::Color32::from_rgb(120, 200, 255)),
        );
        ui.add_space(8.0);
        ui.label(
            egui::RichText::new("핀플 근로자 계정으로 로그인하세요.")
                .color(egui::Color32::from_rgb(200, 200, 210)),
        );
        ui.label(
            egui::RichText::new("로그인 후 PC 사용시간과 미사용 시간이 근무기록 보완 데이터로 기록됩니다.")
                .small()
                .color(egui::Color32::from_rgb(160, 160, 170)),
        );
    });

    ui.add_space(32.0);

    egui::Frame::group(ui.style())
        .fill(egui::Color32::from_rgb(40, 42, 50))
        .inner_margin(egui::Margin::same(20.0))
        .show(ui, |ui| {
            ui.set_width(380.0);
            ui.vertical(|ui| {
                ui.label("아이디");
                let id_resp = ui.add(
                    egui::TextEdit::singleline(&mut state.login_id)
                        .desired_width(f32::INFINITY)
                        .hint_text("worker@example"),
                );

                ui.add_space(8.0);
                ui.label("비밀번호");
                let pw_resp = ui.add(
                    egui::TextEdit::singleline(&mut state.password)
                        .password(true)
                        .desired_width(f32::INFINITY),
                );

                ui.add_space(8.0);
                ui.checkbox(&mut state.auto_login, "자동로그인 유지");

                if let Some(err) = &state.error_message {
                    ui.add_space(6.0);
                    ui.colored_label(egui::Color32::from_rgb(240, 100, 100), err);
                }

                ui.add_space(12.0);
                let btn_text = if state.login_in_progress {
                    "로그인 중..."
                } else {
                    "로그인"
                };
                let btn = egui::Button::new(btn_text).min_size(egui::vec2(0.0, 36.0));
                let clicked = ui
                    .add_enabled(!state.login_in_progress, btn)
                    .clicked();

                let enter_pressed = (id_resp.lost_focus() || pw_resp.lost_focus())
                    && ui.input(|i| i.key_pressed(egui::Key::Enter));

                if clicked || enter_pressed {
                    action = LoginAction::Submit;
                }

                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if ui.link("비밀번호 찾기").clicked() {
                        // ⚠️ TODO: 비밀번호 찾기 페이지 연결
                        log::info!("비밀번호 찾기 클릭 - TODO");
                    }
                    ui.with_layout(
                        egui::Layout::right_to_left(egui::Align::Center),
                        |ui| {
                            ui.weak(format!("v{}", env!("CARGO_PKG_VERSION")));
                        },
                    );
                });
            });
        });

    action
}

/// 메인 상태 화면에 표시할 모든 데이터를 한 번에 전달하는 DTO
/// - 메인 화면은 read-only 표시 위주 → &데이터 형태로 충분
pub struct StatusViewData<'a> {
    pub employee_name: &'a str,
    pub company_name: &'a str,
    pub team_name: &'a str,
    pub status: AgentStatus,
    pub today_idle_seconds: i64,
    pub today_idle_count: i64,
    pub last_sync_at: Option<DateTime<Local>>,
    pub server_online: bool,
    pub policy: &'a PolicyResponse,
    pub session_started_at: DateTime<Local>,
}

/// 메인 상태 화면에서 발생할 수 있는 액션
/// - SyncNow      : 즉시 동기화 트리거 (TODO: sync 스레드 신호)
/// - Logout       : 로그아웃 → 로컬 인증 정보 삭제 + 로그인 화면
/// - OpenSettings : 설정 화면 진입 (기획 5-4 - 추후 구현)
pub enum StatusAction {
    None,
    SyncNow,
    Logout,
    OpenSettings,
}

/// 메인 상태 화면을 그린다.
/// - 사용자가 동기화 / 로그아웃 / 설정 버튼 클릭 시 해당 StatusAction 반환
pub fn render_status(ui: &mut egui::Ui, data: &StatusViewData) -> StatusAction {
    let mut action = StatusAction::None;

    // 사용자 정보
    egui::Frame::group(ui.style())
        .fill(egui::Color32::from_rgb(40, 42, 50))
        .inner_margin(egui::Margin::same(16.0))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.label(
                        egui::RichText::new(format!("👤 {}", data.employee_name))
                            .size(18.0)
                            .strong(),
                    );
                    ui.label(
                        egui::RichText::new(format!("{} · {}", data.company_name, data.team_name))
                            .color(egui::Color32::from_rgb(180, 180, 200))
                            .small(),
                    );
                });

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let (color, label) = match data.status {
                        AgentStatus::Active => (egui::Color32::from_rgb(120, 220, 140), "● 사용 중"),
                        AgentStatus::IdlePending => {
                            (egui::Color32::from_rgb(255, 200, 80), "● 미사용 후보")
                        }
                        AgentStatus::Idle => (egui::Color32::from_rgb(255, 150, 80), "● 미사용"),
                        AgentStatus::Locked => {
                            (egui::Color32::from_rgb(150, 150, 220), "● PC 잠금")
                        }
                        AgentStatus::Offline => {
                            (egui::Color32::from_rgb(200, 100, 100), "● 서버 연결 대기")
                        }
                        AgentStatus::Error => (egui::Color32::from_rgb(240, 100, 100), "● 오류"),
                    };
                    ui.colored_label(color, egui::RichText::new(label).size(16.0).strong());
                });
            });
        });

    ui.add_space(12.0);

    // 통계 카드
    ui.horizontal(|ui| {
        stat_card(
            ui,
            "🕐 오늘 PC 사용 시작",
            &data.session_started_at.format("%H:%M:%S").to_string(),
        );
        stat_card(
            ui,
            "💤 오늘 미사용 누적",
            &format_duration(data.today_idle_seconds),
        );
        stat_card(
            ui,
            "🔁 미사용 횟수",
            &format!("{} 회", data.today_idle_count),
        );
    });

    ui.add_space(12.0);

    // 동기화/서버 정보
    egui::Frame::group(ui.style())
        .inner_margin(egui::Margin::same(12.0))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                let server_label = if data.server_online {
                    egui::RichText::new("✓ 서버 연결됨")
                        .color(egui::Color32::from_rgb(120, 220, 140))
                } else {
                    egui::RichText::new("✗ 서버 연결 끊김 (재시도 중)")
                        .color(egui::Color32::from_rgb(240, 150, 100))
                };
                ui.label(server_label);

                ui.separator();
                let sync_text = match data.last_sync_at {
                    Some(t) => format!("마지막 동기화: {}", t.format("%H:%M:%S")),
                    None => "아직 동기화되지 않음".to_string(),
                };
                ui.label(egui::RichText::new(sync_text).small());
            });
        });

    ui.add_space(12.0);

    // 정책 정보
    egui::CollapsingHeader::new("⚙ 적용된 정책 (관리자 설정)")
        .default_open(false)
        .show(ui, |ui| {
            ui.label(format!(
                "• 미사용 판단 기준: {}",
                format_duration(data.policy.idle_threshold_seconds)
            ));
            ui.label(format!(
                "• 장기 이탈 기준: {}",
                format_duration(data.policy.long_idle_threshold_seconds)
            ));
            ui.label(format!(
                "• 동기화 주기: {}초",
                data.policy.sync_interval_seconds
            ));
            ui.label(format!(
                "• 종료 허용: {}",
                if data.policy.allow_exit { "예" } else { "아니오" }
            ));
            ui.label(format!(
                "• 근무시간만 감지: {}",
                if data.policy.track_only_worktime { "예" } else { "아니오" }
            ));
            ui.label(format!(
                "• PC 잠금 기록: {}",
                if data.policy.track_lock_unlock { "예" } else { "아니오" }
            ));
        });

    ui.add_space(12.0);

    // 액션 버튼
    ui.horizontal(|ui| {
        if ui
            .add(egui::Button::new("🔄 지금 동기화").min_size(egui::vec2(120.0, 32.0)))
            .clicked()
        {
            action = StatusAction::SyncNow;
        }
        if ui
            .add(egui::Button::new("⚙ 설정").min_size(egui::vec2(80.0, 32.0)))
            .clicked()
        {
            action = StatusAction::OpenSettings;
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui
                .add(egui::Button::new("🚪 로그아웃").min_size(egui::vec2(100.0, 32.0)))
                .clicked()
            {
                action = StatusAction::Logout;
            }
        });
    });

    ui.add_space(12.0);
    ui.separator();
    ui.label(
        egui::RichText::new(
            "ℹ 키보드/마우스 입력 내용은 절대 저장되지 않습니다. 입력 발생 여부와 미사용 시간만 기록됩니다.",
        )
        .small()
        .color(egui::Color32::from_rgb(160, 160, 180)),
    );

    action
}

/// 통계 카드 한 칸을 그리는 헬퍼 (제목 + 큰 숫자 형태)
fn stat_card(ui: &mut egui::Ui, title: &str, value: &str) {
    egui::Frame::group(ui.style())
        .fill(egui::Color32::from_rgb(40, 42, 50))
        .show(ui, |ui| {
            ui.set_min_width(170.0);
            ui.vertical(|ui| {
                ui.label(
                    egui::RichText::new(title)
                        .color(egui::Color32::from_rgb(180, 180, 200))
                        .small(),
                );
                ui.add_space(4.0);
                ui.label(
                    egui::RichText::new(value)
                        .color(egui::Color32::from_rgb(120, 200, 255))
                        .heading(),
                );
            });
        });
}

/// 초 단위 시간을 한글 가독형식으로 변환 (UI 표시용)
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
