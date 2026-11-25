//! Photos the Finder asks the app to open: with Open With, by dropping them on the Dock
//! icon, or by opening them where the app is the default. They arrive as the open documents
//! Apple event. NSApplication installs a handler for it while launching, one that winit
//! leaves unanswered, so the app installs its own when launching is about to finish: late
//! enough to replace NSApplication's, early enough for the photos the app was launched for.

use block2::RcBlock;
use objc2::rc::Retained;
use objc2::{define_class, msg_send, sel, MainThreadOnly};
use objc2_app_kit::NSApplicationWillFinishLaunchingNotification;
use objc2_foundation::{MainThreadMarker, NSAppleEventDescriptor, NSAppleEventManager, NSNotification, NSNotificationCenter, NSObject, NSObjectProtocol};
use std::path::PathBuf;
use std::ptr::NonNull;
use std::sync::{Mutex, OnceLock};

/// An Apple event code from its four characters.
const fn code(c: &[u8; 4]) -> u32 {
    u32::from_be_bytes(*c)
}

static ARRIVED: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());
static WAKE: OnceLock<egui::Context> = OnceLock::new();

define_class!(
    // SAFETY: NSObject has no subclassing requirements, and `Opener` does not implement Drop.
    #[unsafe(super = NSObject)]
    #[thread_kind = MainThreadOnly]
    struct Opener;

    // SAFETY: NSObjectProtocol has no safety requirements.
    unsafe impl NSObjectProtocol for Opener {}

    impl Opener {
        // SAFETY: the signature is the one NSAppleEventManager calls its handlers with.
        #[unsafe(method(handleOpenDocuments:withReplyEvent:))]
        fn open(&self, event: &NSAppleEventDescriptor, _reply: &NSAppleEventDescriptor) {
            let Some(list) = event.paramDescriptorForKeyword(code(b"----")) else { return };
            let paths = (1..=list.numberOfItems()).filter_map(|i| list.descriptorAtIndex(i)?.fileURLValue()?.to_file_path());
            ARRIVED.lock().unwrap().extend(paths);
            if let Some(ctx) = WAKE.get() {
                ctx.request_repaint();
            }
        }
    }
);

/// Ask to be handed the photos the Finder opens with the app. Called before the app launches.
pub fn listen() {
    let Some(mtm) = MainThreadMarker::new() else { return };
    // SAFETY: NSObject's init.
    let opener: Retained<Opener> = unsafe { msg_send![super(Opener::alloc(mtm).set_ivars(())), init] };
    let install = RcBlock::new(move |_: NonNull<NSNotification>| {
        // SAFETY: the opener answers to the selector with the arguments the manager passes.
        unsafe { NSAppleEventManager::sharedAppleEventManager().setEventHandler_andSelector_forEventClass_andEventID(&opener, sel!(handleOpenDocuments:withReplyEvent:), code(b"aevt"), code(b"odoc")) };
    });
    // SAFETY: the notification name is AppKit's, and the block takes the notification posted.
    let observer = unsafe { NSNotificationCenter::defaultCenter().addObserverForName_object_queue_usingBlock(Some(NSApplicationWillFinishLaunchingNotification), None, None, &install) };
    // the observer, and the opener its block holds, last as long as the app
    std::mem::forget(observer);
}

/// Wake the window when photos arrive.
pub fn wake(ctx: &egui::Context) {
    let _ = WAKE.set(ctx.clone());
}

/// The photos handed over since the last call.
pub fn arrived() -> Vec<PathBuf> {
    std::mem::take(&mut ARRIVED.lock().unwrap())
}
