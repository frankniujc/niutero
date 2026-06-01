//! Icons — the design's exact SVG stroke set (`app/shared.jsx`'s `Icon`),
//! rendered crisply via egui_extras + resvg and tinted to the theme color.
//!
//! Each glyph's SVG markup uses the design's literal `d` path data with a white
//! stroke; `tint(color)` then paints it in the right color (white × tint =
//! tint). The image loader caches per (uri, size), so this stays cheap.
//! `install_image_loaders` must be called once at startup.

use eframe::egui::{self, Color32, Rect, Vec2};

/// Which glyph. Names mirror the design's `Icon` keys.
#[derive(Clone, Copy)]
#[allow(dead_code)]
pub enum Glyph {
    Library,
    Normalize,
    Ai,
    Settings,
    Sync,
    Sun,
    Moon,
    Branch,
    Search,
    Plus,
    Link,
    PanelLeft,
    PanelRight,
    Star,
    StarFilled,
    Doc,
    Folder,
    Book,
    Attach,
    Lock,
    Unlock,
    Close,
    WinMinimize,
    WinMaximize,
    ChevronRight,
    ChevronDown,
    ChevronUp,
    Rows,
    More,
    Quote,
    Filter,
    Grid,
    Send,
    Check,
    Copy,
    Warn,
    Key,
    Refresh,
    CheckCircle,
    ArrowRight,
    Expand,
    Pause,
    Chat,
    Sparkle,
    Trash,
    Info,
    Clock,
    Download,
    Tag,
}

// Wrap design `d`/element markup into a full SVG with a white stroke (round
// caps/joins, 1.7px — the design's defaults). `concat!` keeps these `&'static`.
macro_rules! svg {
    ($($inner:literal),+ $(,)?) => {
        concat!(
            "<svg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 24 24' fill='none' ",
            "stroke='#ffffff' stroke-width='1.7' stroke-linecap='round' stroke-linejoin='round'>",
            $($inner),+,
            "</svg>"
        )
    };
}

const LIBRARY: &str = svg!("<path d='M4 5h5v14H4zM10 5h4v14h-4zM16 6l4 1-3 12-4-1z'/>");
const NORMALIZE: &str = svg!(
    "<path d='M3 21l10.5-10.5'/>",
    "<path d='M16.5 4.2L17.5 6.5L19.8 7.5L17.5 8.5L16.5 10.8L15.5 8.5L13.2 7.5L15.5 6.5Z'/>",
    "<path d='M6.4 4.5v2.2M5.3 5.6h2.2' stroke-width='1.3'/>",
);
const AI: &str = svg!(
    "<path d='M12 3l1.6 4.4L18 9l-4.4 1.6L12 15l-1.6-4.4L6 9l4.4-1.6z'/>",
    "<path d='M18 15l.8 2.2L21 18l-2.2.8L18 21l-.8-2.2L15 18l2.2-.8z' stroke-width='1.3'/>",
);
const SETTINGS: &str = svg!(
    "<circle cx='12' cy='12' r='3'/>",
    "<path d='M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 1 1-2.83 2.83l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 1 1-4 0v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 1 1-2.83-2.83l.06-.06a1.65 1.65 0 0 0 .33-1.82 1.65 1.65 0 0 0-1.51-1H3a2 2 0 1 1 0-4h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 1 1 2.83-2.83l.06.06a1.65 1.65 0 0 0 1.82.33H9a1.65 1.65 0 0 0 1-1.51V3a2 2 0 1 1 4 0v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 1 1 2.83 2.83l-.06.06a1.65 1.65 0 0 0-.33 1.82V9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 1 1 0 4h-.09a1.65 1.65 0 0 0-1.51 1z'/>",
);
const SYNC: &str =
    svg!("<path d='M4 11a8 8 0 0 1 14-4.5L20 8M20 13a8 8 0 0 1-14 4.5L4 16M20 4v4h-4M4 20v-4h4'/>");
