//! The ten colour themes (named after CopyClip 2's themes).

/// An RGB colour.
pub type Rgb = [u8; 3];

/// A colour theme for the clip window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Theme {
    pub name: &'static str,
    /// Use the dark base style.
    pub dark: bool,
    /// Window / panel background.
    pub background: Rgb,
    /// Background for text fields and the clip list.
    pub surface: Rgb,
    /// Main text colour.
    pub text: Rgb,
    /// Accent for selection and highlights.
    pub accent: Rgb,
}

const fn t(
    name: &'static str,
    dark: bool,
    background: Rgb,
    surface: Rgb,
    text: Rgb,
    accent: Rgb,
) -> Theme {
    Theme {
        name,
        dark,
        background,
        surface,
        text,
        accent,
    }
}

/// All built-in themes; the first one is the default.
pub const THEMES: [Theme; 10] = [
    t(
        "Light",
        false,
        [246, 246, 246],
        [255, 255, 255],
        [28, 28, 30],
        [10, 132, 255],
    ),
    t(
        "Dark",
        true,
        [30, 30, 32],
        [44, 44, 46],
        [235, 235, 240],
        [10, 132, 255],
    ),
    t(
        "Midnight Blue",
        true,
        [16, 24, 48],
        [26, 38, 70],
        [220, 228, 250],
        [94, 156, 255],
    ),
    t(
        "Forest",
        true,
        [22, 40, 30],
        [32, 56, 42],
        [222, 240, 226],
        [96, 196, 120],
    ),
    t(
        "Sunset",
        false,
        [255, 240, 228],
        [255, 250, 245],
        [70, 34, 20],
        [240, 110, 60],
    ),
    t(
        "Lavender",
        false,
        [242, 236, 252],
        [250, 247, 255],
        [52, 36, 80],
        [150, 110, 220],
    ),
    t(
        "Graphite",
        true,
        [52, 54, 58],
        [66, 68, 73],
        [230, 230, 232],
        [160, 164, 172],
    ),
    t(
        "Ocean",
        false,
        [228, 244, 250],
        [244, 251, 254],
        [14, 54, 72],
        [20, 150, 190],
    ),
    t(
        "Rose",
        false,
        [252, 236, 240],
        [255, 247, 249],
        [86, 28, 44],
        [220, 80, 120],
    ),
    t(
        "Slate",
        true,
        [38, 46, 56],
        [50, 60, 72],
        [220, 228, 236],
        [120, 150, 180],
    ),
];

/// The default theme.
pub fn default_theme() -> &'static Theme {
    &THEMES[0]
}

/// Look up a theme by name, ignoring case, spaces and dashes.
/// Unknown names fall back to the default theme.
pub fn by_name(name: &str) -> &'static Theme {
    let key = normalize(name);
    THEMES
        .iter()
        .find(|t| normalize(t.name) == key)
        .unwrap_or_else(default_theme)
}

/// Whether a theme with this name exists.
pub fn exists(name: &str) -> bool {
    let key = normalize(name);
    THEMES.iter().any(|t| normalize(t.name) == key)
}

fn normalize(s: &str) -> String {
    s.chars()
        .filter(|c| !c.is_whitespace() && *c != '-' && *c != '_')
        .flat_map(char::to_lowercase)
        .collect()
}

/// Relative luminance (0..1) used to verify text contrast.
pub fn luminance(c: Rgb) -> f64 {
    let ch = |v: u8| {
        let v = v as f64 / 255.0;
        if v <= 0.039_28 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * ch(c[0]) + 0.7152 * ch(c[1]) + 0.0722 * ch(c[2])
}

/// WCAG contrast ratio between two colours.
pub fn contrast(a: Rgb, b: Rgb) -> f64 {
    let (la, lb) = (luminance(a), luminance(b));
    let (hi, lo) = if la > lb { (la, lb) } else { (lb, la) };
    (hi + 0.05) / (lo + 0.05)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn there_are_ten_unique_themes() {
        let mut names: Vec<_> = THEMES.iter().map(|t| normalize(t.name)).collect();
        names.sort();
        names.dedup();
        assert_eq!(names.len(), 10);
    }

    #[test]
    fn lookup_is_case_and_space_insensitive() {
        assert_eq!(by_name("midnightblue").name, "Midnight Blue");
        assert_eq!(by_name("MIDNIGHT-BLUE").name, "Midnight Blue");
        assert_eq!(by_name("dark").name, "Dark");
        assert!(exists("ocean"));
        assert!(!exists("neon"));
    }

    #[test]
    fn unknown_theme_falls_back_to_light() {
        assert_eq!(by_name("neon").name, "Light");
    }

    #[test]
    fn dark_flag_matches_background() {
        for t in THEMES {
            assert_eq!(t.dark, luminance(t.background) < 0.2, "{}", t.name);
        }
    }

    #[test]
    fn text_is_readable_on_every_theme() {
        for t in THEMES {
            assert!(contrast(t.text, t.background) >= 4.5, "{}", t.name);
            assert!(contrast(t.text, t.surface) >= 4.5, "{}", t.name);
        }
    }

    #[test]
    fn contrast_of_black_and_white_is_21() {
        assert!((contrast([0, 0, 0], [255, 255, 255]) - 21.0).abs() < 0.01);
    }
}
