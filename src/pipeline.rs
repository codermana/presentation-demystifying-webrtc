use std::{
    ptr,
    ptr::NonNull,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc, Arc, Mutex,
    },
    time::Duration,
};

use block2::RcBlock;
use dispatch2::{DispatchQueue, DispatchQueueAttr};
use objc2::rc::Retained;
use objc2::runtime::{NSObject, ProtocolObject};
use objc2::{define_class, msg_send, AnyThread, DefinedClass};
use objc2_core_foundation::{
    CFBoolean, CFDictionary, CFNumber, CFRetained, CFString, CFType, Type,
};
use objc2_core_media::{kCMTimeInvalid, kCMVideoCodecType_H264, CMSampleBuffer, CMTime};
use objc2_core_video::{kCVPixelFormatType_32BGRA, CVImageBuffer};
use objc2_foundation::{NSArray, NSError, NSObjectProtocol};
use objc2_screen_capture_kit::{
    SCContentFilter, SCDisplay, SCShareableContent, SCStream, SCStreamConfiguration,
    SCStreamOutput, SCStreamOutputType,
};
use objc2_video_toolbox::{
    kVTCompressionPropertyKey_AllowFrameReordering, kVTCompressionPropertyKey_AverageBitRate,
    kVTCompressionPropertyKey_ExpectedFrameRate, kVTCompressionPropertyKey_MaxFrameDelayCount,
    kVTCompressionPropertyKey_MaxKeyFrameInterval, kVTCompressionPropertyKey_ProfileLevel,
    kVTCompressionPropertyKey_RealTime, kVTProfileLevel_H264_Baseline_AutoLevel,
    kVTPropertyNotSupportedErr, kVTVideoEncoderSpecification_EnableHardwareAcceleratedVideoEncoder,
    VTCompressionSession, VTEncodeInfoFlags, VTSessionSetProperty,
};
use tokio::sync::broadcast;

use crate::h264::{sample_buffer_to_access_unit, EncodedAccessUnit};

const SCREEN_CAPTURE_KIT_TIMEOUT: Duration = Duration::from_secs(30);
const TARGET_FPS: i32 = 15;
const STARTUP_CHANNEL_CAPACITY: usize = 2;
const MAX_STREAM_WIDTH: i32 = 1280;
const TARGET_AVERAGE_BITRATE_BPS: i32 = 5_000_000;

#[derive(Clone)]
pub struct MacVideoPipeline {
    encoded_tx: broadcast::Sender<EncodedAccessUnit>,
    stats: Arc<PipelineStats>,
    latest_access_unit: Arc<Mutex<Option<EncodedAccessUnit>>>,
}

#[derive(Default)]
struct PipelineStats {
    pipeline_ready: AtomicBool,
    raw_frames: AtomicU64,
    encode_attempts: AtomicU64,
    encoded_frames: AtomicU64,
    dropped_frames: AtomicU64,
    last_raw_frame_at_ms: AtomicU64,
    last_encode_attempt_at_ms: AtomicU64,
    last_encoded_at_ms: AtomicU64,
    last_error: Mutex<Option<String>>,
}

pub struct PipelineHealth {
    pub pipeline_ready: bool,
    pub raw_frames: u64,
    pub encode_attempts: u64,
    pub encoded_frames: u64,
    pub dropped_frames: u64,
    pub last_raw_frame_at_ms: u64,
    pub last_encode_attempt_at_ms: u64,
    pub last_encoded_at_ms: u64,
    pub last_error: Option<String>,
}

pub struct PipelineConfig {
    pub display_id: u32,
    pub fps: i32,
    pub max_width: i32,
    pub target_average_bitrate_bps: i32,
}

impl PipelineConfig {
    fn from_env() -> Self {
        Self {
            display_id: env_i32("POC_DISPLAY_ID", 0).max(0) as u32,
            fps: env_i32("POC_FPS", TARGET_FPS).max(1),
            max_width: env_i32("POC_MAX_WIDTH", MAX_STREAM_WIDTH).max(320),
            target_average_bitrate_bps: env_i32(
                "POC_TARGET_BITRATE_BPS",
                TARGET_AVERAGE_BITRATE_BPS,
            )
            .max(500_000),
        }
    }
}

