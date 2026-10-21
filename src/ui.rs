//! The OneClick Optimizer window. Plain Win32 + GDI/GDI+ painting: no browser
//! engine, no UI framework.
//!
//! Home walks through: Welcome (one big Scan button) -> Scanning (live steps)
//! -> Results (health score, four summary cards, the plan, one big Optimise
//! button) -> Optimising -> Done (what was freed). The sidebar has detail pages
//! for junk files, apps, startup, services and history.

use crate::actions::{self, Action, Journal, JournalEntry, Op, Plan};
use crate::analyze::{self, AppGroup};
use crate::elevate::{self, ElevateError};
use crate::health::Severity;
use crate::model::*;
use crate::optimize::{self, RunSummary, ScanResult, RUN_STEPS, SCAN_STEPS};
use crate::paint::*;
use crate::util::{fmt_bytes, journal_path, wide};
use crate::{icon, junk, kb, sys, APP_NAME};
use std::cell::{Cell, RefCell};
use std::ptr::{null, null_mut};
use std::sync::Mutex;
use windows_sys::Win32::Foundation::{GetLastError, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows_sys::Win32::Graphics::Gdi::{
    BeginPaint, BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, DeleteDC, DeleteObject, EndPaint, InvalidateRect,
    SelectObject, UpdateWindow, HDC, PAINTSTRUCT,
};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::Threading::CreateMutexW;
use windows_sys::Win32::UI::Controls::{ImageList_Create, InitCommonControlsEx, SetWindowTheme, INITCOMMONCONTROLSEX};
use windows_sys::Win32::UI::HiDpi::{GetDpiForSystem, SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_SYSTEM_AWARE};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::EnableWindow;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DispatchMessageW, DrawIconEx, FindWindowW, GetClientRect, GetMessageW,
    GetSystemMetrics, IsDialogMessageW, KillTimer, LoadCursorW, MessageBoxW, MoveWindow, PostMessageW,
    PostQuitMessage, RegisterClassExW, SendMessageW, SetForegroundWindow, SetTimer, ShowWindow, TranslateMessage,
    HICON, IDC_ARROW, MSG, WNDCLASSEXW,
};

// ---------------------------------------------------------------- constants

const WS_OVERLAPPEDWINDOW: u32 = 0x00CF_0000;
const WS_CLIPCHILDREN: u32 = 0x0200_0000;
const WS_CHILD: u32 = 0x4000_0000;
const WS_VISIBLE: u32 = 0x1000_0000;
const WS_TABSTOP: u32 = 0x0001_0000;
const BS_OWNERDRAW: u32 = 0xB;
const LVS_REPORT: u32 = 1;
const LVS_SINGLESEL: u32 = 4;
const LVS_SHOWSELALWAYS: u32 = 8;
const LVS_NOSORTHEADER: u32 = 0x8000;
const LVS_EX_CHECKBOXES: usize = 4;
const LVS_EX_FULLROWSELECT: usize = 0x20;
const LVS_EX_DOUBLEBUFFER: usize = 0x10000;
const LVS_EX_LABELTIP: usize = 0x4000;

const WM_CREATE: u32 = 0x01;
const WM_DESTROY: u32 = 0x02;
const WM_SIZE: u32 = 0x05;
const WM_PAINT: u32 = 0x0F;
const WM_ERASEBKGND: u32 = 0x14;
const WM_SETREDRAW: u32 = 0x0B;
const WM_GETMINMAXINFO: u32 = 0x24;
const WM_DRAWITEM: u32 = 0x2B;
const WM_SETFONT: u32 = 0x30;
const WM_NOTIFY: u32 = 0x4E;
const WM_SETICON: u32 = 0x80;
const WM_COMMAND: u32 = 0x111;
const WM_TIMER: u32 = 0x113;
const WM_CLOSE: u32 = 0x10;
const WM_APP: u32 = 0x8000;
const WM_APP_SCANNED: u32 = WM_APP + 1;
const WM_APP_APPLIED: u32 = WM_APP + 2;
const WM_APP_CHECKS: u32 = WM_APP + 3;
const WM_APP_STEP: u32 = WM_APP + 4;
const WM_APP_OPTIMIZED: u32 = WM_APP + 5;
const WM_APP_UPDATE: u32 = WM_APP + 6;
const WM_APP_UPDATE_DONE: u32 = WM_APP + 7;

/// A newer release found by the background check, waiting to be offered.
static PENDING_UPDATE: Mutex<Option<crate::update::Release>> = Mutex::new(None);
static UPDATE_ERROR: Mutex<Option<String>> = Mutex::new(None);

const LVM_FIRST: u32 = 0x1000;
const LVM_SETIMAGELIST: u32 = LVM_FIRST + 3;
const LVM_GETITEMCOUNT: u32 = LVM_FIRST + 4;
const LVM_DELETEALLITEMS: u32 = LVM_FIRST + 9;
const LVM_ENSUREVISIBLE: u32 = LVM_FIRST + 19;
const LVM_DELETECOLUMN: u32 = LVM_FIRST + 28;
const LVM_GETCOLUMNWIDTH: u32 = LVM_FIRST + 29;
const LVM_SETCOLUMNWIDTH: u32 = LVM_FIRST + 30;
const LVM_SETITEMSTATE: u32 = LVM_FIRST + 43;
const LVM_GETITEMSTATE: u32 = LVM_FIRST + 44;
const LVM_SETEXTENDEDLISTVIEWSTYLE: u32 = LVM_FIRST + 54;
const LVM_INSERTITEMW: u32 = LVM_FIRST + 77;
const LVM_INSERTCOLUMNW: u32 = LVM_FIRST + 97;
const LVM_SETITEMTEXTW: u32 = LVM_FIRST + 116;
const LVCF_WIDTH: u32 = 2;
const LVCF_TEXT: u32 = 4;
const LVCF_SUBITEM: u32 = 8;
const LVIF_TEXT: u32 = 1;
const LVIF_STATE: u32 = 8;
const LVIS_FOCUSED: u32 = 1;
const LVIS_SELECTED: u32 = 2;
const LVIS_STATEIMAGEMASK: u32 = 0xF000;
const LVN_ITEMCHANGED: u32 = (-101i32) as u32;
const NM_CUSTOMDRAW: u32 = (-12i32) as u32;
const CDDS_PREPAINT: u32 = 1;
const CDDS_ITEMPREPAINT: u32 = 0x10001;
const CDDS_SUBITEMPREPAINT: u32 = 0x30001;
const CDRF_NEWFONT: isize = 2;
const CDRF_NOTIFYITEMDRAW: isize = 0x20;
const CDRF_NOTIFYSUBITEMDRAW: isize = 0x20;
const SRCCOPY: u32 = 0x00CC_0020;

const MB_YESNO: u32 = 4;
const MB_ICONQUESTION: u32 = 0x20;
const MB_ICONINFORMATION: u32 = 0x40;
const IDYES: i32 = 6;
const SW_HIDE: i32 = 0;
const SW_SHOW: i32 = 5;
const SW_RESTORE: i32 = 9;
const ERROR_ALREADY_EXISTS: u32 = 183;
const CW_USEDEFAULT: i32 = i32::MIN;

const ID_NAV0: u16 = 300; // 300..=305
const ID_RESCAN: u16 = 200;
const ID_SELECT: u16 = 201;
const ID_PRIMARY: u16 = 204;
const ID_WATCH: u16 = 205;
const ID_SCAN: u16 = 210;
const ID_OPTIMIZE: u16 = 211;
const ID_CARD0: u16 = 220; // 220..=223
const ID_AGAIN: u16 = 230;
const ID_CHANGES: u16 = 231;
const TIMER_WATCH: usize = 2;
const TIMER_SPIN: usize = 3;
const TIMER_LIVE: usize = 4;

pub const CLASS: &str = "OneClickOptimizerWindow";
const AGENT_CLASS: &str = "OneClickOptimizerTray";

// ----------------------------------------------------------------- structs

#[repr(C)]
struct LvColumn {
    mask: u32,
    fmt: i32,
    cx: i32,
    text: *mut u16,
    text_max: i32,
    sub_item: i32,
    image: i32,
    order: i32,
    cx_min: i32,
    cx_default: i32,
    cx_ideal: i32,
}

#[repr(C)]
struct LvItem {
    mask: u32,
    item: i32,
    sub_item: i32,
    state: u32,
    state_mask: u32,
    text: *mut u16,
    text_max: i32,
    image: i32,
    lparam: isize,
    indent: i32,
    group_id: i32,
    columns: u32,
    pu_columns: *mut u32,
    pi_col_fmt: *mut i32,
    group: i32,
}

impl LvItem {
    fn new() -> Self {
        unsafe { std::mem::zeroed() }
    }
}

#[repr(C)]
struct NmHdr {
    hwnd_from: HWND,
    id_from: usize,
    code: u32,
}

#[repr(C)]
struct NmListView {
    hdr: NmHdr,
    item: i32,
    sub_item: i32,
    new_state: u32,
    old_state: u32,
    changed: u32,
    pt: POINT,
    lparam: isize,
}

#[repr(C)]
struct NmCustomDraw {
    hdr: NmHdr,
    stage: u32,
    hdc: HDC,
    rc: RECT,
    item_spec: usize,
    item_state: u32,
    item_lparam: isize,
}

#[repr(C)]
struct NmLvCustomDraw {
    nmcd: NmCustomDraw,
    clr_text: u32,
    clr_text_bk: u32,
    sub_item: i32,
}

#[repr(C)]
struct MinMaxInfo {
    reserved: POINT,
    max_size: POINT,
    max_pos: POINT,
    min_track: POINT,
    max_track: POINT,
}

// ------------------------------------------------------------------- pages

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum View {
    Home,
    Junk,
    Apps,
    Startup,
    Services,
    History,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Home {
    Welcome,
    Scanning,
    Results,
    Optimising,
    Done,
}

struct Page {
    view: View,
    glyph: char,
    nav: &'static str,
    desc: &'static str,
}

const PAGES: [Page; 6] = [
    Page { view: View::Home, glyph: '\u{E80F}', nav: "Home", desc: "" },
    Page {
        view: View::Junk,
        glyph: '\u{E74D}',
        nav: "Junk files",
        desc: "Files your PC doesn't need. Ticked ones are safe; the others are up to you. Deleted files can't be restored.",
    },
    Page {
        view: View::Apps,
        glyph: '\u{E71D}',
        nav: "Apps using memory",
        desc: "Everything running right now, biggest first. Tick an app to close it. Unsaved work in that app is lost.",
    },
    Page {
        view: View::Startup,
        glyph: '\u{E7E8}',
        nav: "Start with Windows",
        desc: "Apps that open by themselves when you sign in. Turning one off doesn't uninstall it.",
    },
    Page {
        view: View::Services,
        glyph: '\u{E713}',
        nav: "Background services",
        desc: "Programs that keep running in the background. \u{201C}When needed\u{201D} means they start only when an app asks for them.",
    },
    Page {
        view: View::History,
        glyph: '\u{E81C}',
        nav: "History & undo",
        desc: "Everything OneClick Optimizer changed. Tick a change to put it back the way it was.",
    },
];

fn page(view: View) -> &'static Page {
    PAGES.iter().find(|p| p.view == view).unwrap_or(&PAGES[0])
}

