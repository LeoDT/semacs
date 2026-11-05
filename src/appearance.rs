use std::{borrow::Cow, fmt};

use gpui_kit::{Font, Global, Pixels, SharedString, font, px};

const DEFAULT_FONT_FAMILY: &str = "Lilex";
const BUNDLED_FONTS: [&[u8]; 4] = [
    include_bytes!("../assets/fonts/lilex/Lilex-Regular.ttf"),
    include_bytes!("../assets/fonts/lilex/Lilex-Bold.ttf"),
    include_bytes!("../assets/fonts/lilex/Lilex-Italic.ttf"),
    include_bytes!("../assets/fonts/lilex/Lilex-BoldItalic.ttf"),
];
const MIN_FONT_SIZE: Pixels = px(8.);
const MAX_FONT_SIZE: Pixels = px(72.);
const LINE_HEIGHT_RATIO: f32 = 1.5;

pub(crate) fn load_bundled_fonts(system: &gpui_kit::TextSystem) -> gpui_kit::Result<()> {
    system.add_fonts(BUNDLED_FONTS.into_iter().map(Cow::Borrowed).collect())
}

#[derive(Clone, Debug, PartialEq)]
pub struct FontSettings {
    family: SharedString,
    base_size: Pixels,
    size: Pixels,
}

impl Global for FontSettings {}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FontSettingsError {
    InvalidFamily,
    InvalidSize,
}

impl fmt::Display for FontSettingsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidFamily => {
                f.write_str("Font family must be nonempty and contain no control characters")
            }
            Self::InvalidSize => f.write_str("Font size must be between 8 and 72 logical pixels"),
        }
    }
}

impl std::error::Error for FontSettingsError {}

impl Default for FontSettings {
    fn default() -> Self {
        Self::new(DEFAULT_FONT_FAMILY, px(16.)).expect("valid default font")
    }
}

impl FontSettings {
    pub fn new(family: &str, base_size: Pixels) -> Result<Self, FontSettingsError> {
        let mut settings = Self {
            family: SharedString::default(),
            base_size,
            size: base_size,
        };
        settings.set_family(family)?;
        settings.set_size(base_size)?;
        Ok(settings)
    }

    pub fn family(&self) -> &str {
        &self.family
    }

    pub fn font(&self) -> Font {
        font(self.family.clone())
    }

    pub fn size(&self) -> Pixels {
        self.size
    }

    pub fn base_size(&self) -> Pixels {
        self.base_size
    }

    pub fn line_height(&self) -> Pixels {
        self.size * LINE_HEIGHT_RATIO
    }

    pub fn set_family(&mut self, family: &str) -> Result<(), FontSettingsError> {
        if family.chars().any(char::is_control) || family.trim().is_empty() {
            return Err(FontSettingsError::InvalidFamily);
        }
        self.family = family.trim().to_owned().into();
        Ok(())
    }

    pub fn set_size(&mut self, size: Pixels) -> Result<(), FontSettingsError> {
        if !f32::from(size).is_finite() || !(MIN_FONT_SIZE..=MAX_FONT_SIZE).contains(&size) {
            return Err(FontSettingsError::InvalidSize);
        }
        self.size = size;
        Ok(())
    }

    pub fn reset_size(&mut self) {
        self.size = self.base_size;
    }
}

#[cfg(all(test, target_os = "linux"))]
mod font_tests;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invalid_settings_leave_the_previous_value_intact() {
        let mut settings = FontSettings::default();
        let previous = settings.clone();
        for family in ["", "  ", "font\nname", "font\0name"] {
            assert_eq!(
                settings.set_family(family),
                Err(FontSettingsError::InvalidFamily)
            );
            assert_eq!(settings, previous);
        }
        for size in [0., 7., 73., f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            assert_eq!(
                settings.set_size(px(size)),
                Err(FontSettingsError::InvalidSize)
            );
            assert_eq!(settings, previous);
            assert!(FontSettings::new("monospace", px(size)).is_err());
        }
        assert!(FontSettings::new(" ", px(16.)).is_err());
    }

    #[test]
    fn resetting_size_preserves_the_family_and_configured_baseline() {
        let mut settings = FontSettings::new(" Initial Font ", px(18.)).unwrap();
        assert_eq!(settings.family(), "Initial Font");
        settings.set_family("Other Font").unwrap();
        settings.set_size(MAX_FONT_SIZE).unwrap();
        assert_eq!(settings.line_height(), px(108.));
        settings.set_size(MIN_FONT_SIZE).unwrap();
        settings.reset_size();
        assert_eq!(settings.family(), "Other Font");
        assert_eq!(settings.size(), px(18.));
        assert_eq!(settings.base_size(), px(18.));
        assert_eq!(settings.line_height(), px(27.));
    }
}