impl MacVideoPipeline {
    pub fn start_default() -> Result<Self, String> {
        Self::start(PipelineConfig::from_env())
    }

    pub fn start(config: PipelineConfig) -> Result<Self, String> {
        let (encoded_tx, _) = broadcast::channel::<EncodedAccessUnit>(STARTUP_CHANNEL_CAPACITY);
        let stats = Arc::new(PipelineStats::default());
        let latest_access_unit = Arc::new(Mutex::new(None));
        spawn_native_pipeline_thread(
            config,
            encoded_tx.clone(),
            Arc::clone(&stats),
            Arc::clone(&latest_access_unit),
        );
        Ok(Self {
            encoded_tx,
            stats,
            latest_access_unit,
        })
    }

    pub fn subscribe(
        &self,
    ) -> (
        broadcast::Receiver<EncodedAccessUnit>,
        Option<EncodedAccessUnit>,
    ) {
        let snapshot = self
            .latest_access_unit
            .lock()
            .map(|guard| guard.clone())
            .unwrap_or_else(|poisoned| poisoned.into_inner().clone());
        (self.encoded_tx.subscribe(), snapshot)
    }

    pub fn health(&self) -> PipelineHealth {
        PipelineHealth {
            pipeline_ready: self.stats.pipeline_ready.load(Ordering::Relaxed),
            raw_frames: self.stats.raw_frames.load(Ordering::Relaxed),
            encode_attempts: self.stats.encode_attempts.load(Ordering::Relaxed),
            encoded_frames: self.stats.encoded_frames.load(Ordering::Relaxed),
            dropped_frames: self.stats.dropped_frames.load(Ordering::Relaxed),
            last_raw_frame_at_ms: self.stats.last_raw_frame_at_ms.load(Ordering::Relaxed),
            last_encode_attempt_at_ms: self.stats.last_encode_attempt_at_ms.load(Ordering::Relaxed),
            last_encoded_at_ms: self.stats.last_encoded_at_ms.load(Ordering::Relaxed),
            last_error: self
                .stats
                .last_error
                .lock()
                .map(|guard| guard.clone())
                .unwrap_or_else(|poisoned| poisoned.into_inner().clone()),
        }
    }
}

fn spawn_native_pipeline_thread(
    config: PipelineConfig,
    encoded_tx: broadcast::Sender<EncodedAccessUnit>,
    stats: Arc<PipelineStats>,
    latest_access_unit: Arc<Mutex<Option<EncodedAccessUnit>>>,
) {
    std::thread::spawn(move || {
        if let Err(err) = run_native_pipeline(
            config,
            encoded_tx,
            Arc::clone(&stats),
            Arc::clone(&latest_access_unit),
        ) {
            stats.pipeline_ready.store(false, Ordering::Relaxed);
            if let Ok(mut last_error) = stats.last_error.lock() {
                *last_error = Some(err);
            }
        }
    });
}

