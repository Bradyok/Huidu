/// NTP time synchronization service.
/// Periodically syncs system clock via NTP.
use tokio::time::{self, Duration};
use tokio_util::sync::CancellationToken;
use tracing::{debug, info, warn};

pub struct TimeSyncService;

impl TimeSyncService {
    /// Run NTP sync in background (every 6 hours).
    /// `ntp_server` is the configured NTP server (e.g. "pool.ntp.org").
    /// Exits when `cancel` is triggered.
    pub async fn run(ntp_server: String, cancel: CancellationToken) {
        // Initial sync after 10 seconds
        tokio::select! {
            biased;
            _ = cancel.cancelled() => return,
            _ = time::sleep(Duration::from_secs(10)) => {}
        }
        Self::sync_once(&ntp_server).await;

        // Then every 6 hours
        let mut interval = time::interval(Duration::from_secs(6 * 3600));
        loop {
            tokio::select! {
                biased;
                _ = cancel.cancelled() => break,
                _ = interval.tick() => {}
            }
            Self::sync_once(&ntp_server).await;
        }
    }

    async fn sync_once(ntp_server: &str) {
        debug!("Attempting NTP time sync via {}", ntp_server);

        // On Linux, try ntpdate or systemctl
        #[cfg(unix)]
        {
            let result = tokio::process::Command::new("ntpdate")
                .args(["-u", ntp_server])
                .output()
                .await;

            match result {
                Ok(output) if output.status.success() => {
                    info!("NTP sync successful");
                }
                Ok(output) => {
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    debug!("ntpdate failed: {}", stderr);
                    // Try timedatectl as fallback
                    let _ = tokio::process::Command::new("timedatectl")
                        .args(["set-ntp", "true"])
                        .output()
                        .await;
                }
                Err(_) => {
                    debug!("ntpdate not available");
                }
            }
        }

        // On Windows, time sync is handled by the OS
        #[cfg(windows)]
        {
            debug!("NTP sync skipped on Windows (OS handles it)");
        }
    }

    /// Manually set device time (from SetTimeInfo SDK command)
    pub async fn set_time(datetime: &str) {
        info!("Setting device time to: {}", datetime);

        #[cfg(unix)]
        {
            let _ = tokio::process::Command::new("date")
                .args(["-s", datetime])
                .output()
                .await;
        }

        #[cfg(windows)]
        {
            warn!("Cannot set system time on Windows without admin privileges");
        }
    }
}
