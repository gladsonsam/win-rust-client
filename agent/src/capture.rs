//! Screen Capture Module – xcap (demand-driven)
//!
//! The capture loop only runs while the server has active MJPEG viewers.
//! When the server wants to start streaming it sends `{"type":"start_capture"}`
//! over the control WebSocket; when the last viewer disconnects it sends
//! `{"type":"stop_capture"}`.
//!
//! [`start_capture`] spawns the OS thread and returns an [`Arc<AtomicBool>`]
//! stop flag.  Setting that flag to `true` causes the thread to exit cleanly
//! after its current frame.

use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::time::Duration;

use image::{codecs::jpeg::JpegEncoder, ExtendedColorType, ImageEncoder};
use tokio::sync::mpsc;
use tokio::sync::mpsc::error::TrySendError;
use tracing::{error, info, warn};
use xcap::Monitor;

// ─────────────────────────────────────────────────────────────────────────────

/// Spawn the capture loop on a dedicated OS thread; return its stop flag.
///
/// Frames are JPEG-encoded at 40 % quality and sent on `tx` at ~5 fps.
/// Setting `stop` to `true` causes the thread to exit after the current frame.
///
/// When `tx` is closed (channel dropped) the thread also exits automatically.
pub fn start_capture(tx: mpsc::Sender<Vec<u8>>, stop: Arc<AtomicBool>) -> anyhow::Result<()> {
    std::thread::Builder::new()
        .name("screen-capture".into())
        .spawn(move || {
            let monitor = match Monitor::all()
                .ok()
                .and_then(|ms| ms.into_iter().find(|m| m.is_primary().unwrap_or(false)))
            {
                Some(m) => m,
                None => {
                    error!("Screen capture: no primary monitor found.");
                    return;
                }
            };

            info!(
                "Screen capture started: {}×{}",
                monitor.width().unwrap_or(0),
                monitor.height().unwrap_or(0),
            );

            loop {
                // Check stop flag first so we exit promptly.
                if stop.load(Ordering::Relaxed) {
                    info!("Screen capture stopped on demand.");
                    break;
                }

                match monitor.capture_image() {
                    Ok(rgba_img) => {
                        let rgb = image::DynamicImage::ImageRgba8(rgba_img).into_rgb8();

                        let mut jpeg_data: Vec<u8> = Vec::new();
                        let encoder = JpegEncoder::new_with_quality(&mut jpeg_data, 40);

                        match encoder.write_image(
                            rgb.as_raw(),
                            rgb.width(),
                            rgb.height(),
                            ExtendedColorType::Rgb8,
                        ) {
                            Err(e) => warn!("JPEG encode error (skipping): {e}"),
                            Ok(()) => match tx.try_send(jpeg_data) {
                                Ok(()) => {}
                                Err(TrySendError::Full(_)) => {
                                    // Consumer busy – drop stale frame.
                                }
                                Err(TrySendError::Closed(_)) => {
                                    info!("Frame channel closed; stopping capture.");
                                    break;
                                }
                            },
                        }
                    }
                    Err(e) => warn!("Screen capture error (skipping): {e}"),
                }

                // 5 fps — adequate for remote viewing, negligible CPU.
                std::thread::sleep(Duration::from_millis(200));
            }
        })
        .map_err(|e| anyhow::anyhow!("Failed to spawn capture thread: {e}"))?;

    Ok(())
}
