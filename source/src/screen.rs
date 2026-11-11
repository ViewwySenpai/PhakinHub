//! Captures only the two configured HUD regions while Roblox is foreground.
//! This module sends no input, never focuses a window and never saves images.
use crate::detection::{self, Reading, Region, Settings};
use image::RgbImage;
use windows_sys::Win32::{
    Foundation::*,
    Graphics::Gdi::*,
    System::Threading::{
        OpenProcess, QueryFullProcessImageNameW, PROCESS_QUERY_LIMITED_INFORMATION,
    },
    UI::WindowsAndMessaging::*,
};

fn roblox_foreground() -> Result<HWND, String> {
    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd == 0 || IsIconic(hwnd) != 0 {
            return Err("Waiting for Roblox in the foreground".into());
        }
        let mut pid = 0;
        GetWindowThreadProcessId(hwnd, &mut pid);
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if process == 0 {
            return Err("Waiting for a readable Roblox window".into());
        }
        let mut path = [0u16; 1024];
        let mut len = path.len() as u32;
        let ok = QueryFullProcessImageNameW(process, 0, path.as_mut_ptr(), &mut len);
        CloseHandle(process);
        let filename =
            String::from_utf16_lossy(&path[..len.min(path.len() as u32) as usize]).to_lowercase();
        let filename = filename.rsplit(['\\', '/']).next().unwrap_or("");
        if ok == 0 || !["robloxplayerbeta.exe", "windows10universal.exe"].contains(&filename) {
            return Err("Waiting for Roblox in the foreground".into());
        }
        Ok(hwnd)
    }
}

fn crop(hwnd: HWND, region: Region, width: i32, height: i32) -> Result<RgbImage, String> {
    unsafe {
        let mut rect: RECT = std::mem::zeroed();
        let mut origin = POINT { x: 0, y: 0 };
        if GetClientRect(hwnd, &mut rect) == 0 || ClientToScreen(hwnd, &mut origin) == 0 {
            return Err("Cannot locate the Roblox client area".into());
        }
        let [x, y, w, h] = region
            .pixels(rect.right, rect.bottom)
            .ok_or("Detection region is outside the Roblox client area")?;
        let (x, y) = (origin.x + x, origin.y + y);
        let sx = GetSystemMetrics(SM_XVIRTUALSCREEN);
        let sy = GetSystemMetrics(SM_YVIRTUALSCREEN);
        if x < sx
            || y < sy
            || x + w > sx + GetSystemMetrics(SM_CXVIRTUALSCREEN)
            || y + h > sy + GetSystemMetrics(SM_CYVIRTUALSCREEN)
        {
            return Err("Move the Roblox HUD fully onto a screen".into());
        }
        let screen = GetDC(0);
        if screen == 0 {
            return Err("Screen capture is unavailable".into());
        }
        let memory = CreateCompatibleDC(screen);
        if memory == 0 {
            ReleaseDC(0, screen);
            return Err("Cannot create capture buffer".into());
        }
        let bitmap = CreateCompatibleBitmap(screen, width, height);
        if bitmap == 0 {
            DeleteDC(memory);
            ReleaseDC(0, screen);
            return Err("Cannot create capture bitmap".into());
        }
        let old = SelectObject(memory, bitmap);
        SetStretchBltMode(memory, HALFTONE);
        SetBrushOrgEx(memory, 0, 0, std::ptr::null_mut());
        let copied = StretchBlt(
            memory,
            0,
            0,
            width,
            height,
            screen,
            x,
            y,
            w,
            h,
            SRCCOPY | CAPTUREBLT,
        );
        // GetDIBits requires the bitmap not to be selected into a DC.
        SelectObject(memory, old);
        let mut info: BITMAPINFO = std::mem::zeroed();
        info.bmiHeader.biSize = std::mem::size_of::<BITMAPINFOHEADER>() as u32;
        info.bmiHeader.biWidth = width;
        info.bmiHeader.biHeight = -height; // top-down BGRA
        info.bmiHeader.biPlanes = 1;
        info.bmiHeader.biBitCount = 32;
        info.bmiHeader.biCompression = BI_RGB;
        let mut pixels = vec![0u8; (width * height * 4) as usize];
        let rows = if copied != 0 {
            GetDIBits(
                screen,
                bitmap,
                0,
                height as u32,
                pixels.as_mut_ptr().cast(),
                &mut info,
                DIB_RGB_COLORS,
            )
        } else {
            0
        };
        DeleteObject(bitmap);
        DeleteDC(memory);
        ReleaseDC(0, screen);
        if rows != height {
            return Err("Could not read the Roblox HUD".into());
        }
        let rgb: Vec<_> = pixels
            .chunks_exact(4)
            .flat_map(|p| [p[2], p[1], p[0]])
            .collect();
        RgbImage::from_raw(width as u32, height as u32, rgb)
            .ok_or("Invalid capture dimensions".into())
    }
}

pub fn read(settings: &Settings) -> Result<Reading, String> {
    let hwnd = roblox_foreground()?;
    let mut reading = Reading::default();
    if settings.speed_auto {
        reading.speed = detection::speed(&crop(hwnd, settings.speed_region, 90, 60)?);
    }
    if settings.tournament_auto {
        reading.wave = detection::wave(&crop(hwnd, settings.wave_region, 335, 73)?);
    }
    if unsafe { GetForegroundWindow() } != hwnd {
        return Err("Waiting for Roblox in the foreground".into());
    }
    Ok(reading)
}
