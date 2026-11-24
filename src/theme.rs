//! Colours. One built-in theme (One Dark, as used by Zed) for now; change the
//! values here, or add a second constructor and a config switch.

use crate::highlight::Token;
use ratatui::style::Color;
use std::sync::OnceLock;

/// 24-bit colour when the terminal advertises it, otherwise the nearest
/// entry in the 256-colour palette (older macOS Terminal.app, plain tmux).
fn rgb(hex: u32) -> Color {
    static TRUECOLOR: OnceLock<bool> = OnceLock::new();
    let (r, g, b) = ((hex >> 16) as u8, (hex >> 8) as u8, hex as u8);
    let truecolor = *TRUECOLOR.get_or_init(|| {
        std::env::var("COLORTERM")
            .map(|v| v.contains("truecolor") || v.contains("24bit"))
            .unwrap_or(false)
    });
    if truecolor {
        return Color::Rgb(r, g, b);
    }
    to_256(r, g, b)
}

/// The welcome logo's left-to-right gradient: green, violet, blue.
const LOGO_STOPS: [u32; 3] = [0x6ee7b7, 0xa78bfa, 0x5eb6f7];

/// The logo colour `t` of the way across it (0.0 = left edge, 1.0 = right).
pub fn logo_gradient(t: f32) -> Color {
    let t = t.clamp(0.0, 1.0) * (LOGO_STOPS.len() - 1) as f32;
    let i = (t as usize).min(LOGO_STOPS.len() - 2);
    let (a, b, f) = (LOGO_STOPS[i], LOGO_STOPS[i + 1], t - i as f32);
    let mix = |shift: u32| {
        let (x, y) = ((a >> shift) & 0xff, (b >> shift) & 0xff);
        (x as f32 + (y as f32 - x as f32) * f).round() as u32
    };
    rgb(mix(16) << 16 | mix(8) << 8 | mix(0))
}

/// Parse a `"#rrggbb"` (or `"rrggbb"`) hex string into a [`Color`], applying
/// the same truecolor/256-colour fallback as the built-in theme. Used for
/// colours that come from outside the fixed palette above, such as the
/// per-filetype icon colours from the `devicons` crate. Falls back to a grey
/// if the string isn't valid hex.
pub fn parse_hex(hex: &str) -> Color {
    let hex = hex.trim_start_matches('#');
    match u32::from_str_radix(hex, 16) {
        Ok(v) => rgb(v),
        Err(_) => rgb(0x7e8ea8),
    }
}

fn to_256(r: u8, g: u8, b: u8) -> Color {
    // Nearest of the 6x6x6 colour cube or the 24-step grey ramp.
    const LEVELS: [i32; 6] = [0, 95, 135, 175, 215, 255];
    let nearest = |v: u8| {
        (0..6)
            .min_by_key(|i| (LEVELS[*i] - v as i32).abs())
            .unwrap()
    };
    let dist = |c: (i32, i32, i32)| {
        (c.0 - r as i32).pow(2) + (c.1 - g as i32).pow(2) + (c.2 - b as i32).pow(2)
    };
    let (ri, gi, bi) = (nearest(r), nearest(g), nearest(b));
    let grey = (((r as i32 + g as i32 + b as i32) / 3 - 8).clamp(0, 230) + 5) / 10;
    let grey_level = 8 + 10 * grey;
    if dist((grey_level, grey_level, grey_level)) <= dist((LEVELS[ri], LEVELS[gi], LEVELS[bi])) {
        Color::Indexed(232 + grey as u8)
    } else {
        Color::Indexed((16 + 36 * ri + 6 * gi + bi) as u8)
    }
}

pub struct Theme {
    pub bg: Color,
    pub fg: Color,
    pub panel: Color,
    pub border: Color,
    pub dim: Color,
    pub line: Color,
    pub line_no: Color,
    pub selection: Color,
    pub find: Color,
    pub accent: Color,
    pub popup: Color,
    pub popup_sel: Color,
    pub error: Color,
    pub warning: Color,
    /// The ● on a tab with unsaved changes.
    pub modified: Color,
    pub info: Color,
    pub comment: Color,
    pub string: Color,
    pub keyword: Color,
    pub function: Color,
    pub type_: Color,
    pub constant: Color,
    pub property: Color,
    pub operator: Color,
}

/// Identifies one of the built-in themes, for the Settings panel and for
/// `config.toml`'s `theme = "..."` key.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ThemeId {
    OneDark,
    Monokai,
    CatppuccinMocha,
    Dracula,
}

pub const ALL_THEMES: &[ThemeId] = &[
    ThemeId::OneDark,
    ThemeId::Monokai,
    ThemeId::CatppuccinMocha,
    ThemeId::Dracula,
];

impl ThemeId {
    pub fn label(self) -> &'static str {
        match self {
            ThemeId::OneDark => "One Dark",
            ThemeId::Monokai => "Monokai",
            ThemeId::CatppuccinMocha => "Catppuccin Mocha",
            ThemeId::Dracula => "Dracula",
        }
    }

    /// The id stored in `config.toml`.
    pub fn id(self) -> &'static str {
        match self {
            ThemeId::OneDark => "one_dark",
            ThemeId::Monokai => "monokai",
            ThemeId::CatppuccinMocha => "catppuccin_mocha",
            ThemeId::Dracula => "dracula",
        }
    }

    pub fn from_id(s: &str) -> ThemeId {
        match s {
            "monokai" => ThemeId::Monokai,
            "catppuccin_mocha" | "catppuccin" => ThemeId::CatppuccinMocha,
            "dracula" => ThemeId::Dracula,
            _ => ThemeId::OneDark,
        }
    }

    pub fn theme(self) -> Theme {
        match self {
            ThemeId::OneDark => Theme::one_dark(),
            ThemeId::Monokai => Theme::monokai(),
            ThemeId::CatppuccinMocha => Theme::catppuccin_mocha(),
            ThemeId::Dracula => Theme::dracula(),
        }
    }
}