#[derive(Clone, Copy)]
enum Row {
    Fix(usize),
    Junk(usize),
    Insight,
    App(usize),
    Startup(usize),
    Service(usize),
    Entry(u64),
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tone {
    Muted,
    Good,
    Bad,
}

/// Scan results and journal, shared with worker threads.
#[derive(Default)]
struct Data {
    scan: Option<ScanResult>,
    apps: Vec<AppGroup>,
    services: Vec<usize>,
    journal: Vec<JournalEntry>,
    status: Option<(String, bool)>,
    done: Option<RunSummary>,
    score_before: Option<u8>,
    live: Option<MemInfo>,
}

static DATA: Mutex<Option<Data>> = Mutex::new(None);

fn with_data<R>(f: impl FnOnce(&mut Data) -> R) -> R {
    let mut g = DATA.lock().unwrap_or_else(|e| e.into_inner());
    f(g.get_or_insert_with(Data::default))
}

/// Window handles and fonts. Copied out before any SendMessage, so no borrow
/// is held while Windows re-enters the window procedure.
#[derive(Clone, Copy)]
struct Ui {
    hwnd: HWND,
    nav: [HWND; 6],
    rescan: HWND,
    select: HWND,
    primary: HWND,
    watch: HWND,
    list: HWND,
    scan: HWND,
    optimize: HWND,
    cards: [HWND; 4],
    again: HWND,
    changes: HWND,
    dpi: i32,
    f: Fonts,
    logo: HICON,
}

struct RowInfo {
    row: Row,
    detail: String,
    default_checked: bool,
    tone: Option<Rgb>,
}

thread_local! {
    static UI: Cell<Option<Ui>> = const { Cell::new(None) };
    static VIEW: Cell<View> = const { Cell::new(View::Home) };
    static HOME: Cell<Home> = const { Cell::new(Home::Welcome) };
    static BUSY: Cell<bool> = const { Cell::new(false) };
    static STEP: Cell<usize> = const { Cell::new(0) };
    static SPIN: Cell<f32> = const { Cell::new(0.0) };
    static ROWS: RefCell<Vec<RowInfo>> = const { RefCell::new(Vec::new()) };
    static WIDTHS: RefCell<Vec<i32>> = const { RefCell::new(Vec::new()) };
    /// (accent column, muted column) of the current list; -1 = none.
    static COLSTYLE: Cell<(i32, i32)> = const { Cell::new((-1, -1)) };
    static PRIMARY: RefCell<(String, bool)> = const { RefCell::new((String::new(), false)) };
    static OPT_SUB: RefCell<String> = const { RefCell::new(String::new()) };
    static ALL_CHECKED: Cell<bool> = const { Cell::new(false) };
    static NOTE: RefCell<(String, Tone)> = const { RefCell::new((String::new(), Tone::Muted)) };
    static AGENT_ON: Cell<bool> = const { Cell::new(false) };
    static CHECKS_PENDING: Cell<bool> = const { Cell::new(false) };
}

fn ui() -> Option<Ui> {
    UI.with(Cell::get)
}

fn sc(dpi: i32, v: i32) -> i32 {
    v * dpi / 96
}

fn s(ui: &Ui, v: i32) -> i32 {
    sc(ui.dpi, v)
}

fn view() -> View {
    VIEW.with(Cell::get)
}

fn home() -> Home {
    HOME.with(Cell::get)
}

unsafe fn send(h: HWND, msg: u32, w: usize, l: isize) -> isize {
    SendMessageW(h, msg, w, l)
}

fn set_note(text: &str, tone: Tone) {
    NOTE.with(|n| *n.borrow_mut() = (text.to_string(), tone));
    if let Some(ui) = ui() {
        let g = geom(&ui);
        unsafe { InvalidateRect(ui.hwnd, &g.note, 0) };
    }
}

// ----------------------------------------------------------------- startup

pub fn run() {
    unsafe {
        SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_SYSTEM_AWARE);
        // One window at a time: bring the existing one forward instead.
        CreateMutexW(null(), 0, wide("Local\\OneClickOptimizer.UI").as_ptr());
        if GetLastError() == ERROR_ALREADY_EXISTS {
            let existing = FindWindowW(wide(CLASS).as_ptr(), null());
            if !existing.is_null() {
                ShowWindow(existing, SW_RESTORE);
                SetForegroundWindow(existing);
            }
            return;
        }
        let icc = INITCOMMONCONTROLSEX {
            dwSize: std::mem::size_of::<INITCOMMONCONTROLSEX>() as u32,
            dwICC: 0x1 | 0x4000, // list view, standard
        };
        InitCommonControlsEx(&icc);
        startup();

        let hinst = GetModuleHandleW(null());
        let class = wide(CLASS);
        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            style: 0,
            lpfnWndProc: Some(wndproc),
            cbClsExtra: 0,
            cbWndExtra: 0,
            hInstance: hinst,
            hIcon: icon::app_icon_sized(true),
            hCursor: LoadCursorW(null_mut(), IDC_ARROW),
            hbrBackground: null_mut(),
            lpszMenuName: null(),
            lpszClassName: class.as_ptr(),
            hIconSm: icon::app_icon_sized(false),
        };
        RegisterClassExW(&wc);

        let dpi = GetDpiForSystem() as i32;
        // OCO_OFFSCREEN=1 opens the window outside the visible desktop
        // (used by the screenshot tests so they don't pop up over your work).
        let offscreen = std::env::var_os("OCO_OFFSCREEN").is_some();
        let hwnd = CreateWindowExW(
            0,
            class.as_ptr(),
            wide(APP_NAME).as_ptr(),
            WS_OVERLAPPEDWINDOW | WS_CLIPCHILDREN,
            if offscreen { -10_000 } else { CW_USEDEFAULT },
            if offscreen { -10_000 } else { CW_USEDEFAULT },
            sc(dpi, 1140),
            sc(dpi, 780),
            null_mut(),
            null_mut(),
            hinst,
            null(),
        );
        if hwnd.is_null() {
            return;
        }
        // Tests (off-screen) never take keyboard focus from the user.
        ShowWindow(hwnd, if offscreen { 4 } else { SW_SHOW });
        UpdateWindow(hwnd);
        // `ramdoctor.exe --scan` style launch from the tray: start scanning right away.
        if std::env::args().any(|a| a == "--scan") {
            PostMessageW(hwnd, WM_COMMAND, ID_SCAN as usize, 0);
        }