const SUN: &str = svg!(
    "<circle cx='12' cy='12' r='4'/>",
    "<path d='M12 2v2M12 20v2M4 12H2M22 12h-2M5 5l1.4 1.4M17.6 17.6L19 19M19 5l-1.4 1.4M6.4 17.6L5 19'/>",
);
const MOON: &str = svg!("<path d='M20 14.5A8 8 0 0 1 9.5 4a7 7 0 1 0 10.5 10.5z'/>");
const BRANCH: &str = svg!(
    "<circle cx='6' cy='6' r='2.4'/><circle cx='6' cy='18' r='2.4'/><circle cx='18' cy='8' r='2.4'/>",
    "<path d='M6 8.4v7.2M18 10.4c0 3-3 3.6-6 3.6'/>",
);
const SEARCH: &str = svg!("<circle cx='11' cy='11' r='7'/><path d='M20 20l-3.5-3.5'/>");
const PLUS: &str = svg!("<path d='M12 5v14M5 12h14'/>");
const LINK: &str = svg!(
    "<path d='M10 14a4 4 0 0 0 6 .4l2-2a4 4 0 0 0-5.7-5.7l-1 1M14 10a4 4 0 0 0-6-.4l-2 2a4 4 0 0 0 5.7 5.7l1-1'/>",
);
const PANEL_LEFT: &str =
    svg!("<rect x='3' y='4' width='18' height='16' rx='2'/><path d='M9 4v16' stroke-width='1.5'/>");
const PANEL_RIGHT: &str = svg!(
    "<rect x='3' y='4' width='18' height='16' rx='2'/><path d='M15 4v16' stroke-width='1.5'/>"
);
const STAR: &str =
    svg!("<path d='M12 4l2.3 4.8 5.2.7-3.8 3.6.9 5.2L12 16.6 7.4 18.3l.9-5.2L4.5 9.5l5.2-.7z'/>");
// Filled star (rating "on"): same outline but filled.
const STAR_FILLED: &str = concat!(
    "<svg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 24 24' fill='#ffffff' stroke='#ffffff' ",
    "stroke-width='1.0' stroke-linejoin='round'>",
    "<path d='M12 4l2.3 4.8 5.2.7-3.8 3.6.9 5.2L12 16.6 7.4 18.3l.9-5.2L4.5 9.5l5.2-.7z'/>",
    "</svg>"
);
const DOC: &str = svg!(
    "<path d='M6 3h8l4 4v14H6z'/>",
    "<path d='M14 3v4h4M9 13h6M9 17h6' stroke-width='1.3'/>"
);
const FOLDER: &str = svg!(
    "<path d='M4 6.5A1.5 1.5 0 0 1 5.5 5h3.3a1.5 1.5 0 0 1 1.2.6L11.2 7H18.5A1.5 1.5 0 0 1 20 8.5V17.5A1.5 1.5 0 0 1 18.5 19h-13A1.5 1.5 0 0 1 4 17.5z'/>",
);
const BOOK: &str = svg!(
    "<path d='M4 5a2 2 0 0 1 2-2h6v16H6a2 2 0 0 0-2 2zM20 5a2 2 0 0 0-2-2h-6v16h6a2 2 0 0 1 2 2z'/>",
);
const ATTACH: &str = svg!(
    "<path d='M21 9.5l-8.5 8.5a4 4 0 0 1-5.7-5.7l8.5-8.5a2.7 2.7 0 0 1 3.8 3.8l-8.5 8.5a1.3 1.3 0 0 1-1.9-1.9l7.8-7.8'/>",
);
const LOCK: &str =
    svg!("<rect x='5' y='11' width='14' height='9' rx='2'/><path d='M8 11V8a4 4 0 0 1 8 0v3'/>");
const UNLOCK: &str =
    svg!("<rect x='5' y='11' width='14' height='9' rx='2'/><path d='M8 11V8a4 4 0 0 1 7.5-2'/>");
const CLOSE: &str = svg!("<path d='M6 6l12 12M18 6L6 18'/>");
// Windows window-control glyphs: thinner stroke + square corners to read as OS chrome.
const WIN_MINIMIZE: &str = svg!("<path d='M5 12h14' stroke-width='1.3' stroke-linecap='square'/>");
const WIN_MAXIMIZE: &str =
    svg!("<rect x='5.5' y='5.5' width='13' height='13' rx='1' stroke-width='1.3' stroke-linejoin='miter'/>");
