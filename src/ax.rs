use core_foundation::base::TCFType;
use core_foundation::string::CFString;
use core_foundation_sys::array::{CFArrayGetCount, CFArrayGetValueAtIndex};
use core_foundation_sys::base::{CFRelease, CFRetain, CFTypeRef};
use core_foundation_sys::string::CFStringRef;
use core_graphics::geometry::{CGPoint, CGSize};
use objc2_app_kit::NSWorkspace;
use std::ffi::c_void;

type AXUIElementRef = CFTypeRef;
type AXError = i32;

const AX_VALUE_CGPOINT_TYPE: u32 = 1;
const AX_VALUE_CGSIZE_TYPE: u32 = 2;
// ponytail: fixed traversal cap, no cycle guard — real app AX trees don't
// loop; add a visited-set if one ever does.
const MAX_DEPTH: u32 = 25;

#[link(name = "ApplicationServices", kind = "framework")]
extern "C" {
    fn AXIsProcessTrusted() -> bool;
    fn AXUIElementCreateApplication(pid: i32) -> AXUIElementRef;
    fn AXUIElementCopyAttributeValue(
        element: AXUIElementRef,
        attribute: CFStringRef,
        value: *mut CFTypeRef,
    ) -> AXError;
    fn AXValueGetValue(value: CFTypeRef, value_type: u32, value_ptr: *mut c_void) -> bool;
}

pub fn is_trusted() -> bool {
    unsafe { AXIsProcessTrusted() }
}

pub struct UIElement {
    pub text: String,
    position: CGPoint,
    size: CGSize,
}

impl UIElement {
    pub fn center(&self) -> CGPoint {
        CGPoint::new(
            self.position.x + self.size.width / 2.0,
            self.position.y + self.size.height / 2.0,
        )
    }
}

fn copy_attribute(element: AXUIElementRef, name: &str) -> Option<CFTypeRef> {
    let cf_name = CFString::new(name);
    let mut value: CFTypeRef = std::ptr::null();
    let err = unsafe {
        AXUIElementCopyAttributeValue(element, cf_name.as_concrete_TypeRef(), &mut value)
    };
    if err == 0 && !value.is_null() {
        Some(value)
    } else {
        None
    }
}

fn attribute_string(element: AXUIElementRef, name: &str) -> Option<String> {
    let value = copy_attribute(element, name)?;
    Some(unsafe { CFString::wrap_under_create_rule(value as CFStringRef).to_string() })
}

fn attribute_point(element: AXUIElementRef, name: &str) -> Option<CGPoint> {
    let value = copy_attribute(element, name)?;
    let mut point = CGPoint::new(0.0, 0.0);
    let ok = unsafe {
        AXValueGetValue(value, AX_VALUE_CGPOINT_TYPE, &mut point as *mut _ as *mut c_void)
    };
    unsafe { CFRelease(value) };
    ok.then_some(point)
}

fn attribute_size(element: AXUIElementRef, name: &str) -> Option<CGSize> {
    let value = copy_attribute(element, name)?;
    let mut size = CGSize::new(0.0, 0.0);
    let ok = unsafe {
        AXValueGetValue(value, AX_VALUE_CGSIZE_TYPE, &mut size as *mut _ as *mut c_void)
    };
    unsafe { CFRelease(value) };
    ok.then_some(size)
}

fn children(element: AXUIElementRef) -> Vec<AXUIElementRef> {
    let Some(value) = copy_attribute(element, "AXChildren") else {
        return Vec::new();
    };
    let count = unsafe { CFArrayGetCount(value as _) };
    let mut out = Vec::with_capacity(count.max(0) as usize);
    for i in 0..count {
        let child = unsafe { CFArrayGetValueAtIndex(value as _, i) } as AXUIElementRef;
        // Elements borrowed from the array we're about to release — retain
        // each one so it outlives that release.
        unsafe { CFRetain(child) };
        out.push(child);
    }
    unsafe { CFRelease(value) };
    out
}

fn walk(element: AXUIElementRef, depth: u32, out: &mut Vec<UIElement>) {
    if depth > MAX_DEPTH {
        return;
    }

    let text = attribute_string(element, "AXTitle")
        .filter(|s| !s.is_empty())
        .or_else(|| attribute_string(element, "AXValue").filter(|s| !s.is_empty()))
        .or_else(|| attribute_string(element, "AXDescription").filter(|s| !s.is_empty()));

    if let Some(text) = text {
        if let (Some(position), Some(size)) = (
            attribute_point(element, "AXPosition"),
            attribute_size(element, "AXSize"),
        ) {
            out.push(UIElement { text, position, size });
        }
    }

    for child in children(element) {
        walk(child, depth + 1, out);
        unsafe { CFRelease(child) };
    }
}

pub(crate) fn frontmost_pid() -> Result<i32, String> {
    let workspace = NSWorkspace::sharedWorkspace();
    let app = workspace.frontmostApplication().ok_or("no frontmost app")?;
    Ok(app.processIdentifier())
}

pub fn frontmost_app_elements() -> Result<Vec<UIElement>, String> {
    let pid = frontmost_pid()?;
    let app_element = unsafe { AXUIElementCreateApplication(pid) };
    if app_element.is_null() {
        return Err("could not create AX reference for frontmost app".to_string());
    }
    let mut elements = Vec::new();
    walk(app_element, 0, &mut elements);
    unsafe { CFRelease(app_element) };
    Ok(elements)
}

// ponytail: substring/prefix scoring only, no edit-distance — upgrade to
// Levenshtein if plain matches start missing real targets.
pub fn find_best_match<'a>(query: &str, elements: &'a [UIElement]) -> Option<&'a UIElement> {
    let q = query.to_lowercase();
    elements
        .iter()
        .find(|e| e.text.to_lowercase() == q)
        .or_else(|| elements.iter().find(|e| e.text.to_lowercase().starts_with(&q)))
        .or_else(|| elements.iter().find(|e| e.text.to_lowercase().contains(&q)))
}