fn run_native_pipeline(
    config: PipelineConfig,
    encoded_tx: broadcast::Sender<EncodedAccessUnit>,
    stats: Arc<PipelineStats>,
    latest_access_unit: Arc<Mutex<Option<EncodedAccessUnit>>>,
) -> Result<(), String> {
    let shareable_content = load_shareable_content()?;
    let display = selected_display(&shareable_content, config.display_id)?;
    let filter = unsafe {
        SCContentFilter::initWithDisplay_excludingWindows(
            SCContentFilter::alloc(),
            &display,
            &NSArray::from_slice(&[]),
        )
    };
    let stream_config = stream_configuration(&filter, config.fps, config.max_width);
    let content_rect = unsafe { filter.contentRect() };
    let scale = unsafe { filter.pointPixelScale() }.max(1.0);
    let native_width = ((content_rect.size.width * scale as f64).round() as i32).max(1);
    let native_height = ((content_rect.size.height * scale as f64).round() as i32).max(1);
    let (width, height) = scaled_dimensions(native_width, native_height, config.max_width);
    let encoder = VideoToolboxEncoder::new(
        width,
        height,
        config.fps,
        config.target_average_bitrate_bps,
        encoded_tx,
        Arc::clone(&stats),
        Arc::clone(&latest_access_unit),
    )?;
    let (sample_tx, sample_rx) = mpsc::sync_channel::<usize>(2);

    let output = FrameStreamOutput::new(sample_tx, Arc::clone(&stats));
    let queue = DispatchQueue::new(
        "com.algogrit.mirrorpad.poc.macos-webrtc-h264.capture",
        DispatchQueueAttr::SERIAL,
    );
    let stream = unsafe {
        SCStream::initWithFilter_configuration_delegate(
            SCStream::alloc(),
            &filter,
            &stream_config,
            None,
        )
    };
    let output_object = ProtocolObject::from_ref(&*output);

    unsafe {
        stream
            .addStreamOutput_type_sampleHandlerQueue_error(
                output_object,
                SCStreamOutputType::Screen,
                Some(queue.as_ref()),
            )
            .map_err(|err| format!("SCStream add output failed: {}", err.localizedDescription()))?;
    }

    start_stream_capture(&stream)?;
    stats.pipeline_ready.store(true, Ordering::Relaxed);
    if let Ok(mut last_error) = stats.last_error.lock() {
        *last_error = None;
    }
    let mut frame_index = 0_i64;
    loop {
        let sample_buffer_ptr = match sample_rx.recv() {
            Ok(sample_buffer_ptr) => sample_buffer_ptr,
            Err(_) => return Err("sample buffer channel closed".to_string()),
        };
        let Some(sample_buffer) = cfr_retained_from_usize::<CMSampleBuffer>(sample_buffer_ptr)
        else {
            continue;
        };
        let Some(image_buffer) = (unsafe { sample_buffer.image_buffer() }) else {
            continue;
        };

        if let Err(err) = encoder.encode_image_buffer(image_buffer.as_ref(), frame_index) {
            if let Ok(mut last_error) = stats.last_error.lock() {
                *last_error = Some(err);
            }
        }
        frame_index = frame_index.saturating_add(1);
    }
}

struct VideoToolboxEncoder {
    session: Retained<VTCompressionSession>,
    output_ctx_ptr: *mut EncoderOutputContext,
    stats: Arc<PipelineStats>,
    fps: i32,
}

struct EncoderOutputContext {
    encoded_tx: broadcast::Sender<EncodedAccessUnit>,
    stats: Arc<PipelineStats>,
    latest_access_unit: Arc<Mutex<Option<EncodedAccessUnit>>>,
    frame_duration: Duration,
}

