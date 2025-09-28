use offdictd::{self, DefItemWrapped, offdict, stat, topk::Strprox};
use wayland::{
    self, App,
    application::{MsgQueue, WgpuLayerShellApp},
    egui,
    layer_shell::LayerShellOptions,
};

fn main() {
    tracing_subscriber::fmt::init();

    let opts = LayerShellOptions::default();

    let (sx, wayland) = WgpuLayerShellApp::new(
        opts,
        Box::new(|ctx, sx| {
            let app = HoverPanelApp {
                ui: sx,
                dict: None,
                search: vec![],
                status: SearchStatus::Initial,
                stat: None,
            };
            Ok(Box::new(app))
        }),
    );
}

struct HoverPanelApp {
    ui: MsgQueue,
    /// allows for loading rocksdb after ui is shown
    dict: Option<offdict<Strprox>>,
    search: Vec<DefItemWrapped>,
    status: SearchStatus,
    stat: Option<stat>,
}

enum SearchStatus {
    Initial,
    Searched,
}

impl App for HoverPanelApp {
    fn update(&mut self, ctx: &egui::Context) {}
}
