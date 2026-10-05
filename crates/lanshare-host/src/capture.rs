// Captura de tela do Host via Windows Graphics Capture (WGC).

use std::error::Error;
use std::sync::mpsc::{self, Receiver, SyncSender, TrySendError};
use std::time::{Duration, Instant};

use windows_capture::capture::{CaptureControl, Context, GraphicsCaptureApiHandler};
use windows_capture::frame::Frame;
use windows_capture::graphics_capture_api::InternalCaptureControl;
use windows_capture::monitor::Monitor;
use windows_capture::settings::{
    ColorFormat, CursorCaptureSettings, DirtyRegionSettings, DrawBorderSettings,
    MinimumUpdateIntervalSettings, SecondaryWindowSettings, Settings,
};

type BoxError = Box<dyn Error + Send + Sync>;

/// Um frame da tela em memória: pixels RGBA, 4 bytes por pixel, sem padding.
pub struct CapturedFrame {
    pub width: u32,
    pub height: u32,
    pub data: Vec<u8>,
}

// Roda dentro da thread de captura e recebe cada frame novo.
struct Handler {
    sender: SyncSender<CapturedFrame>,
    scratch: Vec<u8>, // buffer reaproveitado para remover o padding
}

impl GraphicsCaptureApiHandler for Handler {
    type Flags = SyncSender<CapturedFrame>;
    type Error = BoxError;

    fn new(ctx: Context<Self::Flags>) -> Result<Self, Self::Error> {
        Ok(Self {
            sender: ctx.flags,
            scratch: Vec::new(),
        })
    }

    fn on_frame_arrived(
        &mut self,
        frame: &mut Frame,
        capture_control: InternalCaptureControl,
    ) -> Result<(), Self::Error> {
        let width = frame.width();
        let height = frame.height();

        let buffer = frame.buffer()?;
        let data = buffer.as_nopadding_buffer(&mut self.scratch).to_vec();

        match self.sender.try_send(CapturedFrame { width, height, data }) {
            Ok(()) => {}
            Err(TrySendError::Full(_)) => {} // consumidor lento: descarta o frame
            Err(TrySendError::Disconnected(_)) => {
                capture_control.stop(); // ninguém está ouvindo mais
            }
        }
        Ok(())
    }
}

/// Captura em andamento. Os frames chegam em `frames`.
pub struct ScreenCapture {
    control: CaptureControl<Handler, BoxError>,
    pub frames: Receiver<CapturedFrame>,
}

impl ScreenCapture {
    /// Começa a capturar o monitor principal em outra thread.
    pub fn start() -> Result<Self, BoxError> {
        let monitor = Monitor::primary()?;
        let (sender, frames) = mpsc::sync_channel(2);

        let settings = Settings::new(
            monitor,
            CursorCaptureSettings::Default,
            DrawBorderSettings::Default,
            SecondaryWindowSettings::Default,
            MinimumUpdateIntervalSettings::Default,
            DirtyRegionSettings::Default,
            ColorFormat::Rgba8,
            sender,
        );

        let control = Handler::start_free_threaded(settings)?;
        Ok(Self { control, frames })
    }

    /// Para a captura.
    pub fn stop(self) -> Result<(), BoxError> {
        self.control.stop()?;
        Ok(())
    }
}

/// Teste temporário: captura por alguns segundos e mostra o resultado no terminal.
pub fn test_capture(seconds: u64) -> Result<(), BoxError> {
    let capture = ScreenCapture::start()?;
    let start = Instant::now();
    let mut count: u32 = 0;
    let mut size = (0, 0);
    let mut bytes = 0;

    while start.elapsed() < Duration::from_secs(seconds) {
        if let Ok(frame) = capture.frames.recv_timeout(Duration::from_millis(500)) {
            count += 1;
            size = (frame.width, frame.height);
            bytes = frame.data.len();
        }
    }

    capture.stop()?;

    println!("Resolução capturada: {}x{}", size.0, size.1);
    println!("Tamanho de 1 frame: {:.2} MB", bytes as f64 / 1_048_576.0);
    println!(
        "Frames recebidos: {} em {}s (~{:.1} FPS)",
        count,
        seconds,
        count as f64 / seconds as f64
    );
    Ok(())
}