impl VideoToolboxEncoder {
    fn new(
        width: i32,
        height: i32,
        fps: i32,
        target_average_bitrate_bps: i32,
        encoded_tx: broadcast::Sender<EncodedAccessUnit>,
        stats: Arc<PipelineStats>,
        latest_access_unit: Arc<Mutex<Option<EncodedAccessUnit>>>,
    ) -> Result<Self, String> {
        let encoder_spec_keys: [&CFString; 1] =
            [unsafe { kVTVideoEncoderSpecification_EnableHardwareAcceleratedVideoEncoder }];
        let encoder_spec_values: [&CFType; 1] = [CFBoolean::new(true).as_ref()];
        let encoder_spec =
            CFDictionary::<CFString, CFType>::from_slices(&encoder_spec_keys, &encoder_spec_values);

        let mut session_out = ptr::null_mut();
        let output_ctx = Box::new(EncoderOutputContext {
            encoded_tx,
            stats: Arc::clone(&stats),
            latest_access_unit,
            frame_duration: Duration::from_millis((1000 / fps.max(1)) as u64),
        });
        let output_ctx_ptr = Box::into_raw(output_ctx);
        let status = unsafe {
            VTCompressionSession::create(
                None,
                width,
                height,
                kCMVideoCodecType_H264,
                Some(encoder_spec.as_ref()),
                None,
                None,
                Some(video_toolbox_output_callback),
                output_ctx_ptr.cast(),
                ptr::NonNull::new(&mut session_out).expect("session_out pointer"),
            )
        };
        if status != 0 {
            unsafe {
                drop(Box::from_raw(output_ctx_ptr));
            }
            return Err(format!("VTCompressionSessionCreate failed: {status}"));
        }

        let session = unsafe { Retained::from_raw(session_out) }
            .ok_or_else(|| "VTCompressionSessionCreate returned null session".to_string())?;

        set_required_session_property(
            session.as_ref(),
            "RealTime",
            unsafe { kVTCompressionPropertyKey_RealTime },
            CFBoolean::new(true).as_ref(),
        )?;
        set_required_session_property(
            session.as_ref(),
            "AllowFrameReordering",
            unsafe { kVTCompressionPropertyKey_AllowFrameReordering },
            CFBoolean::new(false).as_ref(),
        )?;
        set_optional_session_property(
            session.as_ref(),
            "ProfileLevel",
            unsafe { kVTCompressionPropertyKey_ProfileLevel },
            unsafe { kVTProfileLevel_H264_Baseline_AutoLevel.as_ref() },
        );
        let fps_number = CFNumber::new_i32(fps);
        set_optional_session_property(
            session.as_ref(),
            "ExpectedFrameRate",
            unsafe { kVTCompressionPropertyKey_ExpectedFrameRate },
            fps_number.as_ref(),
        );
        let average_bitrate = CFNumber::new_i32(target_average_bitrate_bps);
        set_optional_session_property(
            session.as_ref(),
            "AverageBitRate",
            unsafe { kVTCompressionPropertyKey_AverageBitRate },
            average_bitrate.as_ref(),
        );
        let max_frame_delay = CFNumber::new_i32(1);
        set_optional_session_property(
            session.as_ref(),
            "MaxFrameDelayCount",
            unsafe { kVTCompressionPropertyKey_MaxFrameDelayCount },
            max_frame_delay.as_ref(),
        );
        let keyframe_interval = CFNumber::new_i32((fps * 2).max(fps));
        set_optional_session_property(
            session.as_ref(),
            "MaxKeyFrameInterval",
            unsafe { kVTCompressionPropertyKey_MaxKeyFrameInterval },
            keyframe_interval.as_ref(),
        );

        let prepare_status = unsafe { session.prepare_to_encode_frames() };
        if prepare_status != 0 {
            return Err(format!(
                "VTCompressionSessionPrepareToEncodeFrames failed: {prepare_status}"
            ));
        }

        Ok(Self {
            session,
            output_ctx_ptr,
            stats,
            fps,
        })
    }

    fn encode_image_buffer(
        &self,
        image_buffer: &CVImageBuffer,
        frame_index: i64,
    ) -> Result<(), String> {
        self.stats.encode_attempts.fetch_add(1, Ordering::Relaxed);
        self.stats
            .last_encode_attempt_at_ms
            .store(now_unix_ms(), Ordering::Relaxed);
        let pts = unsafe { CMTime::new(frame_index, self.fps.max(1)) };
        let duration = unsafe { CMTime::new(1, self.fps.max(1)) };
        let mut info_flags = VTEncodeInfoFlags::empty();
        let status = unsafe {
            self.session.encode_frame(
                image_buffer,
                pts,
                duration,
                None,
                ptr::null_mut(),
                &mut info_flags,
            )
        };
        if status != 0 {
            return Err(format!("VTCompressionSessionEncodeFrame failed: {status}"));
        }

        Ok(())
    }
}

impl Drop for VideoToolboxEncoder {
    fn drop(&mut self) {
        unsafe {
            let _ = self.session.complete_frames(kCMTimeInvalid);
            self.session.invalidate();
            drop(Box::from_raw(self.output_ctx_ptr));
        }
    }
}

