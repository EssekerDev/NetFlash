//! macOS NSStatusItem: keep the dot in full color (never a template mask).

#![cfg(target_os = "macos")]

use objc2_app_kit::NSCellImagePosition;
use objc2_foundation::MainThreadMarker;
use tray_icon::TrayIcon;

/// Re-apply `isTemplate = NO` after `setImage`. Some macOS versions ignore the
/// flag unless it is set on the image currently attached to the button.
pub fn force_color_image(tray: &TrayIcon) {
    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    let Some(item) = tray.ns_status_item() else {
        return;
    };
    let Some(button) = item.button(mtm) else {
        return;
    };
    if let Some(image) = button.image() {
        image.setTemplate(false);
        button.setImage(Some(&image));
        button.setImagePosition(NSCellImagePosition::ImageOnly);
    }
}