impl Theme {
    /// One Dark (Atom/Zed's default), with a pure black editor background.
    pub fn one_dark() -> Theme {
        Theme {
            bg: rgb(0x000000),
            fg: rgb(0xabb2bf),
            panel: rgb(0x21252b),
            border: rgb(0x3b414d),
            dim: rgb(0x7f848e),
            line: rgb(0x2c313a),
            line_no: rgb(0x4b5263),
            selection: rgb(0x3e4451),
            find: rgb(0x5c4a1e),
            accent: rgb(0x528bff),
            popup: rgb(0x1e2227),
            popup_sel: rgb(0x2f3542),
            error: rgb(0xe06c75),
            warning: rgb(0xe5c07b),
            modified: rgb(0xffc009),
            info: rgb(0x61afef),
            comment: rgb(0x5c6370),
            string: rgb(0x98c379),
            keyword: rgb(0xc678dd),
            function: rgb(0x61afef),
            type_: rgb(0xe5c07b),
            constant: rgb(0xd19a66),
            property: rgb(0xe06c75),
            operator: rgb(0x56b6c2),
        }
    }

    /// Monokai Classic (Wimer Hazenberg).
    pub fn monokai() -> Theme {
        Theme {
            bg: rgb(0x272822),
            fg: rgb(0xf8f8f2),
            panel: rgb(0x1e1f1c),
            border: rgb(0x414339),
            dim: rgb(0x75715e),
            line: rgb(0x3c3d37),
            line_no: rgb(0x90908a),
            selection: rgb(0x49483e),
            find: rgb(0x5c4a1e),
            accent: rgb(0x66d9ef),
            popup: rgb(0x1e1f1c),
            popup_sel: rgb(0x3c3d37),
            error: rgb(0xf92672),
            warning: rgb(0xe6db74),
            modified: rgb(0xffc009),
            info: rgb(0x66d9ef),
            comment: rgb(0x75715e),
            string: rgb(0xe6db74),
            keyword: rgb(0xf92672),
            function: rgb(0xa6e22e),
            type_: rgb(0x66d9ef),
            constant: rgb(0xae81ff),
            property: rgb(0xf8f8f2),
            operator: rgb(0xf92672),
        }
    }

    /// Catppuccin Mocha (the catppuccin project's official dark flavour).
    pub fn catppuccin_mocha() -> Theme {
        Theme {
            bg: rgb(0x1e1e2e),
            fg: rgb(0xcdd6f4),
            panel: rgb(0x181825),
            border: rgb(0x313244),
            dim: rgb(0x6c7086),
            line: rgb(0x313244),
            line_no: rgb(0x585b70),
            selection: rgb(0x45475a),
            find: rgb(0x5c4a1e),
            accent: rgb(0x89b4fa),
            popup: rgb(0x11111b),
            popup_sel: rgb(0x313244),
            error: rgb(0xf38ba8),
            warning: rgb(0xf9e2af),
            modified: rgb(0xffc009),
            info: rgb(0x89dceb),
            comment: rgb(0x9399b2),
            string: rgb(0xa6e3a1),
            keyword: rgb(0xcba6f7),
            function: rgb(0x89b4fa),
            type_: rgb(0xf9e2af),
            constant: rgb(0xfab387),
            property: rgb(0xf38ba8),
            operator: rgb(0x94e2d5),
        }
    }

    /// Dracula (draculatheme.com's standard palette).
    pub fn dracula() -> Theme {
        Theme {
            bg: rgb(0x282a36),
            fg: rgb(0xf8f8f2),
            panel: rgb(0x21222c),
            border: rgb(0x44475a),
            dim: rgb(0x6272a4),
            line: rgb(0x44475a),
            line_no: rgb(0x6272a4),
            selection: rgb(0x44475a),
            find: rgb(0x5c4a1e),
            accent: rgb(0xbd93f9),
            popup: rgb(0x21222c),
            popup_sel: rgb(0x44475a),
            error: rgb(0xff5555),
            warning: rgb(0xf1fa8c),
            modified: rgb(0xffc009),
            info: rgb(0x8be9fd),
            comment: rgb(0x6272a4),
            string: rgb(0xf1fa8c),
            keyword: rgb(0xff79c6),
            function: rgb(0x50fa7b),
            type_: rgb(0x8be9fd),
            constant: rgb(0xbd93f9),
            property: rgb(0xff79c6),
            operator: rgb(0xff79c6),
        }
    }

    pub fn token(&self, t: Token) -> Color {
        match t {
            Token::Comment => self.comment,
            Token::String => self.string,
            Token::Keyword => self.keyword,
            Token::Function => self.function,
            Token::Type => self.type_,
            Token::Constant => self.constant,
            Token::Property | Token::Tag => self.property,
            Token::Attribute => self.constant,
            Token::Operator | Token::Escape => self.operator,
            Token::Title => self.property,
            Token::Punct | Token::Variable => self.fg,
        }
    }

    pub fn severity(&self, sev: u8) -> Color {
        match sev {
            1 => self.error,
            2 => self.warning,
            _ => self.info,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dark_backgrounds_fall_back_to_greys_not_tinted_cube_colours() {
        assert_eq!(to_256(0x28, 0x2c, 0x34), Color::Indexed(236));
        assert_eq!(to_256(0x2c, 0x31, 0x3a), Color::Indexed(236));
        assert_eq!(to_256(0xc6, 0x78, 0xdd), Color::Indexed(176));
        assert_eq!(to_256(0xff, 0xff, 0xff), Color::Indexed(231));
    }
}