unsafe extern "C-unwind" fn video_toolbox_output_callback(
    output_callback_ref_con: *mut std::ffi::c_void,
    _source_frame_ref_con: *mut std::ffi::c_void,
    status: i32,
    info_flags: VTEncodeInfoFlags,
    sample_buffer: *mut CMSampleBuffer,
) {
    let output_ctx = (output_callback_ref_con as *mut EncoderOutputContext).as_ref();
    let Some(output_ctx) = output_ctx else {
        return;
    };

    if status != 0
        || info_flags.contains(VTEncodeInfoFlags::FrameDropped)
        || sample_buffer.is_null()
    {
        output_ctx
            .stats
            .dropped_frames
            .fetch_add(1, Ordering::Relaxed);
        if let Ok(mut last_error) = output_ctx.stats.last_error.lock() {
            *last_error = Some(format!(
                "VideoToolbox encode callback failed: status={status}"
            ));
        }
        return;
    }

    let sample_buffer = &*sample_buffer;
    match sample_buffer_to_access_unit(sample_buffer, output_ctx.frame_duration) {
        Ok(access_unit) => {
            if let Ok(mut latest_access_unit) = output_ctx.latest_access_unit.lock() {
                *latest_access_unit = Some(access_unit.clone());
            }
            let _ = output_ctx.encoded_tx.send(access_unit);
            output_ctx
                .stats
                .encoded_frames
                .fetch_add(1, Ordering::Relaxed);
            output_ctx
                .stats
                .last_encoded_at_ms
                .store(now_unix_ms(), Ordering::Relaxed);
        }
        Err(err) => {
            if let Ok(mut last_error) = output_ctx.stats.last_error.lock() {
                *last_error = Some(err);
            }
        }
    }
}

fn now_unix_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

struct FrameStreamOutputIvars {
    sample_tx: mpsc::SyncSender<usize>,
    stats: Arc<PipelineStats>,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = AnyThread]
    #[ivars = FrameStreamOutputIvars]
    struct FrameStreamOutput;

    unsafe impl NSObjectProtocol for FrameStreamOutput {}

    unsafe impl SCStreamOutput for FrameStreamOutput {
        #[allow(non_snake_case)]
        #[unsafe(method(stream:didOutputSampleBuffer:ofType:))]
        fn stream_didOutputSampleBuffer_ofType(
            &self,
            _stream: &SCStream,
            sample_buffer: &CMSampleBuffer,
            output_type: SCStreamOutputType,
        ) {
            if output_type != SCStreamOutputType::Screen {
                return;
            }

            if unsafe { !sample_buffer.data_is_ready() } {
                return;
            }
            self.ivars()
                .stats
                .raw_frames
                .fetch_add(1, Ordering::Relaxed);
            self.ivars()
                .stats
                .last_raw_frame_at_ms
                .store(now_unix_ms(), Ordering::Relaxed);

            let sample_buffer_ptr = CFRetained::into_raw(sample_buffer.retain()).as_ptr() as usize;
            if self.ivars().sample_tx.try_send(sample_buffer_ptr).is_err() {
                self.ivars()
                    .stats
                    .dropped_frames
                    .fetch_add(1, Ordering::Relaxed);
                drop(cfr_retained_from_usize::<CMSampleBuffer>(sample_buffer_ptr));
            }
        }
    }
);

impl FrameStreamOutput {
    fn new(sample_tx: mpsc::SyncSender<usize>, stats: Arc<PipelineStats>) -> Retained<Self> {
        let this = Self::alloc().set_ivars(FrameStreamOutputIvars { sample_tx, stats });
        unsafe { msg_send![super(this), init] }
    }
}

fn set_required_session_property(
    session: &VTCompressionSession,
    label: &'static str,
    key: &CFString,
    value: &CFType,
) -> Result<(), String> {
    let status = unsafe { VTSessionSetProperty(session.as_ref(), key, Some(value)) };
    if status != 0 {
        return Err(format!("VTSessionSetProperty failed for {label}: {status}"));
    }
    Ok(())
}

fn set_optional_session_property(
    session: &VTCompressionSession,
    label: &'static str,
    key: &CFString,
    value: &CFType,
) {
    let status = unsafe { VTSessionSetProperty(session.as_ref(), key, Some(value)) };
    if status == 0 {
        return;
    }

    if status == kVTPropertyNotSupportedErr {
        eprintln!("macos-webrtc-h264: VideoToolbox property not supported, skipping {label}");
        return;
    }

    eprintln!("macos-webrtc-h264: VTSessionSetProperty failed for optional {label}: {status}");
}

