use crate::frame::Frame;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use block2::RcBlock;
use objc2::rc::{autoreleasepool, Retained};
use objc2::{runtime::AnyObject, AnyThread};
use objc2_core_foundation::{CFData, CFRetained};
use objc2_core_graphics::{
    CGBitmapInfo, CGColorRenderingIntent, CGColorSpace, CGDataProvider, CGImage,
    CGImageAlphaInfo as Alpha, CGImageByteOrderInfo as Order, CGMainDisplayID,
    CGPreflightScreenCaptureAccess,
};
use objc2_foundation::{NSArray, NSDictionary, NSError, NSString};
use objc2_screen_capture_kit::{
    SCCaptureResolutionType, SCContentFilter, SCScreenshotManager, SCShareableContent,
    SCStreamConfiguration, SCWindow,
};
use objc2_vision::{
    VNImageRequestHandler, VNRecognizeTextRequest, VNRequest,
    VNRequestTextRecognitionLevel as Level,
};

pub fn screen_capture_preflight() -> bool {
    CGPreflightScreenCaptureAccess()
}

pub fn text_request_revision() -> usize {
    unsafe { VNRecognizeTextRequest::new().revision() }
}

fn err_text(e: Option<&NSError>) -> String {
    e.map_or("no error object".into(), |e| {
        format!("{} ({} {})", e.localizedDescription(), e.domain(), e.code())
    })
}

fn frame_from_cgimage(img: &CGImage) -> Result<Frame, String> {
    let (i, w, h) = (
        Some(img),
        CGImage::width(Some(img)),
        CGImage::height(Some(img)),
    );
    let (stride, alpha, order) = (
        CGImage::bytes_per_row(i),
        CGImage::alpha_info(i),
        CGImage::byte_order_info(i),
    );
    let alpha_first = [
        Alpha::PremultipliedFirst,
        Alpha::First,
        Alpha::NoneSkipFirst,
    ]
    .contains(&alpha);
    if CGImage::bits_per_pixel(i) != 32
        || CGImage::bits_per_component(i) != 8
        || order != Order::Order32Little
        || !alpha_first
    {
        return Err(format!(
            "not 8-bit BGRA: alphaInfo {}, byteOrderInfo {:#x}",
            alpha.0, order.0
        ));
    }
    let provider = CGImage::data_provider(i).ok_or("CGImageGetDataProvider: NULL")?;
    let data = CGDataProvider::data(Some(&provider)).ok_or("CGDataProviderCopyData: NULL")?;
    let bytes = unsafe { data.as_bytes_unchecked() };
    if w == 0 || h == 0 || stride < w * 4 || bytes.len() < stride * (h - 1) + w * 4 {
        return Err(format!(
            "short data: {} bytes, bytesPerRow {stride}, {w}x{h}",
            bytes.len()
        ));
    }
    let mut bgra = Vec::with_capacity(w * h * 4);
    for row in 0..h {
        bgra.extend_from_slice(&bytes[row * stride..row * stride + w * 4]);
    }
    Ok(Frame {
        width: w,
        height: h,
        bgra,
    })
}

