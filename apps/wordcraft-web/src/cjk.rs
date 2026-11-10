//! The CJK face the site carries beside the app, fetched the first time the text needs one.
//!
//! The web binary embeds no CJK font on purpose: the smallest of them is several megabytes, and
//! every visitor would pay for it in the download (`crates/fonts/build.rs` keeps only the small
//! Arabic and Georgian interface faces in the wasm build). On the desktop the Han characters come
//! from the installed fonts, which the font database reads from the system's font folders; a
//! browser has no font folders to read.
//!
//! So the files travel beside the app instead, and this fetches one when the app asks for it
//! ([`wordcraft_ui_egui::Services::fetch_cjk_font`]): `packaging/web/package.sh` copies the CJK
//! faces of `CRAFT_FONTS_DIR` into `fonts/cjk/<version>/` and lists them in `index.json`; the app
//! names the writing system it needs, this answers with a file from that folder and installs it in
//! the font database, which the app notices the way it notices a font the user added.
//!
//! A site that carries no files simply answers nothing: the text stays as it is, and asking again
//! is the app's call, not this one's.

use wordcraft_ui_egui::CjkSystem;

/// The folder beside the page the packer writes the fonts to, named with this build's version, so
/// every file under it can be cached forever and a new release asks for a new path. All URLs here
/// are relative to the page, which works from any path prefix (`public_url = "./"` in Trunk.toml).
pub fn index_dir() -> String {
    format!("fonts/cjk/v{}", env!("CARGO_PKG_VERSION"))
}

/// The list beside the page: which writing system answers to which file.
pub fn index_path() -> String {
    format!("{}/index.json", index_dir())
}

/// Most bytes this will take from one file. Just under the font database's own limit, so a file
/// the database would refuse is never downloaded into the browser's memory.
const MAX_BYTES: u64 = 48 << 20;
/// Most entries read from the list: a face per writing system is four, so a longer list is
/// somebody else's mistake and is cut off rather than fetched.
const MAX_ENTRIES: usize = 16;
/// Most characters of a name or path taken from the list.
const MAX_TEXT: usize = 512;

/// One file the site carries, as its list describes it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    /// The writing system this face is for.
    pub system: CjkSystem,
    /// The family name to install it under.
    pub name: String,
    /// The style name, `Regular` wanted; see [`pick`].
    pub style: String,
    /// The path to fetch, relative to the page.
    pub file: String,
    /// The size the list claims, `None` when it doesn't say. Checked before anything is downloaded
    /// when it does; the bytes themselves are checked after, so an unknown size is still bounded.
    pub bytes: Option<u64>,
}

/// Whether a path from the list is safe to fetch: a file beside the page, and nothing else. The
/// list comes from whoever hosts the site, so an absolute URL, another scheme, an escaped step or a
/// `..` out of the folder is refused rather than asked for.
fn safe_path(path: &str) -> bool {
    !path.is_empty()
        && path.len() <= MAX_TEXT
        && !path.starts_with('/')
        && !path.contains("//")
        && !path.contains(':')
        && !path.contains('\\')
        && !path.contains('%')
        && path.split('/').all(|part| part != ".." && !part.is_empty())
}

/// The CJK text in `bytes` is at most this long before it is dropped: a name the list carries that
/// runs past it is not a font name somebody typed.
fn bounded(text: &str) -> Option<String> {
    (!text.is_empty() && text.len() <= MAX_TEXT).then(|| text.to_string())
}