const CHEVRON_RIGHT: &str = svg!("<path d='M9 6l6 6-6 6'/>");
const CHEVRON_DOWN: &str = svg!("<path d='M6 9l6 6 6-6'/>");
const CHEVRON_UP: &str = svg!("<path d='M6 15l6-6 6 6'/>");
const ROWS: &str = svg!("<path d='M4 6h16M4 12h16M4 18h16'/>");
// Three filled dots (overflow) — fill, no stroke.
const MORE: &str = concat!(
    "<svg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 24 24' fill='#ffffff' stroke='none'>",
    "<circle cx='5' cy='12' r='1.4'/><circle cx='12' cy='12' r='1.4'/><circle cx='19' cy='12' r='1.4'/>",
    "</svg>"
);
const QUOTE: &str = svg!("<path d='M7 7H4v6h3l-1 4M17 7h-3v6h3l-1 4' stroke-width='1.5'/>");
const FILTER: &str = svg!("<path d='M3 5h18l-7 8v5l-4 2v-7z'/>");
const GRID: &str = svg!("<path d='M4 4h7v7H4zM13 4h7v7h-7zM4 13h7v7H4zM13 13h7v7h-7z'/>");
const SEND: &str = svg!("<path d='M5 12l15-7-6 16-3-7z'/>");
const CHECK: &str = svg!("<path d='M5 12l5 5 9-11'/>");
const COPY: &str = svg!(
    "<rect x='9' y='9' width='11' height='11' rx='2'/>",
    "<path d='M5 15V5a2 2 0 0 1 2-2h8'/>",
);
const WARN: &str = svg!(
    "<path d='M12 4l9 16H3z'/>",
    "<path d='M12 10v4M12 17.5v.01' stroke-width='1.9'/>",
);
const KEY: &str = svg!(
    "<circle cx='8' cy='8' r='4'/>",
    "<path d='M11 11l8 8M16 16l2-2M19 19l2-2'/>",
);
const REFRESH: &str = svg!(
    "<path d='M4 11a8 8 0 0 1 13.5-5l2.5 2.5M20 13a8 8 0 0 1-13.5 5L4 15.5M19 4v4h-4M5 20v-4h4'/>"
);
const CHECK_CIRCLE: &str = svg!(
    "<circle cx='12' cy='12' r='8.5'/>",
    "<path d='M8.5 12l2.5 2.5L16 9'/>",
);
const ARROW_RIGHT: &str = svg!("<path d='M5 12h14M13 6l6 6-6 6'/>");
const EXPAND: &str = svg!("<path d='M9 4H4v5M15 4h5v5M9 20H4v-5M15 20h5v-5'/>");
const PAUSE: &str = svg!("<path d='M8 5v14M16 5v14'/>");
const CHAT: &str = svg!("<path d='M21 12a8 8 0 0 1-11.5 7.2L4 20l1-4.5A8 8 0 1 1 21 12z'/>");
const SPARKLE: &str = svg!("<path d='M12 3l1.7 5.3L19 10l-5.3 1.7L12 17l-1.7-5.3L5 10l5.3-1.7z'/>");
const TRASH: &str = svg!("<path d='M4 7h16M9 7V5h6v2M6 7l1 13h10l1-13'/>");
const INFO: &str = svg!(
    "<circle cx='12' cy='12' r='8.5'/>",
    "<path d='M12 11v5M12 8v.01' stroke-width='1.9'/>",
);
const CLOCK: &str = svg!("<circle cx='12' cy='12' r='8'/>", "<path d='M12 8v4l3 2'/>",);
const DOWNLOAD: &str = svg!("<path d='M12 4v11m0 0l-4-4m4 4l4-4M5 19h14'/>");
// Tag: a stroked tag outline with a filled punch-hole dot.
const TAG: &str = concat!(
    "<svg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 24 24' fill='none' ",
    "stroke='#ffffff' stroke-width='1.7' stroke-linecap='round' stroke-linejoin='round'>",
    "<path d='M3 12l8.5-8.5a2 2 0 0 1 1.4-.6H19a2 2 0 0 1 2 2v5.7a2 2 0 0 1-.6 1.4L12 20.5a2 2 0 0 1-2.8 0l-6-6a2 2 0 0 1 0-2.8z'/>",
    "<circle cx='16' cy='8' r='1.2' fill='#ffffff' stroke='none'/>",
    "</svg>"
);

