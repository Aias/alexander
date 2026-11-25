//! Library bookkeeping, selection, looks, and the round trip from an export back to its original.
use alexander_studio::export::{self, Format, Picture, Size};
use alexander_studio::library::{sync_changed, Library};
use alexander_studio::load;
use alexander_studio::params::Settings;
use alexander_studio::provenance::{self, Record};
use std::collections::BTreeSet;
use std::path::PathBuf;

fn lib(n: usize) -> (Library, Vec<u64>) {
    let mut l = Library::new();
    let ids = (0..n).map(|i| l.add(PathBuf::from(format!("/p/{i}.jpg"))).unwrap()).collect();
    (l, ids)
}

fn sel(l: &Library) -> Vec<u64> {
    l.selection()
}

#[test]
fn duplicates_are_skipped() {
    let (mut l, _) = lib(3);
    assert!(l.add(PathBuf::from("/p/1.jpg")).is_none());
    assert_eq!(l.len(), 3);
}

#[test]
fn click_toggle_extend() {
    let (mut l, id) = lib(6);
    l.click(id[1]);
    assert_eq!(sel(&l), vec![id[1]]);
    l.toggle(id[3]);
    assert_eq!(sel(&l), vec![id[1], id[3]]);
    assert_eq!(l.focus, Some(id[3]));
    l.toggle(id[3]);
    assert_eq!(sel(&l), vec![id[1]]);
    assert_eq!(l.focus, Some(id[1]));
    l.click(id[2]);
    l.extend(id[4]);
    assert_eq!(sel(&l), vec![id[2], id[3], id[4]]);
    l.extend(id[0]);
    assert_eq!(sel(&l), vec![id[0], id[1], id[2]]);
    l.select_all();
    assert_eq!(sel(&l).len(), 6);
}

#[test]
fn arrows_move_and_extend() {
    let (mut l, id) = lib(5);
    l.step(1, false);
    assert_eq!(sel(&l), vec![id[0]]);
    l.step(1, false);
    l.step(1, true);
    l.step(1, true);
    assert_eq!(sel(&l), vec![id[1], id[2], id[3]]);
    l.step(10, false);
    assert_eq!(sel(&l), vec![id[4]]);
    l.step(-10, false);
    assert_eq!(sel(&l), vec![id[0]]);
    l.select_all();
    l.step_in_selection(2);
    assert_eq!(l.focus, Some(id[2]));
    assert_eq!(sel(&l).len(), 5);
}

#[test]
fn remove_never_touches_disk_and_moves_focus() {
    let (mut l, id) = lib(5);
    l.click(id[1]);
    l.toggle(id[2]);
    let gone: BTreeSet<u64> = l.selected().clone();
    l.remove(&gone);
    assert_eq!(l.len(), 3);
    assert!(l.get(id[1]).is_none());
    assert_eq!(l.focus, Some(id[3]));
    assert_eq!(sel(&l), vec![id[3]]);
    assert_eq!(l.position(id[4]), Some(2));
}

#[test]
fn edits_carry_only_the_changed_knob() {
    let (mut l, id) = lib(3);
    l.set_look(&[id[0]], Some(Settings { grade: 4.0, ..Settings::default() }));
    let before = Settings::default();
    let after = Settings { grain: 2.0, ..Settings::default() };
    l.checkpoint(&[id[0], id[1]]);
    l.apply_change(&[id[0], id[1]], &before, &after);
    let a = l.get(id[0]).unwrap().look.clone().unwrap();
    assert_eq!((a.grade, a.grain), (4.0, 2.0));
    let b = l.get(id[1]).unwrap().look.clone().unwrap();
    assert_eq!((b.grade, b.grain), (3.0, 2.0));
    assert!(l.get(id[2]).unwrap().look.is_none());
    l.undo();
    assert_eq!(l.get(id[0]).unwrap().look.as_ref().unwrap().grain, 1.0);
    assert!(l.get(id[1]).unwrap().look.is_none());
    l.redo();
    assert_eq!(l.get(id[1]).unwrap().look.as_ref().unwrap().grain, 2.0);
}

