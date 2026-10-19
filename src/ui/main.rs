//! Kindle Scribe Studio - Native GPUI Desktop Application
//! Ultra high-performance GPU-accelerated interface for EPUB to KFX conversion and Kindle management.

use gpui::*;

mod theme;
mod state;
mod app;

use app::KindleStudio;

fn main() {
    Application::new().run(|cx: &mut App| {
        let bounds = Bounds::centered(None, size(px(1100.0), px(760.0)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                titlebar: Some(TitlebarOptions {
                    title: Some("Kindle Scribe Studio (GPUI Native)".into()),
                    ..Default::default()
                }),
                ..Default::default()
            },
            |_window, cx| cx.new(|cx| KindleStudio::new(cx)),
        )
        .unwrap();
    });
}