/// Read the list. What can't be used is skipped rather than an error: a damaged list means the same
/// thing an absent one does, that this site carries no fonts.
pub fn parse(text: &str) -> Vec<Entry> {
    let Ok(root) = serde_json::from_str::<serde_json::Value>(text) else {
        log::warn!("the site's font list is not JSON");
        return Vec::new();
    };
    let Some(files) = root.get("files").and_then(|f| f.as_array()) else {
        log::warn!("the site's font list has no `files`");
        return Vec::new();
    };
    let mut out = Vec::new();
    for f in files.iter().take(MAX_ENTRIES) {
        let get = |key: &str| f.get(key).and_then(|v| v.as_str());
        let (Some(name), Some(style), Some(file)) = (get("name"), get("style"), get("file")) else { continue };
        // An id this build doesn't know is skipped: a newer packager may ship faces for scripts
        // the app has no name for yet, and those must not hide the ones it does.
        let Some(system) = get("system").and_then(CjkSystem::from_id) else { continue };
        if !safe_path(file) {
            log::warn!("the site's font list names an unusable path; skipped");
            continue;
        }
        let (Some(name), Some(style)) = (bounded(name), bounded(style)) else { continue };
        let bytes = f.get("bytes").and_then(|v| v.as_u64());
        if bytes.is_some_and(|n| n > MAX_BYTES) {
            log::warn!("{name} is larger than the web build will fetch; skipped");
            continue;
        }
        out.push(Entry { system, name, style, file: file.to_string(), bytes });
    }
    out
}

/// The file to fetch for `system`: the Regular face naming it, else any face naming it, else the
/// system it may stand in for ([`CjkSystem::fallbacks`], so a site with only a Simplified Chinese
/// face answers Traditional Chinese with it, as the desktop does with no Traditional font
/// installed). `None` when the site carries nothing usable.
pub fn pick(system: CjkSystem, index: &[Entry]) -> Option<&Entry> {
    for want in system.fallbacks() {
        let mut any = None;
        for entry in index.iter().filter(|e| e.system == *want) {
            if entry.style.eq_ignore_ascii_case("Regular") {
                return Some(entry);
            }
            if any.is_none() {
                any = Some(entry);
            }
        }
        if any.is_some() {
            return any;
        }
    }
    None
}

#[cfg(target_arch = "wasm32")]
use wasm_bindgen::JsCast as _;

/// Ask the site for `system`'s face and install it. Returns once the answer is in: the caller
/// (the browser shell) spawns this, and the app notices the font through the font database.
#[cfg(target_arch = "wasm32")]
pub fn fetch(system: CjkSystem, ctx: egui::Context) {
    wasm_bindgen_futures::spawn_local(async move {
        match download(system).await {
            Ok(true) => ctx.request_repaint(),
            Ok(false) => {}
            Err(e) => log::warn!("couldn't fetch the {system:?} font: {e}"),
        }
    });
}

/// The list, the chosen file, the install. `Ok(false)`: nothing to do (the site carries no files,
/// or none for this system) — an expected answer, so it is logged once and not retried.
#[cfg(target_arch = "wasm32")]
async fn download(system: CjkSystem) -> Result<bool, String> {
    let text = fetch_text(&index_path()).await?;
    let index = parse(&text);
    let Some(entry) = pick(system, &index) else {
        log::info!("{} carries no {system:?} font", index_path());
        return Ok(false);
    };
    log::info!("fetching {} ({} bytes) for {system:?}", entry.file, entry.bytes.unwrap_or(0));
    let bytes = fetch_bytes(&entry.file).await?;
    if bytes.len() as u64 > MAX_BYTES {
        log::warn!("{} is {} bytes, more than the web build fetches", entry.file, bytes.len());
        return Ok(false);
    }
    match wordcraft_fonts::FontDb::global().install_font(&entry.name, &bytes) {
        Ok(installed) => {
            if let Some(warning) = installed.warning {
                log::warn!("{} works for this visit only: {warning}", entry.name);
            }
            log::info!("{} installed from {}", entry.name, entry.file);
            Ok(true)
        }
        Err(e) => {
            log::warn!("{} is not a font this build can use: {e}", entry.file);
            Ok(false)
        }
    }
}

/// GET `path` beside the page and its text.
#[cfg(target_arch = "wasm32")]
async fn fetch_text(path: &str) -> Result<String, String> {
    let body = response(path).await?;
    let future = body.text().map_err(js)?;
    let value = wasm_bindgen_futures::JsFuture::from(future).await.map_err(js)?;
    value.as_string().ok_or_else(|| "the answer is not text".to_string())
}

/// GET `path` beside the page and its bytes.
#[cfg(target_arch = "wasm32")]
async fn fetch_bytes(path: &str) -> Result<Vec<u8>, String> {
    let body = response(path).await?;
    let future = body.array_buffer().map_err(js)?;
    let value = wasm_bindgen_futures::JsFuture::from(future).await.map_err(js)?;
    Ok(js_sys::Uint8Array::new(&value).to_vec())
}

