use crate::recording::{KeyCode, Op};
use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, AtomicU32, Ordering},
        mpsc, Mutex, OnceLock,
    },
    time::Instant,
};
use windows_sys::Win32::{
    Foundation::*,
    Graphics::Gdi::*,
    System::{
        LibraryLoader::GetModuleHandleW,
        Threading::{GetCurrentProcessId, GetCurrentThreadId},
    },
    UI::{Controls::Dialogs::*, Input::KeyboardAndMouse::*, WindowsAndMessaging::*},
};
pub const TAG: usize = 0x50484B36;
static START: OnceLock<Instant> = OnceLock::new();
static EVENTS: OnceLock<mpsc::Sender<NativeEvent>> = OnceLock::new();
pub static REBIND: AtomicBool = AtomicBool::new(false);
pub static CAPTURE: AtomicBool = AtomicBool::new(false);
pub static EDITING: AtomicBool = AtomicBool::new(false);
static THREAD_ID: AtomicU32 = AtomicU32::new(0);
static SWALLOWED_KEYS: Mutex<BTreeSet<u32>> = Mutex::new(BTreeSet::new());
static SWALLOWED_BUTTONS: Mutex<BTreeSet<u8>> = Mutex::new(BTreeSet::new());
pub fn now() -> u64 {
    START.get_or_init(Instant::now).elapsed().as_millis() as u64
}
#[derive(Debug, Clone)]
pub struct NativeEvent {
    pub at: u64,
    pub op: Op,
    pub vk: Option<u16>,
    pub own: bool,
    pub injected: bool,
    pub position: Option<(i32, i32)>,
}
fn own_window(hwnd: HWND) -> bool {
    let mut pid = 0;
    unsafe {
        GetWindowThreadProcessId(hwnd, &mut pid);
        pid == GetCurrentProcessId()
    }
}
pub fn foreground_is_app() -> bool {
    unsafe { own_window(GetForegroundWindow()) }
}
pub fn cursor_is_app() -> bool {
    unsafe {
        let (x, y) = cursor();
        own_window(WindowFromPoint(POINT { x, y }))
    }
}
unsafe extern "system" fn key_hook(code: i32, w: WPARAM, l: LPARAM) -> LRESULT {
    if code >= 0 {
        let kb = &*(l as *const KBDLLHOOKSTRUCT);
        // Ignore our own playback, but accept remappers/accessibility input for
        // binding and hotkeys. The recorder still filters injected events.
        if kb.dwExtraInfo != TAG {
            let down = w as u32 == WM_KEYDOWN || w as u32 == WM_SYSKEYDOWN;
            if down || w as u32 == WM_KEYUP || w as u32 == WM_SYSKEYUP {
                let value = kb.scanCode as u16
                    | if kb.flags & LLKHF_EXTENDED != 0 {
                        0x100
                    } else {
                        0
                    };
                let key = if value & 0xff != 0 {
                    KeyCode { value, scan: true }
                } else {
                    KeyCode::vk(kb.vkCode as u16)
                };
                let mut swallowed = SWALLOWED_KEYS.lock().unwrap();
                let capture = REBIND.load(Ordering::Relaxed)
                    || (CAPTURE.load(Ordering::Relaxed) && kb.vkCode == VK_ESCAPE as u32);
                let swallow = if down {
                    if capture {
                        swallowed.insert(kb.vkCode);
                    }
                    capture
                } else {
                    swallowed.remove(&kb.vkCode) || capture
                };
                drop(swallowed);
                if let Some(tx) = EVENTS.get() {
                    let _ = tx.send(NativeEvent {
                        at: now(),
                        op: Op::Key(key, down),
                        vk: Some(kb.vkCode as u16),
                        own: foreground_is_app(),
                        injected: kb.flags & LLKHF_INJECTED != 0,
                        position: None,
                    });
                }
                if swallow {
                    return 1;
                }
            }
        }
    }
    CallNextHookEx(0, code, w, l)
}
unsafe extern "system" fn mouse_hook(code: i32, w: WPARAM, l: LPARAM) -> LRESULT {
    if code >= 0 {
        let ms = &*(l as *const MSLLHOOKSTRUCT);
        if ms.dwExtraInfo != TAG {
            let op = match w as u32 {
                WM_MOUSEMOVE => Some(Op::Move(ms.pt.x, ms.pt.y)),
                WM_LBUTTONDOWN => Some(Op::Button(1, true)),
                WM_LBUTTONUP => Some(Op::Button(1, false)),
                WM_RBUTTONDOWN => Some(Op::Button(2, true)),
                WM_RBUTTONUP => Some(Op::Button(2, false)),
                WM_MBUTTONDOWN => Some(Op::Button(3, true)),
                WM_MBUTTONUP => Some(Op::Button(3, false)),
                WM_XBUTTONDOWN | WM_XBUTTONUP => Some(Op::Button(
                    if (ms.mouseData >> 16) & 0xffff == 1 {
                        4
                    } else {
                        5
                    },
                    w as u32 == WM_XBUTTONDOWN,
                )),
                WM_MOUSEWHEEL | WM_MOUSEHWHEEL => Some(Op::Wheel(
                    ((ms.mouseData >> 16) as i16) as i32,
                    w as u32 == WM_MOUSEHWHEEL,
                )),
                _ => None,
            };
            let own = own_window(WindowFromPoint(ms.pt));
            if let Some(op) = op {
                let vk = match op {
                    Op::Button(b, _) => Some(match b {
                        1 => 1,
                        2 => 2,
                        3 => 4,
                        4 => 5,
                        _ => 6,
                    }),
                    _ => None,
                };
                let mut swallow = (REBIND.load(Ordering::Relaxed)
                    && matches!(op, Op::Button(3..=5, _)))
                    || (CAPTURE.load(Ordering::Relaxed) && !own && matches!(op, Op::Button(1, _)));
                if let Op::Button(b, down) = op {
                    let mut swallowed = SWALLOWED_BUTTONS.lock().unwrap();
                    if down {
                        if swallow {
                            swallowed.insert(b);
                        }
                    } else {
                        swallow = swallowed.remove(&b) || swallow;
                    }
                }
                if let Some(tx) = EVENTS.get() {
                    let _ = tx.send(NativeEvent {
                        at: now(),
                        op,
                        vk,
                        own,
                        injected: ms.flags & LLMHF_INJECTED != 0,
                        position: Some((ms.pt.x, ms.pt.y)),
                    });
                }
                if swallow {
                    return 1;
                }
            }
        }
    }
    CallNextHookEx(0, code, w, l)
}
pub struct Hooks {
    join: Option<std::thread::JoinHandle<()>>,
}
impl Hooks {
    pub fn start(tx: mpsc::Sender<NativeEvent>) -> Result<Self, String> {
        EVENTS
            .set(tx)
            .map_err(|_| "Input hooks already started".to_string())?;
        START.get_or_init(Instant::now);
        let (ready_tx, ready_rx) = mpsc::channel::<Result<(), String>>();
        let join = std::thread::spawn(move || unsafe {
            THREAD_ID.store(GetCurrentThreadId(), Ordering::SeqCst);
            let mut msg: MSG = std::mem::zeroed();
            PeekMessageW(&mut msg, 0, 0, 0, PM_NOREMOVE);
            let module = GetModuleHandleW(std::ptr::null());
            let keyboard = SetWindowsHookExW(WH_KEYBOARD_LL, Some(key_hook), module, 0);
            let mouse = SetWindowsHookExW(WH_MOUSE_LL, Some(mouse_hook), module, 0);
            if keyboard == 0 || mouse == 0 {
                if keyboard != 0 {
                    UnhookWindowsHookEx(keyboard);
                }
                if mouse != 0 {
                    UnhookWindowsHookEx(mouse);
                }
                let _ = ready_tx.send(Err("Windows input hooks could not start".into()));
                return;
            }
            let _ = ready_tx.send(Ok(()));
            while GetMessageW(&mut msg, 0, 0, 0) > 0 {
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
            UnhookWindowsHookEx(keyboard);
            UnhookWindowsHookEx(mouse);
        });
        ready_rx.recv().map_err(|e| e.to_string())??;
        Ok(Self { join: Some(join) })
    }
}
impl Drop for Hooks {
    fn drop(&mut self) {
        unsafe {
            PostThreadMessageW(THREAD_ID.load(Ordering::SeqCst), WM_QUIT, 0, 0);
        }
        if let Some(j) = self.join.take() {
            let _ = j.join();
        }
    }
}
pub fn cursor() -> (i32, i32) {
    unsafe {
        let mut p = POINT { x: 0, y: 0 };
        GetCursorPos(&mut p);
        (p.x, p.y)
    }
}
pub fn set_cursor(x: i32, y: i32) -> Result<(), String> {
    if unsafe { SetCursorPos(x, y) } == 0 {
        Err("Could not move cursor".into())
    } else {
        Ok(())
    }
}
pub fn send(op: &Op) -> Result<(), String> {
    unsafe {
        if let Op::Move(x, y) = op {
            return set_cursor(*x, *y);
        }
        if matches!(op, Op::Wait) {
            return Ok(());
        }
        let mut input: INPUT = std::mem::zeroed();
        match op {
            Op::Key(k, down) => {
                input.r#type = INPUT_KEYBOARD;
                input.Anonymous.ki = KEYBDINPUT {
                    wVk: if k.scan { 0 } else { k.value },
                    wScan: if k.scan { k.value & 0xff } else { 0 },
                    dwFlags: if k.scan {
                        KEYEVENTF_SCANCODE
                            | if k.value & 0x100 != 0 {
                                KEYEVENTF_EXTENDEDKEY
                            } else {
                                0
                            }
                    } else if matches!(
                        k.value,
                        0x21..=0x28 | 0x2d | 0x2e | 0x5b | 0x5c | 0x6f | 0x90 | 0xa3 | 0xa5
                    ) {
                        KEYEVENTF_EXTENDEDKEY
                    } else {
                        0
                    } | if !down { KEYEVENTF_KEYUP } else { 0 },
                    time: 0,
                    dwExtraInfo: TAG,
                };
            }
            Op::Button(b, down) => {
                input.r#type = INPUT_MOUSE;
                let flags = match (*b, *down) {
                    (1, true) => MOUSEEVENTF_LEFTDOWN,
                    (1, false) => MOUSEEVENTF_LEFTUP,
                    (2, true) => MOUSEEVENTF_RIGHTDOWN,
                    (2, false) => MOUSEEVENTF_RIGHTUP,
                    (3, true) => MOUSEEVENTF_MIDDLEDOWN,
                    (3, false) => MOUSEEVENTF_MIDDLEUP,
                    (4..=5, true) => MOUSEEVENTF_XDOWN,
                    (4..=5, false) => MOUSEEVENTF_XUP,
                    _ => return Err("Invalid mouse button".into()),
                };
                input.Anonymous.mi = MOUSEINPUT {
                    dx: 0,
                    dy: 0,
                    mouseData: if *b == 4 {
                        1
                    } else if *b == 5 {
                        2
                    } else {
                        0
                    },
                    dwFlags: flags,
                    time: 0,
                    dwExtraInfo: TAG,
                };
            }
            Op::Wheel(delta, h) => {
                input.r#type = INPUT_MOUSE;
                input.Anonymous.mi = MOUSEINPUT {
                    dx: 0,
                    dy: 0,
                    mouseData: *delta as u32,
                    dwFlags: if *h {
                        MOUSEEVENTF_HWHEEL
                    } else {
                        MOUSEEVENTF_WHEEL
                    },
                    time: 0,
                    dwExtraInfo: TAG,
                };
            }
            _ => {}
        }
        if SendInput(1, &input, std::mem::size_of::<INPUT>() as i32) != 1 {
            Err("Windows refused input. Check that the target app runs at the same permission level.".into())
        } else {
            Ok(())
        }
    }
}
#[derive(Default)]
pub struct Held {
    keys: BTreeSet<KeyCode>,
    buttons: BTreeSet<u8>,
}
impl Held {
    pub fn apply(&mut self, op: &Op) -> Result<(), String> {
        send(op)?;
        match op {
            Op::Key(k, true) => {
                self.keys.insert(*k);
            }
            Op::Key(k, false) => {
                self.keys.remove(k);
            }
            Op::Button(b, true) => {
                self.buttons.insert(*b);
            }
            Op::Button(b, false) => {
                self.buttons.remove(b);
            }
            _ => {}
        }
        Ok(())
    }
}
impl Drop for Held {
    fn drop(&mut self) {
        for k in &self.keys {
            let _ = send(&Op::Key(*k, false));
        }
        for b in &self.buttons {
            let _ = send(&Op::Button(*b, false));
        }
    }
}
pub fn vk_of(k: KeyCode) -> u16 {
    if k.scan {
        unsafe {
            MapVirtualKeyW(
                (k.value as u32 & 0xff) | if k.value & 0x100 != 0 { 0xe000 } else { 0 },
                MAPVK_VSC_TO_VK_EX,
            ) as u16
        }
    } else {
        k.value
    }
}
pub fn screen_work_area() -> [i32; 4] {
    unsafe {
        let (x, y) = cursor();
        let monitor = MonitorFromPoint(POINT { x, y }, MONITOR_DEFAULTTONEAREST);
        let mut info: MONITORINFO = std::mem::zeroed();
        info.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
        GetMonitorInfoW(monitor, &mut info);
        let r = info.rcWork;
        [r.left, r.top, r.right - r.left, r.bottom - r.top]
    }
}
pub fn visible_position(position: [f32; 2], size: [f32; 2], scale: f32) -> [f32; 2] {
    unsafe {
        let monitor = MonitorFromPoint(
            POINT {
                x: (position[0] * scale) as i32,
                y: (position[1] * scale) as i32,
            },
            MONITOR_DEFAULTTONEAREST,
        );
        let mut info: MONITORINFO = std::mem::zeroed();
        info.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
        if GetMonitorInfoW(monitor, &mut info) == 0 {
            return position;
        }
        let r = info.rcWork;
        let left = r.left as f32 / scale;
        let top = r.top as f32 / scale;
        [
            position[0].clamp(left, (r.right as f32 / scale - size[0]).max(left)),
            position[1].clamp(top, (r.bottom as f32 / scale - size[1] - 32.).max(top)),
        ]
    }
}
pub fn fit_game() -> Result<(), String> {
    unsafe extern "system" fn find(hwnd: HWND, data: LPARAM) -> BOOL {
        unsafe {
            if IsWindowVisible(hwnd) != 0 {
                let mut text = [0u16; 256];
                let n = GetWindowTextW(hwnd, text.as_mut_ptr(), 256);
                if String::from_utf16_lossy(&text[..n as usize])
                    .to_lowercase()
                    .contains("roblox")
                {
                    *(data as *mut HWND) = hwnd;
                    return 0;
                }
            }
        }
        1
    }
    unsafe {
        let mut hwnd: HWND = 0;
        EnumWindows(Some(find), &mut hwnd as *mut HWND as isize);
        if hwnd == 0 {
            return Err("Open Roblox before using Fit game".into());
        }
        let [x, y, w, h] = screen_work_area();
        ShowWindow(hwnd, SW_RESTORE);
        if SetWindowPos(
            hwnd,
            0,
            x + 210,
            y + 44,
            (w - 420).max(300),
            (h - 90).max(200),
            SWP_NOZORDER | SWP_NOACTIVATE,
        ) == 0
        {
            return Err("Could not resize game window".into());
        }
        Ok(())
    }
}
pub fn file_dialog(save: bool, initial: &Path) -> Option<PathBuf> {
    let mut buf = [0u16; 2048];
    let filter: Vec<_> = "Recording (*.rec)\0*.rec\0All files\0*.*\0\0"
        .encode_utf16()
        .collect();
    let title: Vec<_> = (if save {
        "Save recording as"
    } else {
        "Open recording"
    })
    .encode_utf16()
    .chain([0])
    .collect();
    let dir: Vec<_> = initial
        .to_string_lossy()
        .encode_utf16()
        .chain([0])
        .collect();
    let ext: Vec<_> = "rec".encode_utf16().chain([0]).collect();
    unsafe {
        let mut ofn: OPENFILENAMEW = std::mem::zeroed();
        ofn.lStructSize = std::mem::size_of::<OPENFILENAMEW>() as u32;
        ofn.hwndOwner = GetForegroundWindow();
        ofn.lpstrFilter = filter.as_ptr();
        ofn.lpstrFile = buf.as_mut_ptr();
        ofn.nMaxFile = buf.len() as u32;
        ofn.lpstrInitialDir = dir.as_ptr();
        ofn.lpstrTitle = title.as_ptr();
        ofn.lpstrDefExt = ext.as_ptr();
        ofn.Flags = OFN_NOCHANGEDIR
            | OFN_PATHMUSTEXIST
            | if save {
                OFN_OVERWRITEPROMPT
            } else {
                OFN_FILEMUSTEXIST
            };
        let ok = if save {
            GetSaveFileNameW(&mut ofn)
        } else {
            GetOpenFileNameW(&mut ofn)
        };
        if ok == 0 {
            None
        } else {
            Some(
                String::from_utf16_lossy(&buf[..buf.iter().position(|c| *c == 0).unwrap_or(0)])
                    .into(),
            )
        }
    }
}
