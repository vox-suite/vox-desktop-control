use crate::ax;
use core_graphics::geometry::CGPoint;
use objc2::rc::Retained;
use objc2::AnyThread;
use objc2_core_foundation::CFData;
use objc2_core_graphics::{
    CGBitmapInfo, CGColorRenderingIntent, CGColorSpace, CGDataProvider, CGImage, CGImageAlphaInfo,
};
use objc2_foundation::{NSArray, NSDictionary};
use objc2_vision::{VNImageRequestHandler, VNRecognizeTextRequest, VNRequest};
use xcap::Window;

pub struct OcrHit {
    pub text: String,
    pub center: CGPoint,
}

fn focused_window() -> Result<Window, String> {
    let pid = ax::frontmost_pid()?;
    Window::all()
        .map_err(|e| e.to_string())?
        .into_iter()
        .find(|w| w.pid().map(|p| p as i32).unwrap_or(-1) == pid)
        .ok_or_else(|| "no window found for frontmost app".to_string())
}

fn rgba_to_cgimage(
    image: &image::RgbaImage,
) -> Result<objc2_core_foundation::CFRetained<CGImage>, String> {
    let width = image.width() as usize;
    let height = image.height() as usize;
    let data = CFData::from_bytes(image.as_raw());
    let provider =
        CGDataProvider::with_cf_data(Some(&data)).ok_or("failed to create CGDataProvider")?;
    let color_space = CGColorSpace::new_device_rgb().ok_or("failed to create color space")?;
    let bitmap_info = CGBitmapInfo(CGImageAlphaInfo::Last.0);
    unsafe {
        CGImage::new(
            width,
            height,
            8,
            32,
            width * 4,
            Some(&color_space),
            bitmap_info,
            Some(&provider),
            std::ptr::null(),
            false,
            CGColorRenderingIntent::RenderingIntentDefault,
        )
    }
    .ok_or_else(|| "CGImageCreate failed".to_string())
}

fn recognize_text(
    cg_image: &CGImage,
) -> Result<Vec<(String, objc2_core_foundation::CGRect)>, String> {
    let options = NSDictionary::new();
    let handler = unsafe {
        VNImageRequestHandler::initWithCGImage_options(
            VNImageRequestHandler::alloc(),
            cg_image,
            &options,
        )
    };
    let request = VNRecognizeTextRequest::new();
    let as_request: Retained<VNRequest> =
        Retained::into_super(Retained::into_super(request.clone()));
    let requests: Retained<NSArray<VNRequest>> = NSArray::from_retained_slice(&[as_request]);
    handler
        .performRequests_error(&requests)
        .map_err(|e| e.to_string())?;

    let observations = request.results().unwrap_or_default();
    let mut hits = Vec::with_capacity(observations.len());
    for observation in observations.iter() {
        let candidates = observation.topCandidates(1);
        let Some(best) = candidates.iter().next() else {
            continue;
        };
        let text = best.string().to_string();
        if text.is_empty() {
            continue;
        }
        let bbox = unsafe { observation.boundingBox() };
        hits.push((text, bbox));
    }
    Ok(hits)
}

// ponytail: scale derived from capture-vs-window size ratio instead of
// querying NSScreen backingScaleFactor directly — fine as long as xcap keeps
// returning the full backing-store pixels for the window's own monitor.
pub fn scan_frontmost_window() -> Result<Vec<OcrHit>, String> {
    let window = focused_window()?;
    let image = window.capture_image().map_err(|e| e.to_string())?;
    let (px_width, px_height) = (image.width() as f64, image.height() as f64);
    let win_x = window.x().map_err(|e| e.to_string())? as f64;
    let win_y = window.y().map_err(|e| e.to_string())? as f64;
    let win_width = (window.width().map_err(|e| e.to_string())? as f64).max(1.0);
    let win_height = (window.height().map_err(|e| e.to_string())? as f64).max(1.0);
    let scale_x = px_width / win_width;
    let scale_y = px_height / win_height;

    let cg_image = rgba_to_cgimage(&image)?;
    let observations = recognize_text(&cg_image)?;

    let mut hits = Vec::with_capacity(observations.len());
    for (text, bbox) in observations {
        // Vision's boundingBox is normalized [0,1], origin bottom-left.
        let px_x = bbox.origin.x * px_width;
        let px_w = bbox.size.width * px_width;
        let px_y_top = (1.0 - bbox.origin.y - bbox.size.height) * px_height;
        let px_h = bbox.size.height * px_height;
        let center = CGPoint::new(
            win_x + (px_x + px_w / 2.0) / scale_x,
            win_y + (px_y_top + px_h / 2.0) / scale_y,
        );
        hits.push(OcrHit { text, center });
    }
    Ok(hits)
}

pub fn find_best_match<'a>(query: &str, hits: &'a [OcrHit]) -> Option<&'a OcrHit> {
    let q = query.to_lowercase();
    hits.iter()
        .find(|h| h.text.to_lowercase() == q)
        .or_else(|| hits.iter().find(|h| h.text.to_lowercase().contains(&q)))
}