/// GET `path`, answering only when the server says it is there. A network failure comes back
/// through the promise, the same way a 404 does from the status check.
#[cfg(target_arch = "wasm32")]
async fn response(path: &str) -> Result<web_sys::Response, String> {
    let window = web_sys::window().ok_or("no window")?;
    let value = wasm_bindgen_futures::JsFuture::from(window.fetch_with_str(path)).await.map_err(js)?;
    let body: web_sys::Response = value.dyn_into().map_err(|_| "the answer is not a response".to_string())?;
    if !body.ok() {
        return Err(format!("{path} answered {}", body.status()));
    }
    Ok(body)
}

#[cfg(target_arch = "wasm32")]
fn js(e: wasm_bindgen::JsValue) -> String {
    format!("{e:?}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fmt::Write as _;

    /// The list `packaging/web/package.sh` writes, as it would be served beside the app.
    const LIST: &str = r#"{"files":[{"system":"hans","name":"Noto Sans CJK SC","style":"Regular","file":"v1/noto-sans-cjk-sc.ttf","bytes":17301000},{"system":"hans","name":"Noto Sans CJK SC","style":"Bold","file":"v1/noto-sans-cjk-sc-bold.ttf","bytes":17401000},{"system":"hant","name":"Noto Sans CJK TC","style":"Regular","file":"v1/noto-sans-cjk-tc.ttf","bytes":17201000},{"system":"jpan","name":"Noto Sans CJK JP","style":"Regular","file":"v1/noto-sans-cjk-jp.ttf","bytes":4700000},{"system":"kore","name":"Noto Sans CJK KR","style":"Regular","file":"v1/noto-sans-cjk-kr.ttf","bytes":17101000}]}"#;

    fn list(text: &str) -> Vec<Entry> {
        parse(text)
    }

    #[test]
    fn the_list_reads_per_writing_system() {
        let index = list(LIST);
        assert_eq!(index.len(), 5);
        assert_eq!(pick(CjkSystem::Hans, &index).map(|e| e.file.as_str()), Some("v1/noto-sans-cjk-sc.ttf"));
        assert_eq!(pick(CjkSystem::Japan, &index).map(|e| e.file.as_str()), Some("v1/noto-sans-cjk-jp.ttf"));
        assert_eq!(pick(CjkSystem::Korea, &index).map(|e| e.name.as_str()), Some("Noto Sans CJK KR"));
    }

    /// The Regular face answers even when the list names others, and a list that names only a
    /// bold face still answers with it: a wrong weight is readable, boxes are not.
    #[test]
    fn the_regular_face_is_taken_first_but_anything_beats_nothing() {
        let index = list(LIST);
        assert_eq!(pick(CjkSystem::Hans, &index).map(|e| e.style.as_str()), Some("Regular"));
        let only_bold = list(r#"{"files":[{"system":"hans","name":"SC","style":"Bold","file":"bold.ttf","bytes":1}]}"#);
        assert_eq!(pick(CjkSystem::Hans, &only_bold).map(|e| e.file.as_str()), Some("bold.ttf"));
    }

    /// A Traditional Chinese site without a Traditional face is answered with the Simplified one,
    /// as the desktop does when no Traditional font is installed. Japanese and Korean stand alone:
    /// their hanzi are the Simplified standard's shapes, which is worse than a box.
    #[test]
    fn traditional_chinese_takes_the_simplified_face_when_the_site_has_no_other() {
        let only_hans = list(r#"{"files":[{"system":"hans","name":"SC","style":"Regular","file":"sc.ttf","bytes":1}]}"#);
        assert_eq!(pick(CjkSystem::Hant, &only_hans).map(|e| e.file.as_str()), Some("sc.ttf"));
        assert_eq!(pick(CjkSystem::Japan, &only_hans), None);
        assert_eq!(pick(CjkSystem::Korea, &only_hans), None);
    }

    /// The list is whoever hosts the site wrote: junk, unknown systems, paths that would leave the
    /// folder, and files too big to fetch are all skipped, and never fetched. An entry that simply
    /// doesn't say its size stays — the bytes are checked once they're in hand.
    #[test]
    fn a_damaged_or_hostile_list_answers_no_font() {
        for bad in ["", "null", "[]", "{}", "{\"files\":42}", "{\"files\":{}}", "not json at all"] {
            assert!(list(bad).is_empty(), "{bad}");
        }
        let hostile = list(
            r#"{"files":[
                {"system":"hans","name":"SC","style":"Regular","file":"https://elsewhere.test/sc.ttf","bytes":10},
                {"system":"hans","name":"SC","style":"Regular","file":"/etc/passwd","bytes":10},
                {"system":"hans","name":"SC","style":"Regular","file":"../../secret.ttf","bytes":10},
                {"system":"hans","name":"SC","style":"Regular","file":"v1//sc.ttf","bytes":10},
                {"system":"hans","name":"SC","style":"Regular","file":"v1/..%2f..%2fsc.ttf","bytes":10},
                {"system":"hans","name":"SC","style":"Regular","file":"","bytes":10},
                {"system":"hangul","name":"SC","style":"Regular","file":"v1/sc.ttf","bytes":10},
                {"system":"hans","name":"SC","file":"v1/sc.ttf","bytes":10},
                {"system":"hans","name":"SC","style":"Regular","file":"v1/sc.ttf","bytes":99999999999},
                {"system":"hans","name":"SC","style":"Regular","file":"v1/sc.ttf"}
            ]}"#,
        );
        assert_eq!(hostile.len(), 1, "{hostile:?}");
        assert_eq!(hostile[0].bytes, None);
    }

    /// A longer list than the web build will read is cut off, not fetched entry by entry.
    #[test]
    fn a_long_list_is_cut_off() {
        let mut text = String::from("{\"files\":[");
        for i in 0..MAX_ENTRIES * 3 {
            if i > 0 {
                text.push(',');
            }
            let _ = write!(text, "{{\"system\":\"hans\",\"name\":\"F{i}\",\"style\":\"Regular\",\"file\":\"v1/f{i}.ttf\",\"bytes\":10}}");
        }
        text.push_str("]}");
        let index = list(&text);
        assert_eq!(index.len(), MAX_ENTRIES);
    }

    /// A file the font database would refuse is never downloaded: the list's own claim is checked
    /// against the same bound.
    #[test]
    fn a_file_bigger_than_the_web_build_can_take_is_skipped() {
        let big = format!(r#"{{"files":[{{"system":"hans","name":"SC","style":"Regular","file":"v1/sc.ttf","bytes":{}}}]}}"#, MAX_BYTES + 1);
        assert!(list(&big).is_empty());
        let just_under = format!(r#"{{"files":[{{"system":"hans","name":"SC","style":"Regular","file":"v1/sc.ttf","bytes":{}}}]}}"#, MAX_BYTES);
        assert_eq!(list(&just_under).len(), 1);
        assert!((MAX_BYTES as usize) < wordcraft_fonts::MAX_FONT_FILE_BYTES, "the web bound stays under the database's own");
    }

    /// The ids are the ones the packager writes; an id it doesn't know is no system at all.
    #[test]
    fn the_system_ids_are_the_ones_the_packager_writes() {
        for s in [CjkSystem::Hans, CjkSystem::Hant, CjkSystem::Japan, CjkSystem::Korea] {
            assert_eq!(CjkSystem::from_id(s.as_str()), Some(s));
        }
        assert_eq!(CjkSystem::from_id(""), None);
        assert_eq!(CjkSystem::from_id("LATN"), None);
        assert_eq!(CjkSystem::from_id("hanshans"), None);
    }

    /// The files live under the app's own version, so they can be cached forever and a release
    /// that changes them asks for a new path.
    #[test]
    fn the_list_lives_under_the_build_version() {
        let path = index_path();
        assert!(path.starts_with("fonts/cjk/v"), "{path}");
        assert!(path.ends_with("/index.json"), "{path}");
        assert!(!path.starts_with('/'), "{path} stays relative to the page");
        assert!(path.contains(env!("CARGO_PKG_VERSION")), "{path}");
    }
}
