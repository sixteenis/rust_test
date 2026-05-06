// =====================================================================
// permissions.rs
// ---------------------------------------------------------------------
// 📌 역할: OS 별 시스템 권한 체크 / 설정 페이지 자동 오픈
//
// 🔧 담당 기능
//   1) macOS:
//        - 손쉬운 사용 (Accessibility) 권한 확인
//        - 입력 모니터링 (Input Monitoring) 권한 확인
//        - 권한 설정 페이지 자동 오픈 (Cmd+, 안 누르고 바로 진입)
//        - IOHIDRequestAccess 로 권한 프롬프트 트리거
//   2) Windows:
//        - 일반 권한으로 글로벌 후킹 가능 (별도 권한 불필요)
//        - 관리자 권한 여부 정도만 확인
//   3) Linux:
//        - /dev/input 접근 가능 여부 확인 (input 그룹 멤버십)
//
// 🔗 사용 위치
//   - main.rs::PinPleApp::update 가 2초마다 권한 재확인
//   - ui 의 권한 부족 경고 패널이 설정 페이지 오픈 함수 호출
// =====================================================================

#![allow(dead_code)]

use std::io::{self, Write};

pub use platform::{
    check_accessibility, check_input_monitoring, open_accessibility_settings,
    open_input_monitoring_settings,
};

#[cfg(target_os = "macos")]
mod platform {
    use std::process::Command;

    #[link(name = "ApplicationServices", kind = "framework")]
    extern "C" {
        fn AXIsProcessTrusted() -> bool;
    }

    #[link(name = "IOKit", kind = "framework")]
    extern "C" {
        fn IOHIDCheckAccess(request: u32) -> u32;
        fn IOHIDRequestAccess(request: u32) -> bool;
    }

    const REQUEST_LISTEN_EVENT: u32 = 1;
    const ACCESS_GRANTED: u32 = 0;

    pub fn check_accessibility() -> bool {
        unsafe { AXIsProcessTrusted() }
    }

    pub fn check_input_monitoring() -> bool {
        unsafe { IOHIDCheckAccess(REQUEST_LISTEN_EVENT) == ACCESS_GRANTED }
    }

    pub fn trigger_input_monitoring_prompt() {
        unsafe {
            IOHIDRequestAccess(REQUEST_LISTEN_EVENT);
        }
    }

    pub fn open_accessibility_settings() {
        let _ = Command::new("open")
            .arg("x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility")
            .spawn();
    }

    pub fn open_input_monitoring_settings() {
        let _ = Command::new("open")
            .arg("x-apple.systempreferences:com.apple.preference.security?Privacy_ListenEvent")
            .spawn();
    }

    pub fn print_status() {
        let acc = check_accessibility();
        let im = check_input_monitoring();
        println!("[권한 상태]");
        println!(
            "  손쉬운 사용 (Accessibility):      {}",
            if acc { "허용 ✓" } else { "거부 ✗" }
        );
        println!(
            "  입력 모니터링 (Input Monitoring): {}",
            if im { "허용 ✓" } else { "거부 ✗" }
        );
    }

    pub fn all_granted() -> bool {
        check_accessibility() && check_input_monitoring()
    }

    pub fn os_name() -> &'static str {
        "macOS"
    }
}

#[cfg(target_os = "windows")]
mod platform {
    use std::process::Command;

    pub fn check_accessibility() -> bool {
        true
    }

    pub fn check_input_monitoring() -> bool {
        true
    }

    pub fn is_admin() -> bool {
        // 현재 토큰의 관리자 권한 여부 확인
        Command::new("net")
            .args(&["session"])
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }

    pub fn trigger_input_monitoring_prompt() {}

    pub fn open_accessibility_settings() {
        let _ = Command::new("cmd")
            .args(&["/C", "start", "ms-settings:easeofaccess"])
            .spawn();
    }

    pub fn open_input_monitoring_settings() {
        let _ = Command::new("cmd")
            .args(&["/C", "start", "ms-settings:privacy"])
            .spawn();
    }

    pub fn print_status() {
        println!("[권한 상태]");
        println!(
            "  관리자 권한:                       {}",
            if is_admin() { "예 ✓" } else { "아니오 (일반 사용자)" }
        );
        println!("  글로벌 후킹:                       자동 허용 ✓");
        println!();
        println!("ℹ️  Windows는 별도 시스템 권한이 필요하지 않습니다.");
        println!("   다만 일부 백신이 키로거로 오인할 수 있으니");
        println!("   백신 예외 처리가 필요할 수 있습니다.");
    }

