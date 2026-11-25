//! SF Symbols, drawn by AppKit into small images for marks in the interface.

use objc2::AnyThread;
use objc2_app_kit::{NSBitmapImageRep, NSDeviceRGBColorSpace, NSGraphicsContext, NSImage};
use objc2_foundation::{NSPoint, NSRect, NSSize, NSString};

/// The SF Symbol `name`, `px` pixels tall: white, its shape in the alpha, for tinting.
pub fn symbol(name: &str, px: u32) -> Option<egui::ColorImage> {
    let image = NSImage::imageWithSystemSymbolName_accessibilityDescription(&NSString::from_str(name), None)?;
    let size = image.size();
    let (w, h) = ((px as f64 * size.width / size.height).ceil() as usize, px as usize);
    // SAFETY: the rep allocates its own buffer of four 8-bit samples a pixel, in device RGB.
    let rep = unsafe { NSBitmapImageRep::initWithBitmapDataPlanes_pixelsWide_pixelsHigh_bitsPerSample_samplesPerPixel_hasAlpha_isPlanar_colorSpaceName_bytesPerRow_bitsPerPixel(NSBitmapImageRep::alloc(), std::ptr::null_mut(), w as isize, h as isize, 8, 4, true, false, NSDeviceRGBColorSpace, (w * 4) as isize, 32) }?;
    let context = NSGraphicsContext::graphicsContextWithBitmapImageRep(&rep)?;
    NSGraphicsContext::saveGraphicsState_class();
    NSGraphicsContext::setCurrentContext(Some(&context));
    image.drawInRect(NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(w as f64, h as f64)));
    context.flushGraphics();
    NSGraphicsContext::restoreGraphicsState_class();
    // SAFETY: the buffer holds `w` × `h` pixels of four bytes and lives as long as `rep`.
    let bytes = unsafe { std::slice::from_raw_parts(rep.bitmapData(), w * h * 4) };
    Some(egui::ColorImage { size: [w, h], pixels: bytes.chunks(4).map(|p| egui::Color32::from_white_alpha(p[3])).collect() })
}
