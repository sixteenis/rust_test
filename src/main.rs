#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod permissions;

use chrono::Local;
use device_query::{DeviceQuery, DeviceState, Keycode};
use eframe::egui;
use std::collections::VecDeque;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::{Duration, Instant};

#[derive(Clone)]
enum InputMessage {
    KeyPress(String),
    KeyRelease(String),
    Click(String),
    MouseMove(f64, f64),
}

struct LogEntry {
    timestamp: String,
    label: String,
    detail: String,
    color: egui::Color32,
}

struct App {
    rx: Receiver<InputMessage>,
    tx: Sender<InputMessage>,

    listening: bool,

    key_count: u64,
    click_count: u64,
    mouse_x: f64,
    mouse_y: f64,
    last_mouse_log: Option<Instant>,

    events: VecDeque<LogEntry>,
    max_events: usize,

    log_to_file: bool,
    log_file: Option<std::fs::File>,
    log_path: PathBuf,
    auto_scroll: bool,

    perm_acc: bool,
    perm_im: bool,
    last_perm_check: Instant,
}

impl App {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        setup_fonts(&cc.egui_ctx);

        let (tx, rx) = mpsc::channel();
        let log_path = std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join("input_log.txt");

        Self {
            rx,
            tx,
            listening: false,
            key_count: 0,
            click_count: 0,
            mouse_x: 0.0,
            mouse_y: 0.0,
            last_mouse_log: None,
            events: VecDeque::new(),
            max_events: 200,
            log_to_file: false,
            log_file: None,
            log_path,
            auto_scroll: true,
            perm_acc: permissions::check_accessibility(),
            perm_im: permissions::check_input_monitoring(),
            last_perm_check: Instant::now(),
        }
    }

    fn start_listening(&mut self, ctx: egui::Context) {
        if self.listening {
            return;
        }
        self.listening = true;

        let tx = self.tx.clone();

        // device_query 기반 폴링 스레드 (rdev와 달리 메인 스레드 강제 안 함, macOS에서 안정)
        thread::spawn(move || {
            let device_state = DeviceState::new();
            let mut prev_keys: Vec<Keycode> = Vec::new();
            let mut prev_mouse_buttons: Vec<bool> = Vec::new();
            let mut prev_pos: (i32, i32) = (i32::MIN, i32::MIN);
            let mut last_repaint = Instant::now();

            loop {
                // 1) 키 상태
                let keys = device_state.get_keys();
                for key in &keys {
                    if !prev_keys.contains(key) {
                        let _ = tx.send(InputMessage::KeyPress(format!("{:?}", key)));
                    }
                }
                for key in &prev_keys {
                    if !keys.contains(key) {
                        let _ = tx.send(InputMessage::KeyRelease(format!("{:?}", key)));
                    }
                }
                prev_keys = keys;

                // 2) 마우스 상태
                let mouse = device_state.get_mouse();

                for (idx, &pressed) in mouse.button_pressed.iter().enumerate() {
                    let was_pressed = prev_mouse_buttons.get(idx).copied().unwrap_or(false);
                    if pressed && !was_pressed {
                        let name = match idx {
                            1 => "Left",
                            2 => "Right",
                            3 => "Middle",
                            _ => "Other",
                        };
                        let _ = tx.send(InputMessage::Click(name.to_string()));
                    }
                }
                prev_mouse_buttons = mouse.button_pressed.clone();

                // 3) 좌표
                if mouse.coords != prev_pos {
                    let _ = tx.send(InputMessage::MouseMove(
                        mouse.coords.0 as f64,
                        mouse.coords.1 as f64,
                    ));
                    prev_pos = mouse.coords;
                }

                // UI 갱신 트리거 (60ms마다)
                if last_repaint.elapsed() >= Duration::from_millis(60) {
                    ctx.request_repaint();
                    last_repaint = Instant::now();
                }

                thread::sleep(Duration::from_millis(16));
            }
        });
    }

    fn process_messages(&mut self) {
        while let Ok(msg) = self.rx.try_recv() {
            let timestamp = Local::now().format("%H:%M:%S%.3f").to_string();

            let (label, detail, color) = match &msg {
                InputMessage::KeyPress(k) => {
                    self.key_count += 1;
                    ("KEY ↓", k.clone(), egui::Color32::from_rgb(100, 180, 255))
                }
                InputMessage::KeyRelease(k) => {
                    ("KEY ↑", k.clone(), egui::Color32::from_rgb(120, 120, 130))
                }
                InputMessage::Click(b) => {
                    self.click_count += 1;
                    ("CLICK", b.clone(), egui::Color32::from_rgb(120, 220, 140))
                }
                InputMessage::MouseMove(x, y) => {
                    self.mouse_x = *x;
                    self.mouse_y = *y;
                    let now = Instant::now();
                    let should_log = match self.last_mouse_log {
                        Some(prev) => now.duration_since(prev).as_millis() >= 500,
                        None => true,
                    };
                    if !should_log {
                        continue;
                    }
                    self.last_mouse_log = Some(now);
                    (
                        "MOVE",
                        format!("x: {:.0}, y: {:.0}", x, y),
                        egui::Color32::from_rgb(180, 180, 200),
                    )
                }
            };

            if self.log_to_file {
                if self.log_file.is_none() {
                    self.log_file = OpenOptions::new()
                        .create(true)
                        .append(true)
                        .open(&self.log_path)
                        .ok();
                }
                if let Some(f) = self.log_file.as_mut() {
                    let _ = writeln!(f, "[{}] {:8} | {}", timestamp, label, detail);
                    let _ = f.flush();
                }
            }

            self.events.push_front(LogEntry {
                timestamp,
                label: label.to_string(),
                detail,
                color,
            });
            while self.events.len() > self.max_events {
                self.events.pop_back();
            }
        }
    }

    fn refresh_permissions_if_idle(&mut self) {
        if self.listening {
            return;
        }
        if self.last_perm_check.elapsed() >= Duration::from_secs(2) {
            self.perm_acc = permissions::check_accessibility();
            self.perm_im = permissions::check_input_monitoring();
            self.last_perm_check = Instant::now();
        }
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _: &mut eframe::Frame) {
        self.refresh_permissions_if_idle();
        self.process_messages();

        // 상단 상태바
        egui::TopBottomPanel::top("status_bar").show(ctx, |ui| {
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.heading("🎯 Input Tracker");
                ui.separator();

                let acc_color = if self.perm_acc {
                    egui::Color32::from_rgb(120, 220, 140)
                } else {
                    egui::Color32::from_rgb(240, 100, 100)
                };
                ui.colored_label(
                    acc_color,
                    if self.perm_acc { "✓ Accessibility" } else { "✗ Accessibility" },
                );

                ui.separator();

                let im_color = if self.perm_im {
                    egui::Color32::from_rgb(120, 220, 140)
                } else {
                    egui::Color32::from_rgb(240, 100, 100)
                };
                ui.colored_label(
                    im_color,
                    if self.perm_im { "✓ Input Monitoring" } else { "✗ Input Monitoring" },
                );

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if self.listening {
                        ui.colored_label(
                            egui::Color32::from_rgb(120, 220, 140),
                            "● 감지 중",
                        );
                    } else {
                        ui.colored_label(
                            egui::Color32::from_rgb(180, 180, 180),
                            "● 대기 중",
                        );
                    }
                });
            });
            ui.add_space(4.0);
        });

        egui::TopBottomPanel::bottom("bottom_bar").show(ctx, |ui| {
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.label(format!("로그 파일: {}", self.log_path.display()));
            });
            ui.add_space(4.0);
        });

        egui::CentralPanel::default().show(ctx, |ui| {
            // 권한 부족 경고
            if !self.perm_acc || !self.perm_im {
                egui::Frame::group(ui.style())
                    .fill(egui::Color32::from_rgb(60, 30, 30))
                    .show(ui, |ui| {
                        ui.colored_label(
                            egui::Color32::from_rgb(255, 200, 80),
                            "⚠️  필요한 권한이 부족합니다",
                        );
                        ui.label("키보드/마우스 감지를 위해 시스템 권한이 필요합니다. 아래 버튼으로 설정 페이지를 열고, 권한 부여 후 앱을 종료한 뒤 재실행해주세요.");
                        ui.add_space(4.0);
                        ui.horizontal(|ui| {
                            if !self.perm_im && ui.button("📂 입력 모니터링 설정 열기").clicked() {
                                permissions::open_input_monitoring_settings();
                            }
                            if !self.perm_acc && ui.button("📂 손쉬운 사용 설정 열기").clicked() {
                                permissions::open_accessibility_settings();
                            }
                            if ui.button("🔄 권한 다시 확인").clicked() {
                                self.perm_acc = permissions::check_accessibility();
                                self.perm_im = permissions::check_input_monitoring();
                                self.last_perm_check = Instant::now();
                            }
                        });
                    });
                ui.add_space(8.0);
            }

            // 컨트롤 버튼
            ui.horizontal(|ui| {
                let can_start = self.perm_acc && self.perm_im && !self.listening;

                let start_btn = egui::Button::new(if self.listening {
                    "⏺  감지 중..."
                } else {
                    "▶  감지 시작"
                })
                .min_size(egui::vec2(140.0, 32.0));

                if ui.add_enabled(can_start, start_btn).clicked() {
                    self.start_listening(ctx.clone());
                }

                ui.separator();
                ui.checkbox(&mut self.log_to_file, "📄 파일에도 저장");
                ui.checkbox(&mut self.auto_scroll, "⬇ 자동 스크롤");

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("🗑  로그 지우기").clicked() {
                        self.events.clear();
                        self.key_count = 0;
                        self.click_count = 0;
                    }
                });
            });

            ui.add_space(12.0);

            // 통계 카드
            ui.horizontal(|ui| {
                stat_card(ui, "⌨ 키 입력", &self.key_count.to_string());
                stat_card(ui, "🖱 클릭", &self.click_count.to_string());
                stat_card(
                    ui,
                    "📍 좌표",
                    &format!("{:.0}, {:.0}", self.mouse_x, self.mouse_y),
                );
            });

            ui.add_space(12.0);
            ui.separator();
            ui.label(
                egui::RichText::new(format!(
                    "최근 이벤트 ({} / {})",
                    self.events.len(),
                    self.max_events
                ))
                .strong(),
            );
            ui.add_space(4.0);

            let scroll = egui::ScrollArea::vertical().auto_shrink([false; 2]);
            let scroll = if self.auto_scroll {
                scroll.stick_to_bottom(false)
            } else {
                scroll
            };

            scroll.show(ui, |ui| {
                if self.events.is_empty() {
                    ui.weak("입력을 시작하면 여기에 로그가 표시됩니다.");
                } else {
                    for event in &self.events {
                        ui.horizontal(|ui| {
                            ui.colored_label(
                                egui::Color32::from_rgb(140, 140, 150),
                                format!("[{}]", event.timestamp),
                            );
                            ui.colored_label(
                                event.color,
                                egui::RichText::new(&event.label).monospace().strong(),
                            );
                            ui.label(&event.detail);
                        });
                    }
                }
            });
        });

        ctx.request_repaint_after(Duration::from_millis(100));
    }
}

fn stat_card(ui: &mut egui::Ui, title: &str, value: &str) {
    egui::Frame::group(ui.style())
        .fill(egui::Color32::from_rgb(40, 42, 50))
        .show(ui, |ui| {
            ui.set_min_width(150.0);
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
            std::fs::read("C:\\Windows\\Fonts\\malgun.ttf")
                .or_else(|_| std::fs::read("C:\\Windows\\Fonts\\malgunbd.ttf"))
                .ok()
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

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([780.0, 640.0])
            .with_min_inner_size([520.0, 420.0])
            .with_title("Input Tracker"),
        ..Default::default()
    };

    eframe::run_native(
        "Input Tracker",
        options,
        Box::new(|cc| Ok(Box::new(App::new(cc)))),
    )
}