#[test]
fn every_knob_is_carried() {
    // every field of Settings, changed one at a time through its JSON form
    let base = serde_json::to_value(Settings::default()).unwrap();
    let keys: Vec<String> = base.as_object().unwrap().keys().cloned().collect();
    assert!(keys.len() >= 40);
    for k in keys {
        let mut v = base.clone();
        let changed = match &v[&k] {
            serde_json::Value::Number(n) if n.is_u64() => serde_json::json!(n.as_u64().unwrap() + 7),
            serde_json::Value::Number(n) => serde_json::json!(n.as_f64().unwrap() + 0.125),
            serde_json::Value::Bool(b) => serde_json::json!(!b),
            serde_json::Value::Null => match k.as_str() {
                "dev" => serde_json::json!({"Minutes": 9.0}),
                "page_mm" => serde_json::json!(120.0),
                "view" => serde_json::json!("Page"),
                other => panic!("no test value for {other}"),
            },
            serde_json::Value::String(_) => match k.as_str() {
                "film" => serde_json::json!("DoubleX"),
                "filter" => serde_json::json!("Red"),
                "repro" => serde_json::json!("Book"),
                "frame" => serde_json::json!("Strip"),
                other => panic!("no test value for {other}"),
            },
            other => panic!("unexpected {other}"),
        };
        v[&k] = changed;
        let after: Settings = serde_json::from_value(v).unwrap();
        assert_ne!(after, Settings::default(), "{k} did not change");
        let mut target = Settings::default();
        sync_changed(&Settings::default(), &after, &mut target);
        assert_eq!(target, after, "{k} is not carried by sync_changed");
    }
}

#[test]
fn process_toggle_keeps_the_look() {
    let (mut l, id) = lib(2);
    let look = Settings { grade: 1.5, ..Settings::default() };
    l.set_look(&[id[0]], Some(look.clone()));
    l.toggle_process(&[id[0], id[1]]);
    assert_eq!(l.get(id[0]).unwrap().look, Some(look.clone()));
    assert_eq!(l.get(id[1]).unwrap().look, Some(Settings::default()));
    l.toggle_process(&[id[0], id[1]]);
    assert!(l.get(id[0]).unwrap().look.is_none());
    l.toggle_process(&[id[0]]);
    assert_eq!(l.get(id[0]).unwrap().look, Some(look));
}