/// (stable cache uri, SVG markup) for a glyph.
fn source(g: Glyph) -> (&'static str, &'static str) {
    match g {
        Glyph::Library => ("bytes://niu-library.svg", LIBRARY),
        Glyph::Normalize => ("bytes://niu-normalize.svg", NORMALIZE),
        Glyph::Ai => ("bytes://niu-ai.svg", AI),
        Glyph::Settings => ("bytes://niu-settings.svg", SETTINGS),
        Glyph::Sync => ("bytes://niu-sync.svg", SYNC),
        Glyph::Sun => ("bytes://niu-sun.svg", SUN),
        Glyph::Moon => ("bytes://niu-moon.svg", MOON),
        Glyph::Branch => ("bytes://niu-branch.svg", BRANCH),
        Glyph::Search => ("bytes://niu-search.svg", SEARCH),
        Glyph::Plus => ("bytes://niu-plus.svg", PLUS),
        Glyph::Link => ("bytes://niu-link.svg", LINK),
        Glyph::PanelLeft => ("bytes://niu-panel-left.svg", PANEL_LEFT),
        Glyph::PanelRight => ("bytes://niu-panel-right.svg", PANEL_RIGHT),
        Glyph::Star => ("bytes://niu-star.svg", STAR),
        Glyph::StarFilled => ("bytes://niu-star-filled.svg", STAR_FILLED),
        Glyph::Doc => ("bytes://niu-doc.svg", DOC),
        Glyph::Folder => ("bytes://niu-folder.svg", FOLDER),
        Glyph::Book => ("bytes://niu-book.svg", BOOK),
        Glyph::Attach => ("bytes://niu-attach.svg", ATTACH),
        Glyph::Lock => ("bytes://niu-lock.svg", LOCK),
        Glyph::Unlock => ("bytes://niu-unlock.svg", UNLOCK),
        Glyph::Close => ("bytes://niu-close.svg", CLOSE),
        Glyph::WinMinimize => ("bytes://niu-win-minimize.svg", WIN_MINIMIZE),
        Glyph::WinMaximize => ("bytes://niu-win-maximize.svg", WIN_MAXIMIZE),
        Glyph::ChevronRight => ("bytes://niu-chevron-right.svg", CHEVRON_RIGHT),
        Glyph::ChevronDown => ("bytes://niu-chevron-down.svg", CHEVRON_DOWN),
        Glyph::ChevronUp => ("bytes://niu-chevron-up.svg", CHEVRON_UP),
        Glyph::Rows => ("bytes://niu-rows.svg", ROWS),
        Glyph::More => ("bytes://niu-more.svg", MORE),
        Glyph::Quote => ("bytes://niu-quote.svg", QUOTE),
        Glyph::Filter => ("bytes://niu-filter.svg", FILTER),
        Glyph::Grid => ("bytes://niu-grid.svg", GRID),
        Glyph::Send => ("bytes://niu-send.svg", SEND),
        Glyph::Check => ("bytes://niu-check.svg", CHECK),
        Glyph::Copy => ("bytes://niu-copy.svg", COPY),
        Glyph::Warn => ("bytes://niu-warn.svg", WARN),
        Glyph::Key => ("bytes://niu-key.svg", KEY),
        Glyph::Refresh => ("bytes://niu-refresh.svg", REFRESH),
        Glyph::CheckCircle => ("bytes://niu-check-circle.svg", CHECK_CIRCLE),
        Glyph::ArrowRight => ("bytes://niu-arrow-right.svg", ARROW_RIGHT),
        Glyph::Expand => ("bytes://niu-expand.svg", EXPAND),
        Glyph::Pause => ("bytes://niu-pause.svg", PAUSE),
        Glyph::Chat => ("bytes://niu-chat.svg", CHAT),
        Glyph::Sparkle => ("bytes://niu-sparkle.svg", SPARKLE),
        Glyph::Trash => ("bytes://niu-trash.svg", TRASH),
        Glyph::Info => ("bytes://niu-info.svg", INFO),
        Glyph::Clock => ("bytes://niu-clock.svg", CLOCK),
        Glyph::Download => ("bytes://niu-download.svg", DOWNLOAD),
        Glyph::Tag => ("bytes://niu-tag.svg", TAG),
    }
}

/// An `Image` for `glyph`, tinted to `color` (the white SVG × tint = color).
pub fn image(glyph: Glyph, color: Color32) -> egui::Image<'static> {
    let (uri, markup) = source(glyph);
    egui::Image::new(egui::ImageSource::Bytes {
        uri: uri.into(),
        bytes: markup.as_bytes().into(),
    })
    .tint(color)
}

/// Add a `size`×`size` icon inline (returns its `Response`).
pub fn show(ui: &mut egui::Ui, glyph: Glyph, size: f32, color: Color32) -> egui::Response {
    ui.add(image(glyph, color).fit_to_exact_size(Vec2::splat(size)))
}

/// Paint a glyph into an explicit `rect` (for hand-laid rows/buttons).
pub fn paint_at(ui: &egui::Ui, rect: Rect, glyph: Glyph, color: Color32) {
    image(glyph, color).paint_at(ui, rect);
}