pub fn capture_main_display(timeout_ms: u64) -> Result<Frame, String> {
    let (tx, rx) = mpsc::channel::<Result<Frame, String>>();
    let main_id = CGMainDisplayID();
    let on_content = RcBlock::new(move |content: *mut SCShareableContent, err: *mut NSError| {
        let Some(content) = (unsafe { content.as_ref() }) else {
            let _ = tx.send(Err(format!(
                "SCShareableContent: {}",
                err_text(unsafe { err.as_ref() })
            )));
            return;
        };
        let displays = unsafe { content.displays() };
        let Some(display) = (0..displays.count())
            .map(|n| displays.objectAtIndex(n))
            .min_by_key(|d| unsafe { d.displayID() } != main_id)
        else {
            let _ = tx.send(Err("SCShareableContent lists no display".into()));
            return;
        };
        let filter = unsafe {
            SCContentFilter::initWithDisplay_excludingWindows(
                SCContentFilter::alloc(),
                &display,
                &NSArray::<SCWindow>::new(),
            )
        };
        let (rect, scale) = unsafe { (filter.contentRect(), f64::from(filter.pointPixelScale())) };
        let config = unsafe { SCStreamConfiguration::new() };
        unsafe {
            config.setWidth((rect.size.width * scale).round() as usize);
            config.setHeight((rect.size.height * scale).round() as usize);
            config.setShowsCursor(false);
            config.setCaptureResolution(SCCaptureResolutionType::Best);
            config.setPixelFormat(u32::from_be_bytes(*b"BGRA"));
        }
        let tx_image = tx.clone();
        let on_image = RcBlock::new(move |image: *mut CGImage, err: *mut NSError| {
            let _ = tx_image.send(match unsafe { image.as_ref() } {
                Some(image) => frame_from_cgimage(image),
                None => Err(format!(
                    "captureImage: {}",
                    err_text(unsafe { err.as_ref() })
                )),
            });
        });
        unsafe {
            SCScreenshotManager::captureImageWithFilter_configuration_completionHandler(
                &filter,
                &config,
                Some(&on_image),
            )
        };
    });
    unsafe { SCShareableContent::getShareableContentWithCompletionHandler(&on_content) };
    rx.recv_timeout(Duration::from_millis(timeout_ms))
        .map_err(|e| format!("no completion handler result within {timeout_ms} ms: {e}"))?
}

pub fn cgimage_from_bgra(frame: &Frame) -> Result<CFRetained<CGImage>, String> {
    let stride = frame.width * 4;
    if frame.bgra.is_empty() || frame.bgra.len() != stride * frame.height {
        return Err(format!(
            "{}x{} frame holds {} bytes",
            frame.width,
            frame.height,
            frame.bgra.len()
        ));
    }
    let provider = CGDataProvider::with_cf_data(Some(&CFData::from_bytes(&frame.bgra)))
        .ok_or("CGDataProviderCreateWithCFData: NULL")?;
    let space = CGColorSpace::new_device_rgb().ok_or("CGColorSpaceCreateDeviceRGB: NULL")?;
    let info = CGBitmapInfo(Alpha::NoneSkipFirst.0 | Order::Order32Little.0);
    let intent = CGColorRenderingIntent::RenderingIntentDefault;
    unsafe {
        CGImage::new(
            frame.width,
            frame.height,
            8,
            32,
            stride,
            Some(&space),
            info,
            Some(&provider),
            std::ptr::null(),
            false,
            intent,
        )
    }
    .ok_or_else(|| "CGImageCreate: NULL".to_string())
}

pub fn recognize(
    frame: &Frame,
    accurate: bool,
    correction: bool,
    languages: &[&str],
) -> Result<(Vec<String>, f64), String> {
    let image = cgimage_from_bgra(frame)?;
    autoreleasepool(|_| {
        let request = VNRecognizeTextRequest::new();
        request.setRecognitionLevel(if accurate {
            Level::Accurate
        } else {
            Level::Fast
        });
        request.setUsesLanguageCorrection(correction);
        if !languages.is_empty() {
            let tags: Vec<Retained<NSString>> = languages
                .iter()
                .map(|tag| NSString::from_str(tag))
                .collect();
            request.setRecognitionLanguages(&NSArray::from_retained_slice(&tags));
        }
        let options = NSDictionary::<NSString, AnyObject>::new();
        let handler = unsafe {
            VNImageRequestHandler::initWithCGImage_options(
                VNImageRequestHandler::alloc(),
                &image,
                &options,
            )
        };
        let as_request: &VNRequest = &request;
        let requests = NSArray::from_slice(&[as_request]);
        let started = Instant::now();
        let outcome = handler.performRequests_error(&requests);
        let elapsed_ms = started.elapsed().as_secs_f64() * 1000.0;
        outcome.map_err(|e| format!("performRequests: {}", err_text(Some(&e))))?;
        let mut lines = Vec::new();
        if let Some(results) = request.results() {
            for n in 0..results.count() {
                let top = results.objectAtIndex(n).topCandidates(1);
                lines.push(if top.count() == 0 {
                    String::new()
                } else {
                    top.objectAtIndex(0).string().to_string()
                });
            }
        }
        Ok((lines, elapsed_ms))
    })
}