#[test]
fn an_export_opens_as_its_original_with_its_look() {
    let dir = std::env::temp_dir().join(format!("studio-roundtrip-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let src = dir.join("street.png");
    let mut img = image::RgbImage::new(90, 60);
    for (x, y, p) in img.enumerate_pixels_mut() {
        *p = image::Rgb([(x * 2) as u8, (y * 3) as u8, 90]);
    }
    img.save(&src).unwrap();
    let look = Settings { grade: 4.5, repro: alexander_studio::params::Repro::Copy, ..Settings::default() };
    let pic = Picture { w: 45, h: 30, rgba: vec![0.5; 45 * 30 * 4] };
    for format in Format::ALL {
        let out = export::output_path(&dir, &src, format, &|_| false);
        export::write(&pic, Size::Original, 90, format, &Record { source: src.clone(), look: look.clone() }, &out).unwrap();
        let opened = load::open(&out).unwrap();
        assert_eq!(opened.path, src, "{format:?}");
        assert_eq!(opened.look.as_ref(), Some(&look));
        assert_eq!(opened.original, (90, 60));
        std::fs::remove_file(&out).unwrap();
    }
    // with the original gone, the export is a photo in its own right
    let out = dir.join("street-twb.png");
    export::write(&pic, Size::Grid, 90, Format::Png8, &Record { source: dir.join("gone.png"), look }, &out).unwrap();
    assert!(provenance::read(&out).is_some());
    let opened = load::open(&out).unwrap();
    assert_eq!(opened.path, out);
    assert!(opened.look.is_none());
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn opening_turns_upright_and_makes_a_proxy() {
    let dir = std::env::temp_dir().join(format!("studio-open-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let p = dir.join("wide.png");
    image::RgbImage::from_pixel(1600, 900, image::Rgb([200, 120, 40])).save(&p).unwrap();
    let o = load::open(&p).unwrap();
    assert_eq!(o.original, (1600, 900));
    assert_eq!((o.proxy.w, o.proxy.h), (640, 360));
    let full = load::full(&p, 2560).unwrap();
    assert_eq!((full.w, full.h), (2560, 1440));
    assert_eq!(load::grid_long(0, (1600, 900), 16384), 1600);
    assert_eq!(load::grid_long(0, (30000, 900), 16384), 16384);
    assert_eq!(load::collect(std::slice::from_ref(&dir)), vec![std::fs::canonicalize(&p).unwrap()]);
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn exif_orientation_is_honoured() {
    let dir = std::env::temp_dir().join(format!("studio-exif-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let p = dir.join("sideways.jpg");
    // stored 40 wide and 80 tall, bright at the top; orientation 6 asks for a quarter turn clockwise
    let (w, h) = (40u16, 80u16);
    let px: Vec<u8> = (0..h as usize).flat_map(|y| (0..w as usize).flat_map(move |_| if y < 20 { [250, 250, 250] } else { [10, 10, 10] })).collect();
    let mut out = Vec::new();
    let mut enc = jpeg_encoder::Encoder::new(&mut out, 95);
    let tiff = [b'I', b'I', 42, 0, 8, 0, 0, 0, 1, 0, 0x12, 0x01, 3, 0, 1, 0, 0, 0, 6, 0, 0, 0, 0, 0, 0, 0];
    enc.add_exif_metadata(&tiff).unwrap();
    enc.encode(&px, w, h, jpeg_encoder::ColorType::Rgb).unwrap();
    std::fs::write(&p, out).unwrap();
    let o = load::open(&p).unwrap();
    assert_eq!(o.original, (80, 40));
    // the top of the stored picture is now on the right
    let at = |x: u32, y: u32| o.proxy.rgba[((y * o.proxy.w + x) * 4) as usize];
    assert!(at(o.proxy.w - 2, 20) > 200 && at(1, 20) < 60);
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn paths_index_and_revision() {
    let (mut l, id) = lib(3);
    let r = l.revision();
    l.click(id[1]);
    assert_ne!(l.revision(), r);
    l.set_path(id[1], PathBuf::from("/q/orig.jpg"));
    assert_eq!(l.find_path(&PathBuf::from("/q/orig.jpg")), Some(id[1]));
    assert_eq!(l.find_path(&PathBuf::from("/p/1.jpg")), None);
    assert_eq!(l.get(id[1]).unwrap().name, "orig.jpg");
    assert!(l.add(PathBuf::from("/q/orig.jpg")).is_none());
    l.remove(&[id[1]].into());
    assert_eq!(l.find_path(&PathBuf::from("/q/orig.jpg")), None);
    assert!(l.add(PathBuf::from("/q/orig.jpg")).is_some());
}

#[test]
fn records_that_do_not_fit_are_not_followed() {
    let dir = std::env::temp_dir().join(format!("studio-records-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let dir = std::fs::canonicalize(&dir).unwrap();
    let other = dir.join("other.png");
    image::RgbImage::from_pixel(90, 90, image::Rgb([10, 20, 30])).save(&other).unwrap();
    let pic = Picture { w: 60, h: 40, rgba: vec![0.5; 60 * 40 * 4] };
    // a record naming a photo of another shape: the file opens as itself
    let out = dir.join("odd-twb.png");
    export::write(&pic, Size::Grid, 0, Format::Png8, &Record { source: other.clone(), look: Settings::default() }, &out).unwrap();
    let opened = load::open(&out).unwrap();
    assert_eq!((opened.path.clone(), opened.look.is_none()), (out.clone(), true));
    // a relative source is never looked up in the working directory
    let out2 = dir.join("rel-twb.png");
    export::write(&pic, Size::Grid, 0, Format::Png8, &Record { source: "Cargo.toml".into(), look: Settings::default() }, &out2).unwrap();
    assert!(provenance::resolve(&out2).is_none());
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn exposure_is_measured_at_the_simulation_grid() {
    // the levels twb2.py measures are those of the photo resized to the grid, not of the proxy
    let dir = std::env::temp_dir().join(format!("studio-levels-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let p = dir.join("fine.png");
    let (w, h) = (3200u32, 2000u32);
    let img = image::RgbImage::from_fn(w, h, |x, y| {
        // fine bright detail that a 640 px copy smooths away
        let v = if (x / 2 + y / 2) % 7 == 0 { 250 } else { ((x * 160) / w + 40) as u8 };
        image::Rgb([v, v, v])
    });
    img.save(&p).unwrap();
    let o = load::open(&p).unwrap();
    let px = load::full(&p, 2560).unwrap();
    let weights = alexander_studio::film::Film::load(alexander_studio::film::FilmKind::ALL[0]).weights[0];
    let want = alexander_studio::tone::exposure_levels(&px.rgba, px.w as usize, px.h as usize, weights);
    assert_eq!(o.levels[0][0], want);
    std::fs::remove_dir_all(&dir).unwrap();
}
