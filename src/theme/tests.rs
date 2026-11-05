use super::*;

#[test]
fn switching_rebuilds_from_base_and_keeps_only_explicit_overrides() {
    let mut settings = ThemeSettings::default();
    let original = settings.style(StyleRole::Default);
    settings
        .define(
            "custom",
            "dark",
            vec![(
                StyleRole::Default,
                StyleOverride::new(StyleRole::Default, Some("#123456"), None).unwrap(),
            )],
        )
        .unwrap();
    settings.set_style(
        StyleRole::Cursor,
        StyleOverride::new(StyleRole::Cursor, None, Some("#fFaA00")).unwrap(),
    );
    settings.select("custom").unwrap();
    assert_eq!(
        settings.style(StyleRole::Default).foreground,
        parse_color("#123456").unwrap()
    );
    let cursor = settings.style(StyleRole::Cursor).background;
    settings.select("light").unwrap();
    assert_eq!(settings.style(StyleRole::Default), original);
    assert_eq!(settings.style(StyleRole::Cursor).background, cursor);
    let before = settings.clone();
    assert!(settings.select("missing").is_err());
    assert_eq!(settings, before);
}

#[test]
fn validates_names_colors_attributes_and_limits() {
    for value in ["red", "#fff", "#000000ff", "#gggggg", " #123456", "#１２３"] {
        assert!(parse_color(value).is_err(), "{value}");
    }
    for role in StyleRole::ALL {
        assert_eq!(StyleRole::parse(role.name()).unwrap(), role);
    }
    assert!(StyleRole::parse("font-lock-keyword-face").is_err());
    assert!(StyleOverride::new(StyleRole::Region, Some("#ffffff"), None).is_err());
    assert!(StyleOverride::new(StyleRole::Separator, None, Some("#ffffff")).is_err());
    let mut settings = ThemeSettings::default();
    for name in ["", "light", "dark", "BAD", "has space", "中文"] {
        assert!(settings.define(name, "dark", vec![]).is_err());
    }
    assert!(settings.define("custom", "automatic", vec![]).is_err());
    for index in 0..32 {
        settings
            .define(&format!("theme-{index}"), "dark", vec![])
            .unwrap();
    }
    assert!(settings.define("extra", "dark", vec![]).is_err());
}

#[test]
fn style_overrides_merge_by_attribute_and_fresh_settings_reset_them() {
    let mut settings = ThemeSettings::default();
    for (fg, bg) in [(Some("#112233"), None), (None, Some("#445566"))] {
        settings.set_style(
            StyleRole::Default,
            StyleOverride::new(StyleRole::Default, fg, bg).unwrap(),
        );
    }
    assert_eq!(
        settings.style(StyleRole::Default),
        StyleColors {
            foreground: parse_color("#112233").unwrap(),
            background: parse_color("#445566").unwrap(),
        }
    );
    assert_ne!(settings, ThemeSettings::default());
}

#[gpui_kit::test]
fn installation_projects_colors_to_gpui(cx: &mut gpui_kit::TestAppContext) {
    cx.update(crate::app::init);
    cx.update(|cx| {
        let mut settings = ThemeSettings::default();
        settings.select("dark").unwrap();
        settings.set_style(
            StyleRole::Default,
            StyleOverride::new(StyleRole::Default, Some("#123456"), None).unwrap(),
        );
        settings.install(cx);
        let theme = Theme::global(cx);
        assert_eq!(theme.mode, ThemeMode::Dark);
        assert_eq!(theme.colors.foreground, parse_color("#123456").unwrap());
        assert_eq!(
            theme.colors.caret,
            ThemeSettings::global(cx)
                .style(StyleRole::Cursor)
                .background
        );
    });
}
