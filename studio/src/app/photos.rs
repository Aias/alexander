//! Picking photos from the Photos library with the system's picker, which shows the
//! library's albums and search and takes any number of photos. The picker lends each photo
//! as a file that lasts only while it is being handed over, so each is copied into a
//! folder of the app's own, where later full renders and exports read it again.

use block2::RcBlock;
use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2::{define_class, msg_send, AnyThread, DefinedClass, MainThreadOnly};
use objc2_app_kit::{NSApplication, NSModalResponse, NSWindow};
use objc2_foundation::{ns_string, MainThreadMarker, NSArray, NSError, NSObject, NSObjectProtocol, NSSize, NSURL};
use objc2_photos::PHPhotoLibrary;
use objc2_photos_ui::{PHPickerConfiguration, PHPickerConfigurationAssetRepresentationMode, PHPickerFilter, PHPickerResult, PHPickerViewController, PHPickerViewControllerDelegate};
use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, Waker};

/// The copies of the photos picked, in the order picked, as they arrive.
#[derive(Default)]
struct Copies {
    /// The picker has closed and `paths` has a place for every photo picked.
    picked: bool,
    paths: Vec<Option<PathBuf>>,
    left: usize,
    waker: Option<Waker>,
}

impl Copies {
    fn arrived(&mut self) {
        if self.picked && self.left == 0 {
            if let Some(w) = self.waker.take() {
                w.wake();
            }
        }
    }
}

struct Waiting(Arc<Mutex<Copies>>);

impl Future for Waiting {
    type Output = Vec<PathBuf>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Vec<PathBuf>> {
        let mut c = self.0.lock().unwrap();
        if c.picked && c.left == 0 {
            return Poll::Ready(c.paths.drain(..).flatten().collect());
        }
        c.waker = Some(cx.waker().clone());
        Poll::Pending
    }
}

struct Ivars {
    copies: Arc<Mutex<Copies>>,
    dir: PathBuf,
}

define_class!(
    // SAFETY: NSObject has no subclassing requirements, and `Delegate` does not implement Drop.
    #[unsafe(super = NSObject)]
    #[thread_kind = MainThreadOnly]
    #[ivars = Ivars]
    struct Delegate;

    // SAFETY: NSObjectProtocol has no safety requirements.
    unsafe impl NSObjectProtocol for Delegate {}

    // SAFETY: the method's signature is the protocol's.
    unsafe impl PHPickerViewControllerDelegate for Delegate {
        #[unsafe(method(picker:didFinishPicking:))]
        fn did_finish_picking(&self, picker: &PHPickerViewController, results: &NSArray<PHPickerResult>) {
            let Ivars { copies, dir } = self.ivars();
            {
                let mut c = copies.lock().unwrap();
                c.picked = true;
                c.paths = vec![None; results.len()];
                c.left = results.len();
                c.arrived();
            }
            for (i, result) in results.iter().enumerate() {
                copy(&result, i, dir.clone(), copies.clone());
            }
            // last: ending the sheet lets go of this delegate
            if let Some(sheet) = picker.view().window() {
                if let Some(parent) = sheet.sheetParent() {
                    parent.endSheet(&sheet);
                }
            }
        }
    }
);

impl Delegate {
    fn new(mtm: MainThreadMarker, ivars: Ivars) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(ivars);
        // SAFETY: NSObject's init.
        unsafe { msg_send![super(this), init] }
    }
}

/// Copy the photo of `result` into a folder named for its asset, so the same photo picked
/// again lands at the same path and the library opens it once.
fn copy(result: &PHPickerResult, i: usize, dir: PathBuf, copies: Arc<Mutex<Copies>>) {
    // SAFETY: plain getters.
    let (provider, asset) = unsafe { (result.itemProvider(), result.assetIdentifier()) };
    let folder = asset.map_or_else(|| i.to_string(), |a| a.to_string().split('/').next().unwrap_or_default().to_string());
    let dir = dir.join(folder);
    let name = provider.suggestedName().map(|n| n.to_string());
    let handler = RcBlock::new(move |url: *mut NSURL, _: *mut NSError| {
        // SAFETY: the URL is null or valid for the duration of the handler.
        let from = unsafe { url.as_ref() }.and_then(|u| u.to_file_path());
        let copied = from.and_then(|from| {
            let file = match (&name, from.extension()) {
                (Some(n), Some(ext)) => PathBuf::from(n).with_extension(ext),
                _ => PathBuf::from(from.file_name()?),
            };
            let to = dir.join(file);
            std::fs::create_dir_all(&dir).ok()?;
            std::fs::copy(&from, &to).ok()?;
            Some(to)
        });
        let mut c = copies.lock().unwrap();
        c.paths[i] = copied;
        c.left -= 1;
        c.arrived();
    });
    // SAFETY: the handler's signature is the one the method calls.
    unsafe { provider.loadFileRepresentationForTypeIdentifier_completionHandler(ns_string!("public.image"), &handler) };
}

/// Show the picker as a sheet on the app's window, and give the copies of the photos picked
/// once all of them are in `dir`. Nothing is shown until the future is first polled.
pub async fn pick(dir: PathBuf) -> Vec<PathBuf> {
    match show(dir) {
        Some(waiting) => waiting.await,
        None => Vec::new(),
    }
}

fn show(dir: PathBuf) -> Option<Waiting> {
    let mtm = MainThreadMarker::new()?;
    let app = NSApplication::sharedApplication(mtm);
    let parent = app.mainWindow().or_else(|| app.windows().firstObject())?;
    // SAFETY: the configuration and picker are made and used on the main thread.
    let picker = unsafe {
        let config = PHPickerConfiguration::initWithPhotoLibrary(PHPickerConfiguration::alloc(), &PHPhotoLibrary::sharedPhotoLibrary());
        config.setSelectionLimit(0);
        config.setFilter(Some(&PHPickerFilter::imagesFilter()));
        config.setPreferredAssetRepresentationMode(PHPickerConfigurationAssetRepresentationMode::Current);
        PHPickerViewController::initWithConfiguration(PHPickerViewController::alloc(mtm), &config)
    };
    let copies = Arc::new(Mutex::new(Copies::default()));
    let delegate = Delegate::new(mtm, Ivars { copies: copies.clone(), dir });
    // SAFETY: the delegate outlives the picker: the sheet's handler holds it until the sheet ends.
    unsafe { picker.setDelegate(Some(ProtocolObject::from_ref(&*delegate))) };
    let sheet = NSWindow::windowWithContentViewController(&picker);
    let room = parent.frame().size;
    sheet.setContentSize(NSSize::new((room.width * 0.8).clamp(640.0, 1280.0), (room.height * 0.8).clamp(480.0, 900.0)));
    let held = (delegate, sheet.clone());
    let ended = RcBlock::new(move |_: NSModalResponse| {
        let _ = &held;
    });
    parent.beginSheet_completionHandler(&sheet, Some(&ended));
    Some(Waiting(copies))
}
