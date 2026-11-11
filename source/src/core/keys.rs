//! Virtual-key codes to names and back. Kept free of Windows calls so the
//! mapping itself can be tested on any platform.

pub fn vk_from_name(name: &str) -> Option<u16> {
    let n = name.trim().to_ascii_lowercase();
    if n.is_empty() {
        return None;
    }
    if n.len() == 1 {
        let c = n.as_bytes()[0];
        if c.is_ascii_alphabetic() {
            return Some(c.to_ascii_uppercase() as u16);
        }
        if c.is_ascii_digit() {
            return Some(c as u16);
        }
    }
    if let Some(rest) = n.strip_prefix('f') {
        if let Ok(num) = rest.parse::<u16>() {
            if (1..=24).contains(&num) {
                return Some(0x70 + num - 1); // VK_F1 = 0x70
            }
        }
    }
    if let Some(hex) = n.strip_prefix("vk") {
        return u16::from_str_radix(hex, 16)
            .ok()
            .filter(|v| *v > 0 && *v <= 255);
    }
    if let Some(digit) = n.strip_prefix("numpad") {
        if let Ok(v) = digit.parse::<u16>() {
            if v <= 9 {
                return Some(0x60 + v);
            }
        }
    }
    Some(match n.as_str() {
        "lbutton" => 1,
        "rbutton" => 2,
        "mbutton" => 4,
        "xbutton1" => 5,
        "xbutton2" => 6,
        "home" => 0x24,
        "end" => 0x23,
        "pgup" | "pageup" => 0x21,
        "pgdn" | "pagedown" => 0x22,
        "insert" => 0x2d,
        "capslock" => 0x14,
        "numlock" => 0x90,
        "scrolllock" => 0x91,
        "lwin" => 0x5b,
        "rwin" => 0x5c,
        "space" => 0x20,
        "enter" | "return" => 0x0D,
        "tab" => 0x09,
        "esc" | "escape" => 0x1B,
        "backspace" => 0x08,
        "delete" | "del" => 0x2E,
        "up" => 0x26,
        "down" => 0x28,
        "left" => 0x25,
        "right" => 0x27,
        "lctrl" | "lcontrol" => 0xA2,
        "rctrl" | "rcontrol" => 0xA3,
        "lshift" => 0xA0,
        "rshift" => 0xA1,
        "lalt" => 0xA4,
        "ralt" => 0xA5,
        "numpadmult" => 0x6a,
        "numpadadd" => 0x6b,
        "numpadsub" => 0x6d,
        "numpaddot" => 0x6e,
        "numpaddiv" => 0x6f,
        "semicolon" | "colon" => 0xba,
        "equals" | "plus" => 0xbb,
        "comma" => 0xbc,
        "minus" => 0xbd,
        "period" => 0xbe,
        "slash" | "questionmark" => 0xbf,
        "backtick" => 0xc0,
        "openbracket" | "opencurlybracket" => 0xdb,
        "backslash" | "pipe" => 0xdc,
        "closebracket" | "closecurlybracket" => 0xdd,
        "quote" => 0xde,
        "browserback" => 0xa6,
        _ => return None,
    })
}

pub fn name_from_vk(vk: u16) -> Option<String> {
    Some(match vk {
        0x41..=0x5A => ((vk as u8) as char).to_ascii_lowercase().to_string(),
        0x30..=0x39 => ((vk as u8) as char).to_string(),
        0x70..=0x87 => format!("f{}", vk - 0x70 + 1),
        0x60..=0x69 => format!("numpad{}", vk - 0x60),
        0x21 => "pgup".into(),
        0x22 => "pgdn".into(),
        0x23 => "end".into(),
        0x24 => "home".into(),
        0x2d => "insert".into(),
        0x14 => "capslock".into(),
        0x90 => "numlock".into(),
        0x91 => "scrolllock".into(),
        1 => "LButton".into(),
        2 => "RButton".into(),
        4 => "MButton".into(),
        5 => "XButton1".into(),
        6 => "XButton2".into(),
        0x20 => "space".into(),
        0x0D => "enter".into(),
        0x09 => "tab".into(),
        0x1B => "escape".into(),
        0x08 => "backspace".into(),
        0x2E => "delete".into(),
        0x26 => "up".into(),
        0x28 => "down".into(),
        0x25 => "left".into(),
        0x27 => "right".into(),
        0xA2 => "lctrl".into(),
        0xA3 => "rctrl".into(),
        0xA0 => "lshift".into(),
        0xA1 => "rshift".into(),
        0xA4 => "lalt".into(),
        0xA5 => "ralt".into(),
        1..=255 => format!("vk{:02X}", vk),
        _ => return None,
    })
}