    pub fn all_granted() -> bool {
        true
    }

    pub fn os_name() -> &'static str {
        "Windows"
    }
}

#[cfg(target_os = "linux")]
mod platform {
    use std::process::Command;

    pub fn check_accessibility() -> bool {
        true
    }

    pub fn check_input_monitoring() -> bool {
        // /dev/input/* 접근 가능 여부로 간이 체크
        std::path::Path::new("/dev/input").exists()
    }

    pub fn trigger_input_monitoring_prompt() {}

    pub fn open_accessibility_settings() {
        let _ = Command::new("xdg-open")
            .arg("settings://privacy")
            .spawn();
    }

    pub fn open_input_monitoring_settings() {
        open_accessibility_settings();
    }

    pub fn print_status() {
        println!("[권한 상태]");
        println!(
            "  /dev/input 접근:                   {}",
            if check_input_monitoring() {
                "가능 ✓"
            } else {
                "불가 ✗"
            }
        );
        println!();
        println!("ℹ️  Linux에서는 사용자가 'input' 그룹에 속해야 합니다.");
        println!("   sudo usermod -a -G input $USER");
    }

    pub fn all_granted() -> bool {
        check_input_monitoring()
    }

    pub fn os_name() -> &'static str {
        "Linux"
    }
}

pub fn ensure_permissions() -> bool {
    println!("===========================================");
    println!("  🔐 [{}] 권한 확인", platform::os_name());
    println!("===========================================");

    platform::print_status();
    println!();

    if platform::all_granted() {
        println!("✅ 모든 권한이 정상입니다. 감지를 시작합니다.");
        println!();
        return true;
    }

    println!("-------------------------------------------");
    println!("⚠️  필요한 권한이 부족합니다.");
    println!("-------------------------------------------");

    #[cfg(target_os = "macos")]
    {
        // 입력 모니터링이 가장 중요 — 우선 처리
        if !platform::check_input_monitoring() {
            println!();
            println!("📋 [입력 모니터링 권한] 부여 안내");
            println!();
            println!("   1. 잠시 후 시스템 설정 창이 자동으로 열립니다.");
            println!("   2. 우측 목록에서 현재 앱(Terminal / iTerm / RustRover)을 찾으세요.");
            println!("   3. 토글을 ON 으로 변경하세요.");
            println!("   4. 앱을 종료 후 다시 실행해주세요.");
            println!();
            wait_enter("Enter 키를 누르면 시스템 설정이 열립니다");

            // macOS에 권한 등록을 트리거 (앱이 목록에 표시되도록)
            platform::trigger_input_monitoring_prompt();
            std::thread::sleep(std::time::Duration::from_millis(300));
            platform::open_input_monitoring_settings();

            println!();
            println!("🚪 권한 부여 후 [앱을 종료 → 재실행] 해주세요.");
            println!("   (macOS는 권한 변경을 실행 중인 프로세스에 즉시 반영하지 않습니다)");
            return false;
        }

        if !platform::check_accessibility() {
            println!();
            println!("📋 [손쉬운 사용 권한] 부여 안내");
            println!();
            println!("   1. 잠시 후 시스템 설정 창이 자동으로 열립니다.");
            println!("   2. 우측 목록에서 현재 앱(Terminal / iTerm / RustRover)을 추가/체크하세요.");
            println!("   3. 토글을 ON 으로 변경하세요.");
            println!("   4. 앱을 종료 후 다시 실행해주세요.");
            println!();
            wait_enter("Enter 키를 누르면 시스템 설정이 열립니다");

            platform::open_accessibility_settings();

            println!();
            println!("🚪 권한 부여 후 [앱을 종료 → 재실행] 해주세요.");
            return false;
        }
    }

    #[cfg(target_os = "linux")]
    {
        if !platform::check_input_monitoring() {
            println!();
            println!("📋 다음 명령으로 input 그룹에 추가 후 재로그인해주세요:");
            println!("     sudo usermod -a -G input $USER");
            println!();
            return false;
        }
    }

    true
}

fn wait_enter(prompt: &str) {
    print!("{} ... ", prompt);
    io::stdout().flush().unwrap();
    let mut buf = String::new();
    let _ = io::stdin().read_line(&mut buf);
}
