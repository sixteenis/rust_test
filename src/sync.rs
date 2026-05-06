// =====================================================================
// sync.rs
// ---------------------------------------------------------------------
// 📌 역할: 로컬 DB 의 PENDING 이벤트를 서버로 일괄 전송 (기획 3-8)
//
// 🔧 담당 기능
//   1) 별도 OS 스레드에서 무한 루프 실행
//   2) 정해진 주기(기본 60초)마다 PENDING 이벤트 100개씩 조회
//   3) ApiClient::send_events 로 배치 전송
//   4) 결과에 따라 sync_status = SUCCESS / FAILED 업데이트
//   5) 인터넷 끊김 → PENDING 유지 → 다음 주기에 자동 재시도
//
// 🔗 사용 위치
//   - main.rs::ensure_background_threads 에서 1회 spawn
//
// ⚠️ 주의
//   - 토큰 만료 시 refresh 호출 후 재전송 로직은 추후 추가 필요
// =====================================================================

use crate::api::{ApiClient, EventBatchRequest};
use crate::db::{Database, EventContext};
use crate::events::SyncStatus;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

/// 동기화 백그라운드 스레드 시작
/// - 파라미터:
///     db        : 로컬 DB 핸들 (Arc 로 thread-safe 공유)
///     interval  : 전송 주기 (예: 60초)
///     ctx       : 이벤트와 함께 보낼 회사/직원/팀/디바이스 ID
/// - 반환값: JoinHandle (현재는 누수 허용 - 앱 종료 시 OS 가 정리)
pub fn spawn_sync_thread(
    db: Arc<Database>,
    interval: Duration,
    ctx: EventContext,
) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        let api = ApiClient::new();
        loop {
            thread::sleep(interval);

            let pending = match db.pending_events(100) {
                Ok(p) => p,
                Err(e) => {
                    log::error!("pending_events 조회 실패: {}", e);
                    continue;
                }
            };

            if pending.is_empty() {
                continue;
            }

            let ids: Vec<i64> = pending.iter().map(|(id, _)| *id).collect();
            let events: Vec<_> = pending.iter().map(|(_, e)| e.clone()).collect();

            // ⚠️ TODO: 실제 서버 연결 시 access_token 헤더 추가 + 토큰 만료 시 refresh 호출
            let req = EventBatchRequest {
                device_id: ctx.device_id.as_deref().unwrap_or(""),
                employee_id: ctx.employee_id.unwrap_or(0),
                company_id: ctx.company_id.unwrap_or(0),
                events: &events,
            };

            match api.send_events(&req) {
                Ok(resp) => {
                    log::info!("이벤트 전송 성공: {}건", resp.received_count);
                    let _ = db.mark_events(&ids, SyncStatus::Success);
                }
                Err(e) => {
                    log::warn!("이벤트 전송 실패: {} (재시도 대기)", e);
                    // 인터넷 끊김 / 일시 오류는 PENDING 유지가 자연스럽지만
                    // 너무 많이 실패한 건은 FAILED 처리 가능 (정책에 따라 조정)
                    let _ = db.mark_events(&ids, SyncStatus::Failed);
                }
            }
        }
    })
}
