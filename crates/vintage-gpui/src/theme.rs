//! Shared appearance and text scale for native chrome.
use gpui::{App, Global, Rgba, Window, WindowAppearance};
use vintage_runtime::settings::{Appearance, Settings};
#[derive(Default)]
pub struct Preferences(pub Settings);
impl Global for Preferences {}
#[derive(Clone, Copy)]
pub struct Palette {
    light: bool,
    graphite: bool,
    stacked: bool,
}
impl Palette {
    pub fn new(window: &Window, cx: &App) -> Self {
        let appearance = cx.global::<Preferences>().0.appearance;
        let light = match appearance {
            Appearance::Light => true,
            Appearance::Dark | Appearance::Graphite => false,
            Appearance::System => matches!(
                window.appearance(),
                WindowAppearance::Light | WindowAppearance::VibrantLight
            ),
        };
        Self {
            light,
            graphite: appearance == Appearance::Graphite,
            stacked: window.viewport_size().width
                / gpui::px(cx.global::<Preferences>().0.ui_percent as f32 / 100.)
                < 1000.,
        }
    }
    pub fn stacked(self) -> bool {
        self.stacked
    }
    pub fn color(self, dark: u32) -> Rgba {
        gpui::rgb(if self.light {
            match dark {
                0x191816 => 0xfaf8f4,
                0x151411 => 0xeee8de,
                0x777165 => 0x756a5b,
                0x201f1c => 0xf0ece5,
                0x24231f => 0xeae5dc,
                0x2b2923 | 0x2a2824 => 0xeee8de,
                0x343027 | 0x393328 => 0xe6d9c2,
                0x39352e | 0x302d27 => 0xd8d0c3,
                0x544936 | 0x8c754e => 0xa38b61,
                0xe6e1d8 => 0x302b23,
                0xbab1a1 => 0x655b4d,
                0x9b958a => 0x756a5b,
                0xc6a66b => 0x89682e,
                0xe8aa82 => 0x984a27,
                _ => dark,
            }
        } else if self.graphite {
            match dark {
                0x191816 => 0x181818,
                0x151411 => 0x141414,
                0x201f1c => 0x202020,
                0x24231f => 0x242424,
                0x2b2923 | 0x2a2824 => 0x282828,
                0x343027 | 0x393328 => 0x303030,
                0x39352e | 0x302d27 => 0x383838,
                0x544936 | 0x8c754e => 0x4a4a4a,
                0xe6e1d8 => 0xe4e4e4,
                0xbab1a1 => 0xb0b0b0,
                0x9b958a | 0x777165 => 0x858585,
                0xc6a66b => 0xd79a54,
                0xe8aa82 => 0xe28b74,
                _ => dark,
            }
        } else {
            dark
        })
    }
}