/// What the interface shows for a key. An unbound action shows a dash instead
/// of blowing up, which is the failure the AHK build kept hitting.
pub fn key_display(name: &str) -> String {
    if name.trim().is_empty() {
        return "—".into();
    }
    match name.to_ascii_lowercase().as_str() {
        "lctrl" | "lcontrol" => "L-CTRL".into(),
        "rctrl" | "rcontrol" => "R-CTRL".into(),
        "lshift" => "L-SHIFT".into(),
        "rshift" => "R-SHIFT".into(),
        "lalt" => "L-ALT".into(),
        "ralt" => "R-ALT".into(),
        "escape" | "esc" => "ESC".into(),
        "space" => "SPACE".into(),
        "enter" | "return" => "ENTER".into(),
        "backspace" => "BKSP".into(),
        "delete" => "DEL".into(),
        "tab" => "TAB".into(),
        "xbutton1" => "MOUSE 4".into(),
        "xbutton2" => "MOUSE 5".into(),
        "mbutton" => "MOUSE 3".into(),
        other => other.to_ascii_uppercase(),
    }
}

pub fn choices() -> Vec<(&'static str, u16)> {
    let mut out = vec![("Mouse", 4), ("Mouse", 5), ("Mouse", 6)];
    out.extend((0x70..=0x87).map(|v| ("Function keys", v)));
    out.extend((0x41..=0x5a).map(|v| ("Letters", v)));
    out.extend((0x30..=0x39).map(|v| ("Numbers", v)));
    out.extend((0x60..=0x69).map(|v| ("Numpad", v)));
    out.extend(
        [
            0x08, 0x09, 0x0d, 0x20, 0x21, 0x22, 0x23, 0x24, 0x25, 0x26, 0x27, 0x28, 0x2d, 0x2e,
            0x14, 0x90, 0x91, 0xa0, 0xa1, 0xa2, 0xa3, 0xa4, 0xa5,
        ]
        .map(|v| ("Special keys", v)),
    );
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn letters_digits_and_function_keys_round_trip() {
        for name in [
            "a", "z", "0", "9", "f1", "f6", "f12", "space", "escape", "rctrl",
        ] {
            let vk = vk_from_name(name).unwrap_or_else(|| panic!("no vk for {}", name));
            assert_eq!(
                name_from_vk(vk).as_deref(),
                Some(name),
                "round trip for {}",
                name
            );
        }
    }

    #[test]
    fn function_key_numbers_are_right() {
        assert_eq!(vk_from_name("f1"), Some(0x70));
        assert_eq!(vk_from_name("f12"), Some(0x7B));
        assert_eq!(vk_from_name("f24"), Some(0x87));
        assert_eq!(vk_from_name("f25"), None);
    }

    #[test]
    fn an_unbound_key_shows_a_dash_rather_than_failing() {
        assert_eq!(key_display(""), "—");
        assert_eq!(key_display("  "), "—");
        assert_eq!(key_display("f6"), "F6");
        assert_eq!(key_display("rctrl"), "R-CTRL");
    }

    #[test]
    fn unknown_names_are_rejected_not_guessed() {
        assert_eq!(vk_from_name("nonsense"), None);
        assert_eq!(vk_from_name(""), None);
    }
}