        let mut msg: MSG = std::mem::zeroed();
        while GetMessageW(&mut msg, null_mut(), 0, 0) > 0 {
            if IsDialogMessageW(hwnd, &msg) == 0 {
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
    }
}

unsafe fn child(parent: HWND, class: &str, style: u32, id: u16) -> HWND {
    CreateWindowExW(
        0,
        wide(class).as_ptr(),
        wide("").as_ptr(),
        WS_CHILD | WS_VISIBLE | style,
        0,
        0,
        10,
        10,
        parent,
        id as usize as _,
        GetModuleHandleW(null()),
        null(),
    )
}

unsafe fn create_controls(hwnd: HWND) {
    let dpi = GetDpiForSystem() as i32;
    let f = Fonts::new(dpi);
    let btn = |id: u16| {
        let h = child(hwnd, "BUTTON", BS_OWNERDRAW | WS_TABSTOP, id);
        add_hover(h);
        h
    };
    let mut nav = [null_mut(); 6];
    for (i, n) in nav.iter_mut().enumerate() {
        *n = btn(ID_NAV0 + i as u16);
    }
    let mut cards = [null_mut(); 4];
    for (i, c) in cards.iter_mut().enumerate() {
        *c = btn(ID_CARD0 + i as u16);
    }
    let rescan = btn(ID_RESCAN);
    let select = btn(ID_SELECT);
    let primary = btn(ID_PRIMARY);
    let watch = btn(ID_WATCH);
    let scan = btn(ID_SCAN);
    let optimize = btn(ID_OPTIMIZE);
    let again = btn(ID_AGAIN);
    let changes = btn(ID_CHANGES);
    let list = child(hwnd, "SysListView32", LVS_REPORT | LVS_SHOWSELALWAYS | LVS_SINGLESEL | LVS_NOSORTHEADER | WS_TABSTOP, 103);

    SetWindowTheme(list, wide("Explorer").as_ptr(), null());
    send(list, WM_SETFONT, f.body as usize, 1);
    send(
        list,
        LVM_SETEXTENDEDLISTVIEWSTYLE,
        0,
        (LVS_EX_CHECKBOXES | LVS_EX_FULLROWSELECT | LVS_EX_DOUBLEBUFFER | LVS_EX_LABELTIP) as isize,
    );
    // A 1-px-wide image list only to make rows taller and easier to read.
    let rows = ImageList_Create(1, sc(dpi, 34), 0x20, 1, 0);
    send(list, LVM_SETIMAGELIST, 1, rows as isize);

    UI.with(|u| {
        u.set(Some(Ui {
            hwnd,
            nav,
            rescan,
            select,
            primary,
            watch,
            list,
            scan,
            optimize,
            cards,
            again,
            changes,
            dpi,
            f,
            logo: icon::app_icon(sc(dpi, 32)),
        }))
    });
    refresh_agent_state();
}

// ------------------------------------------------------------------ layout

#[derive(Clone, Copy)]
struct Geom {
    sidebar: i32,
    x: i32,
    right: i32,
    // Home: welcome / progress / done
    title: RECT,
    sub: RECT,
    circle: RECT,
    below: RECT,
    // Home: results
    hero: RECT,
    cards: RECT,
    section: RECT,
    // Pages
    desc: RECT,
    card: RECT,
    note: RECT,
    bottom: RECT,
}

fn geom(ui: &Ui) -> Geom {
    let mut rc = rect(0, 0, 0, 0);
    unsafe { GetClientRect(ui.hwnd, &mut rc) };
    let (w, h) = (rc.right, rc.bottom);
    let sidebar = s(ui, 236);
    let pad = s(ui, 28);
    let (x, right) = (sidebar + pad, w - pad);
    let cx = (x + right) / 2;
    let z = rect(0, 0, 0, 0);
    let mut g = Geom {
        sidebar,
        x,
        right,
        title: z,
        sub: z,
        circle: z,
        below: z,
        hero: z,
        cards: z,
        section: z,
        desc: z,
        card: z,
        note: z,
        bottom: z,
    };
    match (view(), home()) {
        (View::Home, Home::Results) => {
            g.hero = rect(x, pad, right, pad + s(ui, 168));
            g.cards = rect(x, g.hero.bottom + s(ui, 16), right, g.hero.bottom + s(ui, 16) + s(ui, 92));
            g.section = rect(x, g.cards.bottom + s(ui, 18), right, g.cards.bottom + s(ui, 18) + s(ui, 32));
            g.note = rect(x, h - pad - s(ui, 40), right, h - pad);
            g.card = rect(x, g.section.bottom + s(ui, 8), right, (g.note.top - s(ui, 6)).max(g.section.bottom + s(ui, 90)));
        }
        (View::Home, _) => {
            let d = (h - s(ui, 470)).clamp(s(ui, 150), s(ui, 220));
            let block = s(ui, 48 + 6 + 50 + 24 + 24 + 150) + d;
            let top = ((h - block) / 2).max(pad);
            g.title = rect(x, top, right, top + s(ui, 48));
            g.sub = rect(x + s(ui, 30), g.title.bottom + s(ui, 6), right - s(ui, 30), g.title.bottom + s(ui, 56));
            g.circle = rect(cx - d / 2, g.sub.bottom + s(ui, 24), cx + d / 2, g.sub.bottom + s(ui, 24) + d);
            g.below = rect(x, g.circle.bottom + s(ui, 24), right, (g.circle.bottom + s(ui, 24) + s(ui, 170)).min(h - s(ui, 8)));
        }
        _ => {
            g.title = rect(x, pad, right - s(ui, 320), pad + s(ui, 40));
            g.desc = rect(x, g.title.bottom + s(ui, 2), right, g.title.bottom + s(ui, 26));
            g.bottom = rect(x, h - pad - s(ui, 46), right, h - pad);
            g.note = rect(x, g.bottom.top - s(ui, 50), right, g.bottom.top - s(ui, 8));
            let top = g.desc.bottom + s(ui, 18);
            g.card = rect(x, top, right, (g.note.top - s(ui, 6)).max(top + s(ui, 80)));
        }
    }
    g
}

unsafe fn place(h: HWND, r: RECT, visible: bool) {
    if visible {
        MoveWindow(h, r.left, r.top, r.right - r.left, r.bottom - r.top, 1);
    }
    ShowWindow(h, if visible { SW_SHOW } else { SW_HIDE });
}

unsafe fn layout(ui: &Ui) {
    let g = geom(ui);
    let mut rc = rect(0, 0, 0, 0);
    GetClientRect(ui.hwnd, &mut rc);
    for (i, n) in ui.nav.iter().enumerate() {
        let top = s(ui, 100) + i as i32 * s(ui, 50);
        place(*n, rect(s(ui, 12), top, g.sidebar - s(ui, 12), top + s(ui, 44)), true);
    }
    place(ui.watch, rect(s(ui, 12), rc.bottom - s(ui, 100), g.sidebar - s(ui, 12), rc.bottom - s(ui, 20)), true);

    let v = view();
    let hs = home();
    let results = v == View::Home && hs == Home::Results;
    let page = v != View::Home;

    // Welcome: the big round scan button.
    place(ui.scan, g.circle, v == View::Home && hs == Home::Welcome);

    // Results: optimise button inside the hero, four cards, rescan by the section title.
    let ow = s(ui, 250);
    let oh = s(ui, 76);
    let ot = g.hero.top + (g.hero.bottom - g.hero.top - oh) / 2;
    place(ui.optimize, rect(g.hero.right - s(ui, 24) - ow, ot, g.hero.right - s(ui, 24), ot + oh), results);
    let gap = s(ui, 14);
    let cw = (g.cards.right - g.cards.left - 3 * gap) / 4;
    for (i, c) in ui.cards.iter().enumerate() {
        let l = g.cards.left + i as i32 * (cw + gap);
        place(*c, rect(l, g.cards.top, l + cw, g.cards.bottom), results);
    }
    let (rw, rh) = (s(ui, 118), s(ui, 36));
    if results {
        place(ui.rescan, rect(g.right - rw, g.section.top - s(ui, 2), g.right, g.section.top - s(ui, 2) + rh), true);
    } else {
        place(ui.rescan, rect(g.right - rw, g.title.top + s(ui, 2), g.right, g.title.top + s(ui, 2) + rh), page);
    }

    // Done: two buttons centred under the summary.
    let bw = s(ui, 210);
    let by = g.below.bottom - s(ui, 52);
    let cx = (g.x + g.right) / 2;
    place(ui.again, rect(cx - bw - s(ui, 6), by, cx - s(ui, 6), by + s(ui, 48)), v == View::Home && hs == Home::Done);
    place(ui.changes, rect(cx + s(ui, 6), by, cx + bw + s(ui, 6), by + s(ui, 48)), v == View::Home && hs == Home::Done);

    // Lists.
    let list_visible = results || page;
    place(ui.list, rect(g.card.left + 1, g.card.top + 1, g.card.right - 1, g.card.bottom - 1), list_visible);
    place(ui.select, rect(g.bottom.left, g.bottom.top, g.bottom.left + s(ui, 130), g.bottom.bottom), v == View::Junk);
    let pw = s(ui, 290);
    place(ui.primary, rect(g.bottom.right - pw, g.bottom.top, g.bottom.right, g.bottom.bottom), page);
}

// --------------------------------------------------------------- painting

unsafe fn paint(ui: &Ui, hdc: HDC, w: i32, h: i32) {
    let g = geom(ui);
    fill(hdc, &rect(0, 0, w, h), BG);
    fill(hdc, &rect(0, 0, g.sidebar, h), SIDEBAR);

    // Brand.
    let logo = s(ui, 32);
    DrawIconEx(hdc, s(ui, 22), s(ui, 26), ui.logo, logo, logo, 0, null_mut(), 3);
    text(hdc, APP_NAME, rect(s(ui, 62), s(ui, 22), g.sidebar - s(ui, 6), s(ui, 44)), ui.f.brand, WHITE, DT_SINGLELINE | DT_END_ELLIPSIS);
    text(hdc, &format!("Faster PC in one click \u{00B7} v{}", crate::install::VERSION), rect(s(ui, 62), s(ui, 44), g.sidebar, s(ui, 64)), ui.f.small, SIDEBAR_MUTED, DT_SINGLELINE | DT_END_ELLIPSIS);

    match (view(), home()) {
        (View::Home, Home::Welcome) => paint_welcome(ui, hdc, &g),
        (View::Home, Home::Scanning) => paint_progress(ui, hdc, &g, "Scanning your PC\u{2026}", "Looking at memory, junk files, startup apps and your PC's health.", &SCAN_STEPS),
        (View::Home, Home::Optimising) => paint_progress(ui, hdc, &g, "Optimising\u{2026}", "Hang on, this takes a few seconds. Windows may ask for admin approval once.", &RUN_STEPS),
        (View::Home, Home::Done) => paint_done(ui, hdc, &g),
        (View::Home, Home::Results) => paint_results(ui, hdc, &g),
        (v, _) => paint_page(ui, hdc, &g, v),
    }

    if view() != View::Home || home() == Home::Results {
        let gfx = Gfx::new(hdc);
        gfx.fill_round(&rect(g.card.left - 1, g.card.top - 1, g.card.right + 1, g.card.bottom + 1), s(ui, 10) as f32, CARD, Some(BORDER));
        drop(gfx);
        let (note, tone) = NOTE.with(|n| n.borrow().clone());
        let color = match tone {
            Tone::Muted => MUTED,
            Tone::Good => GOOD,
            Tone::Bad => BAD,
        };
        text(hdc, &note, g.note, if tone == Tone::Muted { ui.f.body } else { ui.f.bold }, color, DT_WORDBREAK | DT_END_ELLIPSIS);
    }
}

unsafe fn paint_welcome(ui: &Ui, hdc: HDC, g: &Geom) {
    text(hdc, "Make your PC faster in one click", g.title, ui.f.big, INK, DT_CENTER | DT_SINGLELINE | DT_VCENTER);
    text(
        hdc,
        "Scan finds what slows your PC: memory hogs, junk files, apps that start by themselves and health problems. Nothing changes until you press Optimise.",
        g.sub,
        ui.f.body,
        MUTED,
        DT_CENTER | DT_WORDBREAK,
    );
    let cx = (g.x + g.right) / 2;
    // Live memory pill.
    if let Some(m) = with_data(|d| d.live.clone()) {
        let (ratio, level) = analyze::pressure(&m);
        let pill = rect(cx - s(ui, 230), g.below.top, cx + s(ui, 230), g.below.top + s(ui, 40));
        let gfx = Gfx::new(hdc);
        gfx.fill_round(&pill, s(ui, 20) as f32, CARD, Some(BORDER));
        gfx.dot((pill.left + s(ui, 24)) as f32, ((pill.top + pill.bottom) / 2) as f32, s(ui, 6) as f32, level_color(level));
        drop(gfx);
        let msg = format!(
            "Right now: {} used on {} of RAM ({:.0}%) \u{2014} {}",
            fmt_bytes(m.commit_used),
            fmt_bytes(m.total_phys),
            ratio * 100.0,
            level.label()
        );
        text(hdc, &msg, rect(pill.left + s(ui, 40), pill.top, pill.right - s(ui, 12), pill.bottom), ui.f.small_bold, INK, DT_SINGLELINE | DT_VCENTER | DT_END_ELLIPSIS);
    }
    // What the scan looks at.
    let chips: [(char, &str, Rgb); 4] = [
        ('\u{E9D9}', "Memory hogs", BAD),
        ('\u{E74D}', "Junk files", WARN),
        ('\u{E7E8}', "Startup apps", BLUE),
        ('\u{E95E}', "PC health", ACCENT),
    ];
    let gap = s(ui, 12);
    let avail = g.right - g.x;
    let cw = ((avail - 3 * gap) / 4).min(s(ui, 170));
    let total = 4 * cw + 3 * gap;
    let top = g.below.top + s(ui, 62);
    let gfx = Gfx::new(hdc);
    for (i, (glyph, label, color)) in chips.iter().enumerate() {
        let l = cx - total / 2 + i as i32 * (cw + gap);
        let r = rect(l, top, l + cw, top + s(ui, 44));
        gfx.fill_round(&r, s(ui, 10) as f32, CARD, Some(BORDER));
        text(hdc, &glyph.to_string(), rect(l + s(ui, 14), r.top, l + s(ui, 40), r.bottom), ui.f.icons, *color, DT_SINGLELINE | DT_VCENTER);
        text(hdc, label, rect(l + s(ui, 42), r.top, r.right - s(ui, 6), r.bottom), ui.f.small_bold, INK, DT_SINGLELINE | DT_VCENTER | DT_END_ELLIPSIS);
    }
}

unsafe fn paint_progress(ui: &Ui, hdc: HDC, g: &Geom, title: &str, sub: &str, steps: &[&str; 4]) {
    text(hdc, title, g.title, ui.f.big, INK, DT_CENTER | DT_SINGLELINE | DT_VCENTER);
    text(hdc, sub, g.sub, ui.f.body, MUTED, DT_CENTER | DT_WORDBREAK);
    let step = STEP.with(Cell::get).min(3);
    let cx = (g.circle.left + g.circle.right) as f32 / 2.0;
    let cy = (g.circle.top + g.circle.bottom) as f32 / 2.0;
    let r = (g.circle.right - g.circle.left) as f32 / 2.0 - s(ui, 12) as f32;
    let gfx = Gfx::new(hdc);
    gfx.arc(cx, cy, r, s(ui, 14) as f32, 0.0, 360.0, TRACK);
    let done_sweep = 360.0 * step as f32 / 4.0;
    if done_sweep > 0.0 {
        gfx.arc(cx, cy, r, s(ui, 14) as f32, -90.0, done_sweep, ACCENT_LIGHT);
    }
    gfx.arc(cx, cy, r, s(ui, 14) as f32, SPIN.with(Cell::get), 70.0, ACCENT);
    drop(gfx);
    let ring = rect(g.circle.left, g.circle.top, g.circle.right, g.circle.bottom);
    let mid = cy as i32;
    text(hdc, &format!("{}/4", step + 1), rect(ring.left, mid - s(ui, 30), ring.right, mid + s(ui, 10)), ui.f.big, INK, DT_CENTER | DT_SINGLELINE | DT_VCENTER);
    text(hdc, "steps", rect(ring.left, mid + s(ui, 10), ring.right, mid + s(ui, 32)), ui.f.small, MUTED, DT_CENTER | DT_SINGLELINE);
    let cxi = cx as i32;
    let col = rect(cxi - s(ui, 170), g.below.top, cxi + s(ui, 200), g.below.bottom);
    for (i, label) in steps.iter().enumerate() {
        let y = col.top + i as i32 * s(ui, 30);
        let (glyph, color) = if i < step {
            ('\u{E73E}', GOOD)
        } else if i == step {
            ('\u{E895}', ACCENT)
        } else {
            ('\u{EA3A}', DISABLED)
        };
        text(hdc, &glyph.to_string(), rect(col.left, y, col.left + s(ui, 26), y + s(ui, 26)), ui.f.icons, color, DT_SINGLELINE | DT_VCENTER);
        text(hdc, label, rect(col.left + s(ui, 32), y, col.right, y + s(ui, 26)), if i == step { ui.f.bold } else { ui.f.body }, if i > step { MUTED } else { INK }, DT_SINGLELINE | DT_VCENTER);
    }
}

unsafe fn paint_done(ui: &Ui, hdc: HDC, g: &Geom) {
    let (done, before, after) = with_data(|d| (d.done.clone(), d.score_before, d.scan.as_ref().map(|s| s.score)));
    let Some(done) = done else { return };
    let title = if done.dry_run { "Test run finished \u{2014} nothing was changed" } else { "Done! Your PC is optimised" };
    text(hdc, title, g.title, ui.f.big, INK, DT_CENTER | DT_SINGLELINE | DT_VCENTER);
    let sub = match (before, after, BUSY.with(Cell::get)) {
        (Some(b), _, true) => format!("Health score was {b}. Checking the new score\u{2026}"),
        (Some(b), Some(a), false) => format!("Health score: {b} \u{2192} {a}"),
        _ => String::new(),
    };
    text(hdc, &sub, g.sub, ui.f.section, if after.is_some_and(|a| Some(a) > before) { GOOD } else { MUTED }, DT_CENTER | DT_WORDBREAK);
    let cx = (g.circle.left + g.circle.right) as f32 / 2.0;
    let cy = (g.circle.top + g.circle.bottom) as f32 / 2.0;
    let r = (g.circle.right - g.circle.left) as f32 / 2.0 - s(ui, 20) as f32;
    let gfx = Gfx::new(hdc);
    gfx.circle(cx, cy, r + s(ui, 12) as f32, (220, 252, 231), None);
    gfx.circle(cx, cy, r, (34, 197, 94), Some((21, 128, 61)));
    drop(gfx);
    text(hdc, "\u{E73E}", g.circle, ui.f.icons_big, WHITE, DT_CENTER | DT_VCENTER | DT_SINGLELINE);
    for (i, line) in done.lines().iter().take(4).enumerate() {
        let y = g.below.top + i as i32 * s(ui, 26);
        text(hdc, line, rect(g.x, y, g.right, y + s(ui, 24)), if i == 0 { ui.f.bold } else { ui.f.body }, INK, DT_CENTER | DT_SINGLELINE | DT_END_ELLIPSIS);
    }
}

unsafe fn paint_results(ui: &Ui, hdc: HDC, g: &Geom) {
    let Some((score, after, fix, junk_b, n, ratio, level)) = with_data(|d| {
        d.scan.as_ref().map(|r| {
            let issues = r.recs.len() + r.junk.iter().filter(|j| j.default_on && j.bytes > 0).count() + r.warnings();
            let (ratio, level) = analyze::pressure(&r.snap.mem);
            (r.score, r.score_after, r.fix_bytes(), r.junk_bytes(), issues, ratio, level)
        })
    }) else {
        return;
    };
    let gfx = Gfx::new(hdc);
    gfx.fill_round(&g.hero, s(ui, 16) as f32, CARD, Some(BORDER));
    let r = s(ui, 54) as f32;
    let cx = g.hero.left as f32 + s(ui, 30) as f32 + r;
    let cy = (g.hero.top + g.hero.bottom) as f32 / 2.0;
    gfx.arc(cx, cy, r, s(ui, 12) as f32, 0.0, 360.0, TRACK);
    let color = score_color(score);
    gfx.arc(cx, cy, r, s(ui, 12) as f32, -90.0, 360.0 * score.max(2) as f32 / 100.0, color);
    drop(gfx);
    let ring = rect((cx - r) as i32, (cy - r) as i32, (cx + r) as i32, (cy + r) as i32);
    let mid = cy as i32;
    text(hdc, &score.to_string(), rect(ring.left, mid - s(ui, 26), ring.right, mid + s(ui, 10)), ui.f.big, color, DT_CENTER | DT_SINGLELINE | DT_VCENTER);
    text(hdc, "health", rect(ring.left, mid + s(ui, 8), ring.right, mid + s(ui, 28)), ui.f.small, MUTED, DT_CENTER | DT_SINGLELINE);

    let tx = (cx + r) as i32 + s(ui, 30);
    let tr = g.hero.right - s(ui, 24) - s(ui, 250) - s(ui, 16);
    let title = if n == 0 { "Your PC is in great shape".to_string() } else if n == 1 { "1 thing is slowing your PC".to_string() } else { format!("{n} things are slowing your PC") };
    text(hdc, &title, rect(tx, g.hero.top + s(ui, 26), tr, g.hero.top + s(ui, 62)), ui.f.title, if n == 0 { GOOD } else { INK }, DT_SINGLELINE | DT_END_ELLIPSIS);
    let line = if fix + junk_b == 0 {
        "Nothing big to free right now. You can still look through the list below.".to_string()
    } else {
        format!("Optimise frees about {} of memory and {} of disk space.", fmt_bytes(fix), fmt_bytes(junk_b))
    };
    text(hdc, &line, rect(tx, g.hero.top + s(ui, 66), tr, g.hero.top + s(ui, 112)), ui.f.body, INK, DT_WORDBREAK | DT_END_ELLIPSIS);
    let stats = format!("Memory {:.0}% of RAM ({})  \u{00B7}  After Optimise: about {after}/100", ratio * 100.0, level.label());
    text(hdc, &stats, rect(tx, g.hero.bottom - s(ui, 42), tr, g.hero.bottom - s(ui, 18)), ui.f.small, MUTED, DT_SINGLELINE | DT_END_ELLIPSIS);

    text(hdc, "What Optimise will do", g.section, ui.f.section, INK, DT_SINGLELINE | DT_VCENTER);
    text(hdc, "Untick anything you want to keep", rect(g.section.left, g.section.top, g.section.right - s(ui, 132), g.section.bottom), ui.f.small, MUTED, DT_SINGLELINE | DT_VCENTER | DT_RIGHT);
}

unsafe fn paint_page(ui: &Ui, hdc: HDC, g: &Geom, v: View) {
    let p = page(v);
    text(hdc, p.nav, g.title, ui.f.title, INK, DT_SINGLELINE | DT_VCENTER);
    text(hdc, p.desc, g.desc, ui.f.body, MUTED, DT_SINGLELINE | DT_END_ELLIPSIS);
    if let Some(m) = with_data(|d| d.scan.as_ref().map(|r| r.snap.mem.clone())) {
        let (ratio, level) = analyze::pressure(&m);
        let pill = rect(g.right - s(ui, 118) - s(ui, 14) - s(ui, 180), g.title.top + s(ui, 2), g.right - s(ui, 118) - s(ui, 14), g.title.top + s(ui, 38));
        let gfx = Gfx::new(hdc);
        gfx.fill_round(&pill, s(ui, 18) as f32, CARD, Some(BORDER));
        gfx.dot((pill.left + s(ui, 18)) as f32, ((pill.top + pill.bottom) / 2) as f32, s(ui, 5) as f32, level_color(level));
        drop(gfx);
        text(hdc, &format!("Memory {:.0}% \u{00B7} {}", ratio * 100.0, level.label()), rect(pill.left + s(ui, 30), pill.top, pill.right - s(ui, 8), pill.bottom), ui.f.small, INK, DT_SINGLELINE | DT_VCENTER);
    }
}

unsafe fn on_paint(ui: &Ui) {
    let mut ps: PAINTSTRUCT = std::mem::zeroed();
    let hdc = BeginPaint(ui.hwnd, &mut ps);
    let mut rc = rect(0, 0, 0, 0);
    GetClientRect(ui.hwnd, &mut rc);
    let (w, h) = (rc.right.max(1), rc.bottom.max(1));
    let mem_dc = CreateCompatibleDC(hdc);
    let bmp = CreateCompatibleBitmap(hdc, w, h);
    let old = SelectObject(mem_dc, bmp);
    paint(ui, mem_dc, w, h);
    BitBlt(hdc, 0, 0, w, h, mem_dc, 0, 0, SRCCOPY);
    SelectObject(mem_dc, old);
    DeleteObject(bmp);
    DeleteDC(mem_dc);
    EndPaint(ui.hwnd, &ps);
}

/// (glyph, value, caption, tint, glyph colour) for the four result cards.
fn card_info(i: usize) -> (char, String, String, Rgb, Rgb) {
    with_data(|d| {
        let Some(r) = &d.scan else { return ('\u{E9D9}', String::new(), String::new(), BG, MUTED) };
        match i {
            0 => ('\u{E9D9}', fmt_bytes(r.fix_bytes()), format!("{} idle program(s) to close", r.idle_programs()), (254, 226, 226), BAD),
            1 => ('\u{E74D}', fmt_bytes(r.junk_bytes()), "of junk files to clean".into(), (254, 243, 199), WARN),
            2 => ('\u{E7E8}', r.startup_items().to_string(), "apps & services start by themselves".into(), (219, 234, 254), BLUE),
            _ => (
                '\u{E95E}',
                r.insights.len().to_string(),
                if r.insights.is_empty() { "health checks all passed".into() } else { "health warnings & tips".into() },
                (204, 251, 241),
                ACCENT,
            ),
        }
    })
}

/// Owner-drawn buttons.
unsafe fn draw_button(ui: &Ui, d: &DrawItem) {
    let hdc = d.hdc;
    let rc = d.rc;
    let hover = hovered(d.hwnd_item);
    let pressed = d.item_state & ODS_SELECTED != 0;
    let disabled = d.item_state & ODS_DISABLED != 0;
    let v = view();
    let id = d.ctl_id as u16;

    if (ID_NAV0..ID_NAV0 + 6).contains(&id) {
        let p = &PAGES[(id - ID_NAV0) as usize];
        let active = p.view == v;
        fill(hdc, &rc, SIDEBAR);
        let gfx = Gfx::new(hdc);
        if active || hover {
            gfx.fill_round(&rc, s(ui, 8) as f32, if active { SIDEBAR_ACTIVE } else { SIDEBAR_HOVER }, None);
        }
        if active {
            gfx.fill_round(&rect(rc.left, rc.top + s(ui, 10), rc.left + s(ui, 4), rc.bottom - s(ui, 10)), 2.0, ACCENT, None);
        }
        drop(gfx);
        let fg = if active { WHITE } else { SIDEBAR_TEXT };
        let ic = if active { (94, 234, 212) } else { SIDEBAR_MUTED };
        text(hdc, &p.glyph.to_string(), rect(rc.left + s(ui, 16), rc.top, rc.left + s(ui, 44), rc.bottom), ui.f.icons, ic, DT_SINGLELINE | DT_VCENTER);
        text(hdc, p.nav, rect(rc.left + s(ui, 48), rc.top, rc.right - s(ui, 8), rc.bottom), if active { ui.f.bold } else { ui.f.body }, fg, DT_SINGLELINE | DT_VCENTER | DT_END_ELLIPSIS);
        return;
    }
    if (ID_CARD0..ID_CARD0 + 4).contains(&id) {
        let (glyph, value, caption, tint, gc) = card_info((id - ID_CARD0) as usize);
        fill(hdc, &rc, BG);
        let gfx = Gfx::new(hdc);
        gfx.fill_round(&rc, s(ui, 12) as f32, CARD, Some(if hover { ACCENT } else { BORDER }));
        let r = s(ui, 20) as f32;
        let (cx, cy) = ((rc.left + s(ui, 16)) as f32 + r, (rc.top + rc.bottom) as f32 / 2.0);
        gfx.circle(cx, cy, r, tint, None);
        drop(gfx);
        let ring = rect((cx - r) as i32, (cy - r) as i32, (cx + r) as i32, (cy + r) as i32);
        text(hdc, &glyph.to_string(), ring, ui.f.icons, gc, DT_CENTER | DT_VCENTER | DT_SINGLELINE);
        let tx = ring.right + s(ui, 12);
        text(hdc, &value, rect(tx, rc.top + s(ui, 14), rc.right - s(ui, 10), rc.top + s(ui, 50)), ui.f.big, INK, DT_SINGLELINE | DT_END_ELLIPSIS);
        text(hdc, &caption, rect(tx, rc.top + s(ui, 50), rc.right - s(ui, 10), rc.bottom - s(ui, 8)), ui.f.small, MUTED, DT_WORDBREAK | DT_END_ELLIPSIS);
        return;
    }

    match id {
        ID_SCAN => {
            fill(hdc, &rc, BG);
            let gfx = Gfx::new(hdc);
            let cx = (rc.left + rc.right) as f32 / 2.0;
            let cy = (rc.top + rc.bottom) as f32 / 2.0;
            let outer = (rc.right - rc.left) as f32 / 2.0;
            gfx.circle(cx, cy, outer - if hover { 0.0 } else { s(ui, 6) as f32 }, if hover { (153, 246, 228) } else { (204, 251, 241) }, None);
            let inner = outer - s(ui, 18) as f32;
            if pressed {
                gfx.circle(cx, cy, inner, ACCENT_DOWN, None);
            } else {
                gfx.circle(cx, cy, inner, ACCENT, Some(BLUE));
            }
            drop(gfx);
            let mid = cy as i32;
            text(hdc, "\u{E721}", rect(rc.left, mid - s(ui, 66), rc.right, mid - s(ui, 18)), ui.f.icons_big, WHITE, DT_CENTER | DT_SINGLELINE | DT_VCENTER);
            text(hdc, "Scan now", rect(rc.left, mid - s(ui, 16), rc.right, mid + s(ui, 26)), ui.f.big, WHITE, DT_CENTER | DT_SINGLELINE | DT_VCENTER);
            text(hdc, "takes about 5 seconds", rect(rc.left, mid + s(ui, 26), rc.right, mid + s(ui, 48)), ui.f.small, (204, 251, 241), DT_CENTER | DT_SINGLELINE);
        }
        ID_OPTIMIZE => {
            let sub = OPT_SUB.with(|o| o.borrow().clone());
            draw_primary(d, CARD, "Optimise now", &sub, &ui.f, Some('\u{E945}'), ui.dpi);
        }
        ID_AGAIN => draw_primary(d, BG, "Scan again", "", &ui.f, Some('\u{E72C}'), ui.dpi),
        ID_CHANGES | ID_RESCAN => {
            fill(hdc, &rc, BG);
            let bg = if hover || pressed { (248, 250, 252) } else { CARD };
            Gfx::new(hdc).fill_round(&rc, s(ui, 10) as f32, bg, Some(if hover { (148, 163, 184) } else { BORDER }));
            let (glyph, label) = if id == ID_RESCAN { ('\u{E72C}', "Rescan") } else { ('\u{E81C}', "See what changed") };
            let fg = if disabled { DISABLED } else { INK };
            let lw = text_width(hdc, label, ui.f.bold) + s(ui, 30);
            let x0 = (rc.left + rc.right) / 2 - lw / 2;
            text(hdc, &glyph.to_string(), rect(x0, rc.top, x0 + s(ui, 24), rc.bottom), ui.f.icons, if disabled { DISABLED } else { ACCENT }, DT_SINGLELINE | DT_VCENTER);
            text(hdc, label, rect(x0 + s(ui, 30), rc.top, rc.right, rc.bottom), ui.f.bold, fg, DT_SINGLELINE | DT_VCENTER);
        }
        ID_PRIMARY => {
            let (label, _) = PRIMARY.with(|p| p.borrow().clone());
            draw_primary(d, BG, &label, "", &ui.f, None, ui.dpi);
        }
        ID_SELECT => {
            fill(hdc, &rc, BG);
            let label = if ALL_CHECKED.with(Cell::get) { "Clear all" } else { "Select all" };
            let fg = if disabled { DISABLED } else if hover { ACCENT_DOWN } else { ACCENT };
            text(hdc, label, rc, ui.f.bold, fg, DT_SINGLELINE | DT_VCENTER);
        }
        ID_WATCH => {
            fill(hdc, &rc, SIDEBAR);
            let on = AGENT_ON.with(Cell::get);
            let gfx = Gfx::new(hdc);
            gfx.fill_round(&rc, s(ui, 10) as f32, if hover { SIDEBAR_ACTIVE } else { SIDEBAR_HOVER }, None);
            let (sw, sh) = (s(ui, 40), s(ui, 22));
            let sx = rc.right - s(ui, 14) - sw;
            let sy = rc.top + s(ui, 14);
            gfx.fill_round(&rect(sx, sy, sx + sw, sy + sh), (sh / 2) as f32, if on { ACCENT } else { (71, 85, 105) }, None);
            let knob = if on { sx + sw - sh / 2 } else { sx + sh / 2 };
            gfx.dot(knob as f32, (sy + sh / 2) as f32, (sh / 2 - s(ui, 3)) as f32, WHITE);
            drop(gfx);
            text(hdc, "Watch in tray", rect(rc.left + s(ui, 14), rc.top + s(ui, 12), sx - s(ui, 6), rc.top + s(ui, 36)), ui.f.bold, WHITE, DT_SINGLELINE);
            let sub = if on { "On \u{2014} click the tray icon for one-click Optimise" } else { "Off \u{2014} turn on for warnings and one-click Optimise" };
            text(hdc, sub, rect(rc.left + s(ui, 14), rc.top + s(ui, 38), rc.right - s(ui, 10), rc.bottom - s(ui, 6)), ui.f.small, SIDEBAR_MUTED, DT_WORDBREAK);
        }
        _ => {}
    }
}

// ---------------------------------------------------------------- list view

/// Pure layout: shrink preferred widths proportionally to fit `avail` pixels,
/// and give the last column whatever is left (no horizontal scrollbar).
fn fit_widths(preferred: &[i32], avail: i32) -> Vec<i32> {
    let n = preferred.len();
    if n == 0 {
        return Vec::new();
    }
    let total: i32 = preferred.iter().sum::<i32>().max(1);
    let scale = (avail as f64 / total as f64).min(1.0);
    let mut out: Vec<i32> = preferred.iter().map(|w| ((*w as f64 * scale) as i32).max(40)).collect();
    let used: i32 = out[..n - 1].iter().sum();
    out[n - 1] = (avail - used).max(60);
    out
}

unsafe fn fit_columns(ui: &Ui) {
    let preferred: Vec<i32> = WIDTHS.with(|w| w.borrow().iter().map(|v| s(ui, *v)).collect());
    let mut rc = rect(0, 0, 0, 0);
    GetClientRect(ui.list, &mut rc);
    // Always leave room for the vertical scrollbar: it can appear after we measure.
    let avail = rc.right - GetSystemMetrics(2 /* SM_CXVSCROLL */) - 2;
    for (i, w) in fit_widths(&preferred, avail).into_iter().enumerate() {
        if send(ui.list, LVM_GETCOLUMNWIDTH, i, 0) != w as isize {
            send(ui.list, LVM_SETCOLUMNWIDTH, i, w as isize);
        }
    }
}

unsafe fn lv_reset(lv: HWND, cols: &[(&str, i32)], dpi: i32) {
    send(lv, LVM_DELETEALLITEMS, 0, 0);
    while send(lv, LVM_DELETECOLUMN, 0, 0) != 0 {}
    for (i, (title, width)) in cols.iter().enumerate() {
        let mut t = wide(title);
        let col = LvColumn {
            mask: LVCF_TEXT | LVCF_WIDTH | LVCF_SUBITEM,
            fmt: 0,
            cx: width * dpi / 96,
            text: t.as_mut_ptr(),
            text_max: 0,
            sub_item: i as i32,
            image: 0,
            order: 0,
            cx_min: 0,
            cx_default: 0,
            cx_ideal: 0,
        };
        send(lv, LVM_INSERTCOLUMNW, i, &col as *const _ as isize);
    }
}

unsafe fn lv_add(lv: HWND, index: i32, cells: &[String], checked: bool, checkable: bool) {
    let mut first = wide(&cells[0]);
    let mut item = LvItem::new();
    item.mask = LVIF_TEXT;
    item.item = index;
    item.text = first.as_mut_ptr();
    let row = send(lv, LVM_INSERTITEMW, 0, &item as *const _ as isize) as i32;
    if row < 0 {
        return;
    }
    for (j, cell) in cells.iter().enumerate().skip(1) {
        let mut t = wide(cell);
        let mut sub = LvItem::new();
        sub.sub_item = j as i32;
        sub.text = t.as_mut_ptr();
        send(lv, LVM_SETITEMTEXTW, row as usize, &sub as *const _ as isize);
    }
    if !checkable {
        // State image 0 = no checkbox at all for rows that can't be acted on.
        let mut st = LvItem::new();
        st.state_mask = LVIS_STATEIMAGEMASK;
        st.state = 0;
        send(lv, LVM_SETITEMSTATE, row as usize, &st as *const _ as isize);
    } else if checked {
        lv_set_check(lv, row, true);
    }
}

/// `row` = -1 applies to every row.
unsafe fn lv_set_check(lv: HWND, row: i32, on: bool) {
    let mut item = LvItem::new();
    item.state_mask = LVIS_STATEIMAGEMASK;
    item.state = (if on { 2 } else { 1 }) << 12;
    send(lv, LVM_SETITEMSTATE, row as isize as usize, &item as *const _ as isize);
}

unsafe fn lv_checked(lv: HWND) -> Vec<usize> {
    let n = send(lv, LVM_GETITEMCOUNT, 0, 0).max(0) as usize;
    (0..n)
        .filter(|&i| (send(lv, LVM_GETITEMSTATE, i, LVIS_STATEIMAGEMASK as isize) as u32 >> 12) == 2)
        .collect()
}

// ------------------------------------------------------------- view models

struct MRow {
    row: Row,
    cells: Vec<String>,
    checked: bool,
    detail: String,
    checkable: bool,
    tone: Option<Rgb>,
}

fn mrow(row: Row, cells: Vec<String>, checked: bool, detail: String) -> MRow {
    MRow { row, cells, checked, detail, checkable: true, tone: None }
}

struct Model {
    cols: Vec<(&'static str, i32)>,
    style: (i32, i32),
    rows: Vec<MRow>,
}

fn saves(b: u64) -> String {
    if b > 0 {
        fmt_bytes(b)
    } else {
        "at restart".into()
    }
}

fn junk_size(j: &junk::Found) -> String {
    if !j.measurable {
        "needs admin".into()
    } else if j.bytes == 0 {
        "empty".into()
    } else {
        fmt_bytes(j.bytes)
    }
}

fn build_model(v: View, d: &Data) -> Model {
    let empty = Model { cols: vec![("", 600)], style: (-1, -1), rows: Vec::new() };
    let Some(scan) = &d.scan else { return empty };
    let snap = &scan.snap;
    match v {
        View::Home => {
            let mut rows: Vec<MRow> = scan
                .recs
                .iter()
                .enumerate()
                .map(|(i, r)| mrow(Row::Fix(i), vec![r.title.clone(), saves(r.saves_bytes), r.why.clone()], r.preselect, format!("{}: {}", r.category.label(), r.why)))
                .collect();
            for (i, j) in scan.junk.iter().enumerate() {
                if j.default_on && (j.bytes > 0 || (!j.measurable && j.admin)) {
                    rows.push(mrow(
                        Row::Junk(i),
                        vec![format!("Clean {}", j.name.to_lowercase()), junk_size(j), j.why.clone()],
                        true,
                        format!("Junk files: {} {}", j.why, if j.admin { "Windows will ask for admin approval." } else { "" }),
                    ));
                }
            }
            for ins in scan.insights.iter() {
                let (tag, tone) = match ins.severity {
                    Severity::Problem => ("Problem", BAD),
                    Severity::Warning => ("Warning", WARN),
                    Severity::Tip => ("Tip", BLUE),
                };
                let mut r = mrow(Row::Insight, vec![format!("{tag}: {}", ins.title), String::new(), ins.detail.clone()], false, format!("{}. {}", ins.title, ins.detail));
                r.checkable = false;
                r.tone = Some(tone);
                rows.push(r);
            }
            Model { cols: vec![("Fix", 420), ("Frees", 100), ("Why", 380)], style: (1, 2), rows }
        }
        View::Junk => Model {
            cols: vec![("Junk", 320), ("Size", 120), ("Files", 80), ("What it is", 420)],
            style: (1, 3),
            rows: scan
                .junk
                .iter()
                .enumerate()
                .map(|(i, j)| {
                    let mut r = mrow(
                        Row::Junk(i),
                        vec![j.name.clone(), junk_size(j), if j.measurable { j.files.to_string() } else { String::new() }, j.why.clone()],
                        j.default_on && j.bytes > 0,
                        format!("{}: {}{}", j.name, j.why, if j.admin { " Windows will ask for admin approval." } else { "" }),
                    );
                    r.checkable = j.bytes > 0 || (!j.measurable && j.admin);
                    r
                })
                .collect(),
        },
        View::Apps => Model {
            cols: vec![("App", 230), ("Memory", 110), ("Processes", 95), ("What it is", 420)],
            style: (1, 3),
            rows: d
                .apps
                .iter()
                .enumerate()
                .map(|(i, g)| {
                    let what = if g.protected {
                        format!("{} \u{2014} part of Windows, can't be closed here", if g.label.is_empty() { "System process" } else { &g.label })
                    } else {
                        g.label.clone()
                    };
                    MRow {
                        row: Row::App(i),
                        cells: vec![g.name.clone(), fmt_bytes(g.private_bytes), g.count.to_string(), what],
                        checked: false,
                        detail: if g.protected {
                            format!("{} is part of Windows, so it won't be closed.", g.name)
                        } else {
                            format!("{} uses {} across {} process(es); {} of it is in fast RAM right now. Tick it to close it.", g.name, fmt_bytes(g.private_bytes), g.count, fmt_bytes(g.working_set))
                        },
                        checkable: !g.protected && !g.targets.is_empty(),
                        tone: None,
                    }
                })
                .collect(),
        },
        View::Startup => Model {
            cols: vec![("App", 300), ("Opens at sign-in", 150), ("Program", 520)],
            style: (-1, 2),
            rows: snap
                .startup
                .iter()
                .enumerate()
                .map(|(i, e)| {
                    let protected = kb::is_protected_startup(&e.name);
                    let mut r = mrow(
                        Row::Startup(i),
                        vec![e.name.clone(), if e.enabled { "Yes".into() } else { "No".into() }, e.command.clone()],
                        false,
                        format!(
                            "{} \u{2014} {} ({}). {}",
                            e.name,
                            if e.enabled { "opens when you sign in" } else { "turned off" },
                            e.location.label(),
                            if protected {
                                "Windows needs this one, so it's left alone.".to_string()
                            } else {
                                format!("Tick it to turn it {}.{}", if e.enabled { "off" } else { "back on" }, if e.location.is_user() { "" } else { " Windows will ask for admin approval." })
                            }
                        ),
                    );
                    r.checkable = !protected;
                    r
                })
                .collect(),
        },
        View::Services => Model {
            cols: vec![("Service", 320), ("Starts", 110), ("Running", 90), ("Memory", 90), ("What it is", 420)],
            style: (3, 4),
            rows: d
                .services
                .iter()
                .map(|&i| {
                    let sv = &snap.services[i];
                    let mem = if sv.running {
                        snap.procs.iter().find(|p| p.pid == sv.pid && !p.name.eq_ignore_ascii_case("svchost.exe")).map_or(String::new(), |p| fmt_bytes(p.private_bytes))
                    } else {
                        String::new()
                    };
                    let protected = kb::is_protected_service(&sv.name);
                    let note = if protected { "Keeps hardware or security working \u{2014} left alone.".to_string() } else { kb::service_advice(&sv.name).unwrap_or("").to_string() };
                    let starts = match sv.start_type {
                        SERVICE_AUTO_START => "Always",
                        SERVICE_DEMAND_START => "When needed",
                        SERVICE_DISABLED => "Never",
                        _ => "By Windows",
                    };
                    MRow {
                        row: Row::Service(i),
                        cells: vec![sv.display.clone(), starts.into(), if sv.running { "Yes".into() } else { "No".into() }, mem, note.clone()],
                        checked: false,
                        detail: format!("{} ({}). {}", sv.display, sv.name, if note.is_empty() { sv.path.clone() } else { note }),
                        checkable: !protected && !(sv.start_type == SERVICE_DEMAND_START && !sv.running),
                        tone: None,
                    }
                })
                .collect(),
        },
        View::History => Model {
            cols: vec![("When", 130), ("What changed", 450), ("Result", 330), ("Status", 120)],
            style: (-1, 2),
            rows: d
                .journal
                .iter()
                .rev()
                .filter(|e| e.undo_of.is_none())
                .map(|e| {
                    let status = if e.undone {
                        "Undone"
                    } else if !e.ok {
                        "Didn't work"
                    } else if e.action.undoable() {
                        "Can undo"
                    } else {
                        "Done"
                    };
                    MRow {
                        row: Row::Entry(e.id),
                        cells: vec![sys::mem::fmt_local(e.time), e.action.describe(), e.message.clone(), status.into()],
                        checked: false,
                        detail: format!("{} \u{2014} {}", e.action.describe(), e.message),
                        checkable: status == "Can undo",
                        tone: None,
                    }
                })
                .collect(),
        },
    }
}

fn default_note(v: View, empty: bool, scanned: bool) -> &'static str {
    if !scanned {
        return "Press Scan now on Home first.";
    }
    match (v, empty) {
        (View::Home, true) => "Nothing to fix right now. Your PC is in good shape.",
        (View::Home, false) => "Click a row to read more. Closed programs start again when you open them; deleted junk can't be restored.",
        (View::History, true) => "No changes yet. Everything OneClick Optimizer changes shows up here.",
        (View::Apps, _) => "Click a row to see details. Tick only apps you don't need right now.",
        (View::Junk, _) => "Ticked categories are safe to clean. Files that are in use are skipped automatically.",
        _ => "Click a row to see details.",
    }
}

unsafe fn render_list(ui: &Ui) {
    let v = view();
    if v == View::Home && home() != Home::Results {
        return;
    }
    let (model, scanned) = with_data(|d| (build_model(v, d), d.scan.is_some()));
    send(ui.list, WM_SETREDRAW, 0, 0);
    ROWS.with(|r| r.borrow_mut().clear());
    lv_reset(ui.list, &model.cols, ui.dpi);
    let stored: Vec<RowInfo> = model
        .rows
        .iter()
        .map(|r| RowInfo { row: r.row, detail: r.detail.clone(), default_checked: r.checked, tone: r.tone })
        .collect();
    ROWS.with(|r| *r.borrow_mut() = stored);
    COLSTYLE.with(|c| c.set(model.style));
    for (i, r) in model.rows.iter().enumerate() {
        lv_add(ui.list, i as i32, &r.cells, r.checked, r.checkable);
    }
    WIDTHS.with(|w| *w.borrow_mut() = model.cols.iter().map(|(_, width)| *width).collect());
    fit_columns(ui);
    send(ui.list, WM_SETREDRAW, 1, 0);
    InvalidateRect(ui.list, null(), 1);
    set_note(default_note(v, model.rows.is_empty(), scanned), Tone::Muted);
    update_checks(ui);
}

/// Recomputes button labels from what is ticked.
unsafe fn update_checks(ui: &Ui) {
    CHECKS_PENDING.with(|c| c.set(false));
    let v = view();
    let checked = lv_checked(ui.list);
    let total = send(ui.list, LVM_GETITEMCOUNT, 0, 0).max(0) as usize;
    let n = checked.len();
    let rows: Vec<Row> = ROWS.with(|r| r.borrow().iter().map(|ri| ri.row).collect());
    // (memory freed, disk freed) by the ticked rows.
    let (ram, disk, scanned) = with_data(|d| {
        let (mut ram, mut disk) = (0u64, 0u64);
        for &i in &checked {
            match rows.get(i) {
                Some(Row::Fix(k)) => ram += d.scan.as_ref().and_then(|s| s.recs.get(*k)).map_or(0, |r| r.saves_bytes),
                Some(Row::Junk(k)) => disk += d.scan.as_ref().and_then(|s| s.junk.get(*k)).map_or(0, |j| j.bytes),
                Some(Row::App(k)) => ram += d.apps.get(*k).map_or(0, |g| g.private_bytes),
                _ => {}
            }
        }
        (ram, disk, d.scan.is_some())
    });
    let bytes = ram + disk;
    let busy = BUSY.with(Cell::get);
    let plural = |n: usize, one: &str, many: &str| if n == 1 { one.to_string() } else { many.to_string() };

    // Home: the big Optimise button.
    let sub = if n == 0 {
        "tick something first".to_string()
    } else {
        optimize::frees_text(ram, disk).unwrap_or_else(|| format!("{n} {}", plural(n, "fix", "fixes")))
    };
    OPT_SUB.with(|o| *o.borrow_mut() = sub);
    EnableWindow(ui.optimize, (n > 0 && !busy) as i32);
    InvalidateRect(ui.optimize, null(), 0);

    let (label, enabled) = if busy {
        ("Working\u{2026}".to_string(), false)
    } else if !scanned {
        ("Scan first (Home)".to_string(), false)
    } else if n == 0 {
        (
            match v {
                View::Junk => "Tick junk to clean",
                View::Apps => "Tick apps to close",
                View::Startup => "Tick apps to switch",
                View::Services => "Tick services to change",
                _ => "Tick changes to undo",
            }
            .to_string(),
            false,
        )
    } else {
        let label = match v {
            View::Junk if bytes > 0 => format!("Clean {}", fmt_bytes(bytes)),
            View::Junk => format!("Clean {n} {}", plural(n, "category", "categories")),
            View::Apps => format!("Close {n} {} ({})", plural(n, "app", "apps"), fmt_bytes(bytes)),
            View::Startup => format!("Switch {n} on/off"),
            View::Services => format!("Start {n} only when needed"),
            _ => format!("Undo {n} {}", plural(n, "change", "changes")),
        };
        (label, true)
    };
    PRIMARY.with(|p| *p.borrow_mut() = (label, enabled));
    EnableWindow(ui.primary, enabled as i32);
    InvalidateRect(ui.primary, null(), 0);
    ALL_CHECKED.with(|a| a.set(total > 0 && n == total));
    InvalidateRect(ui.select, null(), 0);
}

// ------------------------------------------------------------------ actions

fn start_scan(hwnd: HWND, quiet: bool) {
    let Some(ui) = ui() else { return };
    if BUSY.with(Cell::get) {
        return;
    }
    BUSY.with(|b| b.set(true));
    unsafe {
        EnableWindow(ui.rescan, 0);
        if !quiet {
            HOME.with(|h| h.set(Home::Scanning));
            STEP.with(|s| s.set(0));
            SetTimer(hwnd, TIMER_SPIN, 40, None);
            layout(&ui);
            InvalidateRect(hwnd, null(), 0);
        } else {
            set_note("Rescanning\u{2026}", Tone::Muted);
        }
        update_checks(&ui);
    }
    let target = hwnd as usize;
    std::thread::spawn(move || {
        let progress = move |step: usize| unsafe {
            PostMessageW(target as HWND, WM_APP_STEP, step, 0);
        };
        let scan = optimize::deep_scan(&progress);
        let apps = analyze::group_apps(&scan.snap.procs, scan.snap.self_pid);
        let snap = &scan.snap;
        let mut services: Vec<usize> = (0..snap.services.len())
            .filter(|&i| {
                let sv = &snap.services[i];
                kb::service_advice(&sv.name).is_some() || (sv.running && third_party(&sv.path))
            })
            .collect();
        services.sort_by_key(|&i| (!snap.services[i].running, snap.services[i].display.to_lowercase()));
        let journal = Journal::load(&journal_path()).entries;
        with_data(|d| {
            d.scan = Some(scan);
            d.apps = apps;
            d.services = services;
            d.journal = journal;
        });
        unsafe { PostMessageW(target as HWND, WM_APP_SCANNED, quiet as usize, 0) };
    });
}

/// Services not shipped inside Windows itself.
fn third_party(path: &str) -> bool {
    let p = path.to_ascii_lowercase();
    if p.is_empty() || p.contains("\\windows defender\\") {
        return false;
    }
    !(p.contains("\\windows\\system32\\") || p.contains("\\windows\\syswow64\\") || p.starts_with("\\systemroot"))
        || p.contains("\\driverstore\\")
}

fn collect_ops(checked: &[usize]) -> (Vec<Op>, Vec<String>) {
    let rows: Vec<Row> = ROWS.with(|r| r.borrow().iter().map(|ri| ri.row).collect());
    with_data(|d| {
        let mut ops = Vec::new();
        let mut skipped = Vec::new();
        let Some(scan) = &d.scan else { return (ops, skipped) };
        let snap = &scan.snap;
        for &i in checked {
            let Some(row) = rows.get(i) else { continue };
            match *row {
                Row::Fix(k) => ops.push(Op::Apply { action: scan.recs[k].action.clone() }),
                Row::Junk(k) => ops.push(optimize::junk_op(&scan.junk[k])),
                Row::Insight => {}
                Row::App(k) => {
                    let g = &d.apps[k];
                    if g.protected || g.targets.is_empty() {
                        skipped.push(format!("{} (part of Windows)", g.name));
                    } else {
                        ops.push(Op::Apply { action: Action::KillProcesses { label: g.name.clone(), targets: g.targets.clone() } });
                    }
                }
                Row::Startup(k) => {
                    let e = &snap.startup[k];
                    if kb::is_protected_startup(&e.name) {
                        skipped.push(format!("{} (needed by Windows)", e.name));
                    } else {
                        ops.push(Op::Apply { action: Action::SetStartupEnabled { location: e.location, value_name: e.name.clone(), enabled: !e.enabled } });
                    }
                }
                Row::Service(k) => {
                    let sv = &snap.services[k];
                    if kb::is_protected_service(&sv.name) {
                        skipped.push(format!("{} (keeps hardware or security working)", sv.display));
                    } else if sv.start_type == SERVICE_DEMAND_START && !sv.running {
                        skipped.push(format!("{} (already starts only when needed)", sv.display));
                    } else {
                        ops.push(Op::Apply {
                            action: Action::SetServiceStart {
                                name: sv.name.clone(),
                                display: sv.display.clone(),
                                start: if sv.start_type == SERVICE_DISABLED { SERVICE_DISABLED } else { SERVICE_DEMAND_START },
                                stop: sv.running,
                            },
                        });
                    }
                }
                Row::Entry(id) => match d.journal.iter().find(|e| e.id == id) {
                    Some(e) if e.undone => skipped.push(format!("#{id} (already undone)")),
                    Some(e) if !e.ok || !e.action.undoable() => skipped.push(format!("{} (nothing to undo)", e.action.describe())),
                    Some(_) => ops.push(Op::Undo { id }),
                    None => {}
                },
            }
        }
        (ops, skipped)
    })
}

fn describe_op(op: &Op) -> String {
    match op {
        Op::Apply { action } => action.describe(),
        Op::Undo { id } => with_data(|d| {
            d.journal.iter().find(|e| e.id == *id).map_or(format!("Undo change #{id}"), |e| format!("Undo: {}", e.action.describe()))
        }),
    }
}

fn op_needs_admin(op: &Op) -> bool {
    match op {
        Op::Apply { action } => action.needs_admin(),
        Op::Undo { id } => with_data(|d| d.journal.iter().find(|e| e.id == *id).is_some_and(|e| e.action.needs_admin())),
    }
}

unsafe fn confirm(ui: &Ui, ops: &[Op], skipped: &[String], title: &str) -> bool {
    let admin = ops.iter().any(op_needs_admin);
    let kills = ops.iter().any(|op| matches!(op, Op::Apply { action: Action::KillProcesses { .. } }));
    let deletes = ops.iter().any(|op| matches!(op, Op::Apply { action: Action::CleanJunk { .. } }));
    let mut msg = format!("{title}\n\n");
    for op in ops.iter().take(14) {
        msg.push_str(&format!("\u{2022} {}\n", describe_op(op)));
    }
    if ops.len() > 14 {
        msg.push_str(&format!("\u{2022} \u{2026}and {} more\n", ops.len() - 14));
    }
    if !skipped.is_empty() {
        msg.push_str(&format!("\nSkipped: {}\n", skipped.join(", ")));
    }
    if kills {
        msg.push_str("\nApps that are closed lose unsaved work.");
    }
    if deletes {
        msg.push_str("\nDeleted junk files can't be restored.");
    }
    if admin {
        msg.push_str("\nSome changes need admin rights: Windows will ask once.");
    }
    msg.push_str("\n\nContinue?");
    MessageBoxW(ui.hwnd, wide(&msg).as_ptr(), wide(APP_NAME).as_ptr(), MB_YESNO | MB_ICONQUESTION) == IDYES
}

/// The one-click button on Home.
unsafe fn on_optimize(ui: &Ui) {
    if BUSY.with(Cell::get) {
        return;
    }
    let checked = lv_checked(ui.list);
    // Only ask when the user ticked something we didn't recommend (or deletes forever).
    let custom = ROWS.with(|r| {
        let r = r.borrow();
        let picked_extra = checked.iter().any(|&i| r.get(i).is_some_and(|ri| !ri.default_checked));
        picked_extra
    });
    let (ops, skipped) = collect_ops(&checked);
    if ops.is_empty() {
        return;
    }
    if custom && !confirm(ui, &ops, &skipped, "Optimise will:") {
        return;
    }
    let junk_estimate: u64 = with_data(|d| {
        let rows: Vec<Row> = ROWS.with(|r| r.borrow().iter().map(|ri| ri.row).collect());
        checked
            .iter()
            .filter_map(|&i| match rows.get(i) {
                Some(Row::Junk(k)) => d.scan.as_ref().map(|s| s.junk[*k].bytes),
                _ => None,
            })
            .sum()
    });
    with_data(|d| d.score_before = d.scan.as_ref().map(|s| s.score));
    BUSY.with(|b| b.set(true));
    HOME.with(|h| h.set(Home::Optimising));
    STEP.with(|s| s.set(0));
    SetTimer(ui.hwnd, TIMER_SPIN, 40, None);
    layout(ui);
    InvalidateRect(ui.hwnd, null(), 0);
    let target = ui.hwnd as usize;
    std::thread::spawn(move || {
        let progress = move |step: usize| unsafe {
            PostMessageW(target as HWND, WM_APP_STEP, step, 0);
        };
        let summary = optimize::run(ops, junk_estimate, &progress);
        with_data(|d| d.done = Some(summary));
        unsafe { PostMessageW(target as HWND, WM_APP_OPTIMIZED, 0, 0) };
    });
}

/// The main button on the detail pages.
unsafe fn on_apply(ui: &Ui) {
    if BUSY.with(Cell::get) {
        return;
    }
    let checked = lv_checked(ui.list);
    let (ops, skipped) = collect_ops(&checked);
    if ops.is_empty() {
        let msg = if skipped.is_empty() { "Tick at least one row first.".to_string() } else { format!("Nothing to do. Skipped:\n\u{2022} {}", skipped.join("\n\u{2022} ")) };
        MessageBoxW(ui.hwnd, wide(&msg).as_ptr(), wide(APP_NAME).as_ptr(), MB_ICONINFORMATION);
        return;
    }
    if !confirm(ui, &ops, &skipped, "OneClick Optimizer will:") {
        return;
    }
    BUSY.with(|b| b.set(true));
    update_checks(ui);
    set_note("Applying changes\u{2026}", Tone::Muted);
    let target = ui.hwnd as usize;
    std::thread::spawn(move || {
        let path = journal_path();
        let before = Journal::load(&path).entries.len();
        let deferred = actions::execute(&path, ops, false);
        let mut extra = String::new();
        if !deferred.is_empty() {
            let n = deferred.len();
            match elevate::run_elevated(&Plan { journal: path.clone(), ops: deferred }) {
                Ok(_) => {}
                Err(ElevateError::Cancelled) => extra = format!(" {n} change(s) skipped because admin approval was cancelled."),
                Err(ElevateError::Failed(e)) => extra = format!(" {n} change(s) not applied: {e}."),
            }
        }
        let after = Journal::load(&path).entries;
        let new = &after[before.min(after.len())..];
        let ok = new.iter().filter(|e| e.ok).count();
        let freed: u64 = new.iter().map(|e| e.freed_bytes).sum();
        let failed: Vec<String> = new.iter().filter(|e| !e.ok).map(|e| e.message.clone()).collect();
        let mut status = format!("\u{2714} Done: {ok} change(s) applied.");
        if freed > 0 {
            status = format!("\u{2714} Done: {ok} change(s) applied, {} of junk deleted.", fmt_bytes(freed));
        }
        if !failed.is_empty() {
            status = format!("{ok} change(s) applied, {} didn't work ({}).", failed.len(), failed[0]);
        }
        status.push_str(&extra);
        let good = failed.is_empty() && extra.is_empty();
        with_data(|d| d.status = Some((status, good)));
        unsafe { PostMessageW(target as HWND, WM_APP_APPLIED, 0, 0) };
    });
}

unsafe fn switch_view(ui: &Ui, v: View) {
    VIEW.with(|x| x.set(v));
    for n in ui.nav {
        InvalidateRect(n, null(), 0);
    }
    layout(ui);
    InvalidateRect(ui.hwnd, null(), 0);
    render_list(ui);
}

/// Scrolls to and selects the first health row on Home.
unsafe fn show_health(ui: &Ui) {
    let idx = ROWS.with(|r| r.borrow().iter().position(|ri| matches!(ri.row, Row::Insight)));
    if let Some(i) = idx {
        send(ui.list, LVM_ENSUREVISIBLE, i, 0);
        let mut it = LvItem::new();
        it.state_mask = LVIS_SELECTED | LVIS_FOCUSED;
        it.state = LVIS_SELECTED | LVIS_FOCUSED;
        send(ui.list, LVM_SETITEMSTATE, i, &it as *const _ as isize);
        windows_sys::Win32::UI::Input::KeyboardAndMouse::SetFocus(ui.list);
    } else {
        set_note("All health checks passed.", Tone::Good);
    }
}

fn refresh_agent_state() {
    let on = unsafe { !FindWindowW(wide(AGENT_CLASS).as_ptr(), null()).is_null() };
    AGENT_ON.with(|a| a.set(on));
    if let Some(ui) = ui() {
        unsafe { InvalidateRect(ui.watch, null(), 0) };
    }
}

unsafe fn toggle_agent(ui: &Ui) {
    let agent = FindWindowW(wide(AGENT_CLASS).as_ptr(), null());
    if agent.is_null() {
        match std::env::current_exe().and_then(|exe| std::process::Command::new(exe).arg("agent").spawn()) {
            Ok(_) => set_note("Tray watch is on. Click the icon next to the clock any time for one-click Optimise.", Tone::Good),
            Err(e) => set_note(&format!("Couldn't start the tray watch: {e}"), Tone::Bad),
        }
    } else {
        PostMessageW(agent, WM_CLOSE, 0, 0);
        set_note("Tray watch is off.", Tone::Muted);
    }
    SetTimer(ui.hwnd, TIMER_WATCH, 800, None);
}

// ------------------------------------------------------------ window proc

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_CREATE => {
            create_controls(hwnd);
            with_data(|d| d.live = Some(sys::mem::read_mem()));
            SetTimer(hwnd, TIMER_LIVE, 2000, None);
            if let Some(ui) = ui() {
                layout(&ui);
            }
            // Look for a newer release on GitHub in the background.
            let target = hwnd as usize;
            std::thread::spawn(move || {
                if let Ok(Some(r)) = crate::update::check(false) {
                    *PENDING_UPDATE.lock().unwrap_or_else(|e| e.into_inner()) = Some(r);
                    PostMessageW(target as HWND, WM_APP_UPDATE, 0, 0);
                }
            });
            0
        }
        WM_APP_UPDATE => {
            let pending = PENDING_UPDATE.lock().unwrap_or_else(|e| e.into_inner()).take();
            if let Some(r) = pending {
                match crate::update::prompt(hwnd, &r) {
                    crate::update::Choice::Update => {
                        windows_sys::Win32::UI::WindowsAndMessaging::SetWindowTextW(
                            hwnd,
                            wide(&format!("{APP_NAME} \u{2014} downloading version {}\u{2026}", r.version)).as_ptr(),
                        );
                        let target = hwnd as usize;
                        std::thread::spawn(move || {
                            let ok = match crate::update::apply(&r) {
                                Ok(()) => 1,
                                Err(e) => {
                                    *UPDATE_ERROR.lock().unwrap_or_else(|x| x.into_inner()) = Some(e);
                                    0
                                }
                            };
                            PostMessageW(target as HWND, WM_APP_UPDATE_DONE, ok, 0);
                        });
                    }
                    crate::update::Choice::Skip => crate::update::skip(&r.version),
                    crate::update::Choice::Later => {}
                }
            }
            0
        }
        WM_APP_UPDATE_DONE => {
            if wparam == 1 {
                // The new version takes over from here.
                windows_sys::Win32::UI::WindowsAndMessaging::DestroyWindow(hwnd);
            } else {
                windows_sys::Win32::UI::WindowsAndMessaging::SetWindowTextW(hwnd, wide(APP_NAME).as_ptr());
                let e = UPDATE_ERROR.lock().unwrap_or_else(|x| x.into_inner()).take().unwrap_or_default();
                let msg = wide(&format!("The update couldn't be installed: {e}\n\nYour current version keeps working."));
                MessageBoxW(hwnd, msg.as_ptr(), wide(crate::update::DIALOG_TITLE).as_ptr(), MB_ICONINFORMATION);
            }
            0
        }
        WM_SIZE => {
            if let Some(ui) = ui() {
                layout(&ui);
                fit_columns(&ui);
                InvalidateRect(hwnd, null(), 0);
            }
            0
        }
        WM_ERASEBKGND => 1,
        WM_PAINT => {
            match ui() {
                Some(ui) => on_paint(&ui),
                None => return DefWindowProcW(hwnd, msg, wparam, lparam),
            }
            0
        }
        WM_DRAWITEM => {
            if let Some(ui) = ui() {
                draw_button(&ui, &*(lparam as *const DrawItem));
            }
            1
        }
        WM_GETMINMAXINFO => {
            let mmi = &mut *(lparam as *mut MinMaxInfo);
            let dpi = GetDpiForSystem() as i32;
            mmi.min_track = POINT { x: sc(dpi, 1000), y: sc(dpi, 700) };
            0
        }
        WM_COMMAND => {
            let id = (wparam & 0xFFFF) as u16;
            if let Some(ui) = ui() {
                match id {
                    ID_SCAN | ID_AGAIN => {
                        switch_view(&ui, View::Home);
                        start_scan(hwnd, false);
                    }
                    ID_RESCAN => start_scan(hwnd, view() != View::Home),
                    ID_OPTIMIZE => on_optimize(&ui),
                    ID_CHANGES => switch_view(&ui, View::History),
                    ID_SELECT => {
                        let all = ALL_CHECKED.with(Cell::get);
                        // Only rows that have a checkbox.
                        let n = send(ui.list, LVM_GETITEMCOUNT, 0, 0).max(0) as usize;
                        for i in 0..n {
                            if send(ui.list, LVM_GETITEMSTATE, i, LVIS_STATEIMAGEMASK as isize) as u32 >> 12 != 0 {
                                lv_set_check(ui.list, i as i32, !all);
                            }
                        }
                        update_checks(&ui);
                    }
                    ID_PRIMARY => on_apply(&ui),
                    ID_WATCH => toggle_agent(&ui),
                    ID_CARD0 => switch_view(&ui, View::Apps),
                    i if i == ID_CARD0 + 1 => switch_view(&ui, View::Junk),
                    i if i == ID_CARD0 + 2 => switch_view(&ui, View::Startup),
                    i if i == ID_CARD0 + 3 => show_health(&ui),
                    i if (ID_NAV0..ID_NAV0 + 6).contains(&i) => switch_view(&ui, PAGES[(i - ID_NAV0) as usize].view),
                    _ => {}
                }
            }
            0
        }
        WM_TIMER => {
            match wparam {
                TIMER_WATCH => {
                    KillTimer(hwnd, TIMER_WATCH);
                    refresh_agent_state();
                }
                TIMER_SPIN => {
                    SPIN.with(|s| s.set((s.get() + 9.0) % 360.0));
                    if let Some(ui) = ui() {
                        let g = geom(&ui);
                        InvalidateRect(hwnd, &g.circle, 0);
                    }
                }
                TIMER_LIVE => {
                    if view() == View::Home && home() == Home::Welcome {
                        with_data(|d| d.live = Some(sys::mem::read_mem()));
                        if let Some(ui) = ui() {
                            let g = geom(&ui);
                            InvalidateRect(hwnd, &g.below, 0);
                        }
                    }
                }
                _ => {}
            }
            0
        }
        WM_NOTIFY => {
            let hdr = &*(lparam as *const NmHdr);
            let Some(ui) = ui() else { return 0 };
            if hdr.hwnd_from != ui.list {
                return 0;
            }
            if hdr.code == NM_CUSTOMDRAW {
                let cd = &mut *(lparam as *mut NmLvCustomDraw);
                return match cd.nmcd.stage {
                    CDDS_PREPAINT => CDRF_NOTIFYITEMDRAW,
                    CDDS_ITEMPREPAINT => CDRF_NOTIFYSUBITEMDRAW,
                    CDDS_SUBITEMPREPAINT => {
                        let (accent, muted) = COLSTYLE.with(Cell::get);
                        let tone = ROWS.with(|r| r.try_borrow().ok().and_then(|r| r.get(cd.nmcd.item_spec).and_then(|ri| ri.tone)));
                        let (font, color) = match tone {
                            Some(t) if cd.sub_item == 0 => (ui.f.bold, t),
                            _ if cd.sub_item == accent => (ui.f.bold, ACCENT_DOWN),
                            _ if cd.sub_item == muted => (ui.f.body, MUTED),
                            _ => (ui.f.body, INK),
                        };
                        SelectObject(cd.nmcd.hdc, font);
                        cd.clr_text = colorref(color);
                        CDRF_NEWFONT
                    }
                    _ => 0,
                };
            }
            if hdr.code == LVN_ITEMCHANGED {
                let nm = &*(lparam as *const NmListView);
                if nm.changed & LVIF_STATE != 0 {
                    if (nm.new_state ^ nm.old_state) & LVIS_STATEIMAGEMASK != 0 && !CHECKS_PENDING.with(|c| c.replace(true)) {
                        PostMessageW(hwnd, WM_APP_CHECKS, 0, 0);
                    }
                    if nm.new_state & LVIS_SELECTED != 0 && nm.old_state & LVIS_SELECTED == 0 {
                        let detail = ROWS.with(|r| r.try_borrow().ok().and_then(|r| r.get(nm.item as usize).map(|ri| ri.detail.clone())));
                        if let Some(t) = detail {
                            set_note(&t, Tone::Muted);
                        }
                    }
                }
            }
            0
        }
        WM_APP_CHECKS => {
            if let Some(ui) = ui() {
                update_checks(&ui);
            }
            0
        }
        WM_APP_STEP => {
            STEP.with(|s| s.set(wparam));
            InvalidateRect(hwnd, null(), 0);
            0
        }
        WM_APP_SCANNED => {
            if let Some(ui) = ui() {
                BUSY.with(|b| b.set(false));
                EnableWindow(ui.rescan, 1);
                if home() == Home::Scanning {
                    HOME.with(|h| h.set(Home::Results));
                    KillTimer(hwnd, TIMER_SPIN);
                }
                if let Some(level) = with_data(|d| d.scan.as_ref().map(|s| analyze::pressure(&s.snap.mem).1)) {
                    // Window and taskbar icon follow the memory level like the tray.
                    send(hwnd, WM_SETICON, 0, icon::tray_icon(level) as isize);
                }
                layout(&ui);
                InvalidateRect(hwnd, null(), 0);
                for c in ui.cards {
                    InvalidateRect(c, null(), 0);
                }
                render_list(&ui);
                if let Some((status, good)) = with_data(|d| d.status.take()) {
                    set_note(&status, if good { Tone::Good } else { Tone::Bad });
                }
            }
            0
        }
        WM_APP_APPLIED => {
            // Rescan quietly so every page reflects the new state; the result note survives.
            BUSY.with(|b| b.set(false));
            start_scan(hwnd, true);
            0
        }
        WM_APP_OPTIMIZED => {
            KillTimer(hwnd, TIMER_SPIN);
            HOME.with(|h| h.set(Home::Done));
            BUSY.with(|b| b.set(false));
            if let Some(ui) = ui() {
                layout(&ui);
                InvalidateRect(hwnd, null(), 0);
            }
            // Measure the new health score in the background for the Done screen.
            start_scan(hwnd, true);
            0
        }
        WM_DESTROY => {
            PostQuitMessage(0);
            0
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

#[cfg(test)]
mod tests {
    use super::fit_widths;

    #[test]
    fn columns_fit_without_overflow() {
        // Too wide: shrink proportionally, last column fills the rest exactly.
        let w = fit_widths(&[180, 645, 630, 165], 1500);
        assert_eq!(w.iter().sum::<i32>(), 1500);
        assert!(w[0] < 180 && w[1] < 645);
        // Narrow content: keep preferred widths, last column stretches.
        let w = fit_widths(&[100, 100, 100], 1000);
        assert_eq!(w, vec![100, 100, 800]);
        assert!(fit_widths(&[], 500).is_empty());
    }
}