fn load_shareable_content() -> Result<Retained<SCShareableContent>, String> {
    let (tx, rx) = mpsc::sync_channel::<(usize, usize)>(1);
    let completion = RcBlock::new(move |content: *mut SCShareableContent, err: *mut NSError| {
        let _ = tx.send((retain_ptr(content), retain_ptr(err)));
    });

    unsafe {
        SCShareableContent::getShareableContentExcludingDesktopWindows_onScreenWindowsOnly_completionHandler(
            false,
            true,
            &completion,
        );
    }

    let (content_ptr, error_ptr) = rx
        .recv_timeout(SCREEN_CAPTURE_KIT_TIMEOUT)
        .map_err(|_| {
            "SCShareableContent request timed out; macOS may still be waiting on screen-capture readiness"
                .to_string()
        })?;
    if error_ptr != 0 {
        drop_retained::<NSError>(error_ptr);
        return Err("SCShareableContent request failed".to_string());
    }

    retained_from_usize::<SCShareableContent>(content_ptr)
        .ok_or_else(|| "SCShareableContent returned null".to_string())
}

fn selected_display(
    shareable_content: &SCShareableContent,
    selected_display_id: u32,
) -> Result<Retained<SCDisplay>, String> {
    let displays = unsafe { shareable_content.displays() }.to_vec();
    if displays.is_empty() {
        return Err("ScreenCaptureKit found no displays".to_string());
    }

    if selected_display_id != 0 {
        for display in &displays {
            if unsafe { display.displayID() } == selected_display_id {
                return Ok(display.clone());
            }
        }
    }

    displays
        .into_iter()
        .next()
        .ok_or_else(|| "ScreenCaptureKit found no usable display".to_string())
}

fn stream_configuration(
    filter: &SCContentFilter,
    fps: i32,
    max_width: i32,
) -> Retained<SCStreamConfiguration> {
    let config = unsafe { SCStreamConfiguration::new() };
    let content_rect = unsafe { filter.contentRect() };
    let scale = unsafe { filter.pointPixelScale() }.max(1.0);
    let native_width = ((content_rect.size.width * scale as f64).round() as i32).max(1);
    let native_height = ((content_rect.size.height * scale as f64).round() as i32).max(1);
    let (width, height) = scaled_dimensions(native_width, native_height, max_width);

    unsafe {
        config.setWidth(width as usize);
        config.setHeight(height as usize);
        config.setShowsCursor(true);
        config.setPixelFormat(kCVPixelFormatType_32BGRA);
        config.setQueueDepth(2);
        config.setMinimumFrameInterval(CMTime::new(1, fps.max(1)));
    }

    config
}

fn scaled_dimensions(width: i32, height: i32, max_width: i32) -> (i32, i32) {
    if width <= max_width {
        return (width.max(1), height.max(1));
    }

    let scaled_height = ((height as i64 * max_width as i64) / width as i64) as i32;
    (max_width.max(1), scaled_height.max(1))
}

fn start_stream_capture(stream: &SCStream) -> Result<(), String> {
    let (tx, rx) = mpsc::sync_channel::<usize>(1);
    let completion = RcBlock::new(move |err: *mut NSError| {
        let _ = tx.send(retain_ptr(err));
    });

    unsafe {
        stream.startCaptureWithCompletionHandler(Some(&completion));
    }

    let error_ptr = rx.recv_timeout(SCREEN_CAPTURE_KIT_TIMEOUT).map_err(|_| {
        "SCStream start timed out; check macOS screen-recording permission and capture approval"
            .to_string()
    })?;
    if error_ptr != 0 {
        drop_retained::<NSError>(error_ptr);
        return Err("SCStream start failed".to_string());
    }

    Ok(())
}

fn retain_ptr<T: objc2::Message>(ptr: *mut T) -> usize {
    unsafe { Retained::retain(ptr) }
        .map(Retained::into_raw)
        .map_or(0, |retained| retained as usize)
}

fn retained_from_usize<T: objc2::Message>(value: usize) -> Option<Retained<T>> {
    unsafe { Retained::from_raw(value as *mut T) }
}

fn drop_retained<T: objc2::Message>(value: usize) {
    let _ = retained_from_usize::<T>(value);
}

fn cfr_retained_from_usize<T: Type>(value: usize) -> Option<CFRetained<T>> {
    let ptr = NonNull::new(value as *mut T)?;
    Some(unsafe { CFRetained::from_raw(ptr) })
}

fn env_i32(name: &str, default: i32) -> i32 {
    std::env::var(name)
        .ok()
        .and_then(|value| value.parse::<i32>().ok())
        .unwrap_or(default)
}
