// =====================================================================
// system.rs
// ---------------------------------------------------------------------
// 📌 역할: OS 별 시스템 통합 (자동실행 / 트레이 아이콘 / 디바이스 정보)
//
// 🔧 담당 기능
//   1) 자동실행 등록 / 해제 (기획 3-3)
//        - Windows: HKCU\Run 레지스트리
//        - macOS: LaunchAgent plist
//        - Linux: ~/.config/autostart .desktop
//   2) 트레이 아이콘 초기화 (기획 5-3)
//   3) 디바이스 식별자 / 이름 / OS 정보 조회
//
// ⚠️ 현재 상태: 대부분 stub (TODO 주석 참고)
//   - 실제 구현 시 winreg, plist, tray-icon 크레이트 추가 필요
// =====================================================================

#[cfg(target_os = "windows")]
pub fn register_autostart() -> std::io::Result<()> {
    // ⚠️ TODO: 윈도우 시작 프로그램 등록
    //   레지스트리 HKCU\Software\Microsoft\Windows\CurrentVersion\Run 에
    //   현재 실행파일 경로를 등록.
    //   winreg 크레이트 사용 권장:
    //     let hkcu = winreg::RegKey::predef(winreg::enums::HKEY_CURRENT_USER);
    //     let (key, _) = hkcu.create_subkey("Software\\Microsoft\\Windows\\CurrentVersion\\Run")?;
    //     key.set_value("PinPlePcAgent", &exe_path.to_string_lossy().to_string())?;
    //
    //   2차 확장 시 작업 스케줄러 / Windows 서비스 방식으로 변경 가능.
    log::warn!("윈도우 자동실행 등록: TODO (winreg 크레이트로 구현)");
    Ok(())
}

#[cfg(target_os = "windows")]
pub fn unregister_autostart() -> std::io::Result<()> {
    // ⚠️ TODO: 위 키 삭제
    Ok(())
}

#[cfg(target_os = "macos")]
pub fn register_autostart() -> std::io::Result<()> {
    // ⚠️ TODO: macOS LaunchAgent plist 생성
    //   ~/Library/LaunchAgents/com.pinple.pcagent.plist 작성 후 launchctl load
    log::warn!("macOS 자동실행 등록: TODO (LaunchAgent 구현 필요)");
    Ok(())
}

#[cfg(target_os = "macos")]
pub fn unregister_autostart() -> std::io::Result<()> {
    Ok(())
}

#[cfg(target_os = "linux")]
pub fn register_autostart() -> std::io::Result<()> {
    // ⚠️ TODO: ~/.config/autostart/pinple-pc-agent.desktop 작성
    Ok(())
}

#[cfg(target_os = "linux")]
pub fn unregister_autostart() -> std::io::Result<()> {
    Ok(())
}

// 🛎️ 트레이 아이콘 (기획 5-3)
//
// ⚠️ TODO: tray-icon 크레이트로 구현
//   - 현재 상태 보기
//   - 오늘 기록 보기
//   - 동기화 실행
//   - 설정
//   - 로그아웃
//   - 종료 (정책에 따라 비활성화 가능)
//
// eframe 과 트레이 아이콘 통합은 winit 이벤트 루프 공유 필요 → 다음 단계에서 구현.
#[allow(dead_code)]
pub fn init_tray_icon() {
    log::warn!("시스템 트레이 아이콘: TODO (tray-icon 크레이트로 구현)");
}

// ===== 디바이스 정보 =====

/// 디바이스 고유 ID (서버에서 PC 와 직원 계정 연결용)
/// - 현재: hostname 기반 해시 (간이 구현)
/// - TODO: Windows MachineGuid / macOS IOPlatformUUID 사용 권장
pub fn device_id() -> String {
    // 기기 식별용 - macOS hostname + UUID 조합 / Windows 의 MachineGuid 사용 권장
    // ⚠️ TODO: 더 견고한 식별자 (예: Windows registry MachineGuid, macOS IOPlatformUUID)
    let hostname = hostname();
    format!("{}-{:x}", hostname, fxhash(&hostname))
}

/// 사용자에게 표시되는 디바이스 이름 (= 호스트네임)
pub fn device_name() -> String {
    hostname()
}

/// OS 종류 문자열 ("Windows" / "macOS" / "Linux" / "Other")
pub fn os_type() -> &'static str {
    if cfg!(target_os = "windows") {
        "Windows"
    } else if cfg!(target_os = "macos") {
        "macOS"
    } else if cfg!(target_os = "linux") {
        "Linux"
    } else {
        "Other"
    }
}

/// OS 버전 문자열 (예: "Windows 11", "macOS 14.5")
/// - TODO: sys-info 크레이트 또는 OS API 로 정확한 버전 조회
pub fn os_version() -> String {
    "Unknown".to_string()
}

/// 현재 PC 의 호스트네임 조회 (환경변수 → hostname 명령 순으로 시도)
fn hostname() -> String {
    std::env::var("COMPUTERNAME")
        .or_else(|_| std::env::var("HOSTNAME"))
        .or_else(|_| {
            std::process::Command::new("hostname")
                .output()
                .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
                .map_err(|_| std::env::VarError::NotPresent)
        })
        .unwrap_or_else(|_| "unknown-pc".to_string())
}

/// FNV-1a 64bit 해시 (간이 구현 - device_id 생성용)
fn fxhash(s: &str) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in s.bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}
