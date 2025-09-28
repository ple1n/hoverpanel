use offdictd::{
    self, DefItemWrapped,
    def_bin::{Def, WrapperDef},
    offdict, stat,
    tests::{collect_defs, load_fixture},
    topk::Strprox,
};
use wayland::{
    self, App,
    application::{MsgQueue, WgpuLayerShellApp},
    egui::{self, Color32, Context, Margin, RichText, Ui, Visuals, scroll_area},
    egui_chinese_font,
    layer_shell::{Anchor, KeyboardInteractivity, Layer, LayerShellOptions},
    run_layer,
};

use anyhow::Result;

fn main() -> Result<()> {
    tracing_subscriber::fmt::init();

    let opts = LayerShellOptions {
        width: 400,
        height: 800,
        anchor: Some(Anchor::LEFT),
        margin: (50, 50, 50, 50),
        keyboard_interactivity: Some(KeyboardInteractivity::OnDemand),
        ..Default::default()
    };

    let (sx, mut wayland) = WgpuLayerShellApp::new(
        opts,
        Box::new(|ctx, sx| {
            let mut li = Visuals::dark();
            li.override_text_color = Some(Color32::WHITE.gamma_multiply(0.8));
            ctx.set_visuals(li);
            egui_chinese_font::setup_chinese_fonts(ctx).unwrap();

            let defs = load_fixture()?;
            let defs: Vec<Def> = defs.into_iter().map(|x| x.normalize_def().into()).collect();
            let wrapped = collect_defs(defs);

            let app = HoverPanelApp {
                ui: sx,
                dict: None,
                search: wrapped.values().map(|x| x.to_owned()).collect(),
                status: SearchStatus::Initial,
                stat: None,
            };
            Ok(Box::new(app))
        }),
    );

    wayland.run()?;
    anyhow::Ok(())
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
    fn update(&mut self, ctx: &egui::Context) {
        self.debug_view(ctx);
    }
}

impl HoverPanelApp {
    fn debug_view(&self, ctx: &Context) {
        let win = ctx.available_rect();

        egui::CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .fill(Color32::WHITE.gamma_multiply(0.05))
                    .inner_margin(Margin::same(15)),
            )
            .show(ctx, |ui| {
                let h = ui.available_height() - 30.;
                ui.vertical(|ui| {
                    ui.set_height(h);
                    ui.set_width(win.width());
                    scroll_area::ScrollArea::vertical()
                        .scroll_bar_visibility(scroll_area::ScrollBarVisibility::VisibleWhenNeeded)
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            for per_word in &self.search {
                                ui.label(&per_word.word);

                                for (dict, de) in &per_word.items {
                                    for de in de.definitions.iter().flatten() {
                                        display(de, ui, 0);
                                    }
                                }
                            }
                        });
                    ui.button("exit");
                })
            });
    }
}

fn display(de: &Def, ui: &mut Ui, depth: u32) {
    if let Some(cn) = &de.CN {
        egui::containers::Frame::new()
            .inner_margin(Margin {
                left: depth as i8 * 5,
                ..Default::default()
            })
            .show(ui, |ui| {
                ui.label(
                    RichText::new(format!("d={}", depth)).color(Color32::WHITE.gamma_multiply(0.4)),
                );
                ui.label(RichText::new(cn.to_owned()));
            });
    }
    for de in de.definitions.iter().flatten() {
        display(de, ui, depth + 1);
    }
}
