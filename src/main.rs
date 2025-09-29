use std::{thread, time::Duration};

use eframe::{
    NativeOptions,
    egui::{FontData, FontDefinitions, FontFamily, FontId},
};
use egui_tracing::{EventCollector, Glob, tracing::collector::AllowedTargets};
use hoverpanel::console::{console_over_ev, thread_console};
use offdictd::{
    self, DefItemWrapped,
    def_bin::{Def, MaybeString, WrapperDef},
    offdict, stat,
    tests::{collect_defs, load_fixture},
    topk::Strprox,
};
use tokio::sync::watch;
use tracing::{Level, info, level_filters::LevelFilter};
use tracing_subscriber::{filter::targets, layer::SubscriberExt, util::SubscriberInitExt};
use wayland::{
    self, App,
    application::{Msg, MsgQueue, WgpuLayerShellApp},
    egui::{self, Color32, Context, Margin, RichText, Ui, Vec2, Visuals, scroll_area},
    egui_chinese_font::{self, load_chinese_font},
    errors::wrap_noncritical_sync,
    layer_shell::{Anchor, KeyboardInteractivity, Layer, LayerShellOptions},
    run_layer,
    wayland_clipboard_listener::{self, WlListenType},
};

use anyhow::Result;

static START_AS_DEBUG: bool = false;

fn main() -> Result<()> {
    let opts = LayerShellOptions {
        width: if START_AS_DEBUG { 1000 } else { 400 },
        height: 800,
        anchor: Some(Anchor::LEFT),
        margin: (50, 50, 50, 50),
        keyboard_interactivity: Some(KeyboardInteractivity::OnDemand),
        ..Default::default()
    };

    let (p_sx, p_rx) = watch::channel("selection".to_owned());

    let mut targets: Vec<String> = ["naga"].iter().map(|k| (*k).to_owned()).collect();
    targets.clear();

    let mut ev = if targets.len() > 0 {
        EventCollector::new()
            .with_level(Level::DEBUG)
            .allowed_targets(AllowedTargets::Selected(targets))
    } else {
        EventCollector::new()
            .with_level(Level::DEBUG)
            .allowed_targets(AllowedTargets::All)
    };

    let globs = ["naga*", "egui*", "glob*", "sctk*", "wgpu*"];
    for glob in globs {
        ev.excluded.push(Glob::new(glob)?);
    }

    tracing_subscriber::registry().with(ev.clone()).try_init()?;

    let separate_window_console = true;
    if separate_window_console {
        let ev = ev.clone();
        thread::spawn(move || {
            thread_console(ev);
        });
    }

    info!("{:?}", FontDefinitions::default().families);

    tracing::info!("logger set up");

    let (sx, mut wayland) = WgpuLayerShellApp::new(
        opts,
        Box::new(move |ctx, sx| {
            let mut li = Visuals::dark();
            li.override_text_color = Some(Color32::WHITE.gamma_multiply(0.8));
            ctx.set_visuals(li);

            let mut fonts = FontDefinitions::default();

            let chinese_font_data = load_chinese_font()?;

            fonts
                .font_data
                .insert("chinese".to_owned(), chinese_font_data.into());

            let data = std::fs::read("/usr/share/fonts/TTF/DejaVuSansMNerdFont-Regular.ttf")?;
            let loaded = FontData::from_owned(data);

            fonts.font_data.insert("ipa".to_owned(), loaded.into());
            fonts
                .families
                .entry(FontFamily::Proportional)
                .or_default()
                .insert(0, "chinese".to_owned());
            fonts
                .families
                .entry(FontFamily::Monospace)
                .or_default()
                .insert(0, "chinese".to_owned());
            fonts
                .families
                .entry(FontFamily::Monospace)
                .or_default()
                .insert(0, "ipa".to_owned());
            fonts
                .families
                .entry(FontFamily::Proportional)
                .or_default()
                .insert(0, "ipa".to_owned());

            // I have experimented to conclude that, Dejavu doesnt support CJK
            // Its a fallback mechanism that makes both work.

            info!("{:?}", &fonts.families);

            ctx.set_fonts(fonts);

            let defs = load_fixture()?;
            let defs: Vec<Def> = defs.into_iter().map(|x| x.normalize_def().into()).collect();
            let wrapped = collect_defs(defs);

            let app = HoverPanelApp {
                ui: sx,
                dict: None,
                search: wrapped.values().map(|x| x.to_owned()).collect(),
                status: SearchStatus::Initial,
                stat: None,
                eview: Some(ev),
                debug_view: START_AS_DEBUG,
            };
            Ok(Box::new(app))
        }),
    );

    let msg2 = sx.clone();
    std::thread::spawn(move || {
        wrap_noncritical_sync(|| {
            let mut lis = wayland_clipboard_listener::WlClipboardPasteStream::init(
                WlListenType::ListenOnSelect,
            )?;
            for ctx in lis.paste_stream().flatten() {
                let stx = String::from_utf8(ctx.context.context);
                if let Ok(stx) = stx {
                    info!("select {:?}", &stx);
                    p_sx.send(stx)?;
                    msg2.send(Msg::Repaint)?;
                }
            }
            anyhow::Ok(())
        });
    });

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
    eview: Option<EventCollector>,
    debug_view: bool,
}

enum SearchStatus {
    Initial,
    Searched,
}

impl App for HoverPanelApp {
    fn update(&mut self, ctx: &egui::Context) {
        if self.debug_view {
            self.debug_view(ctx);
        } else {
            self.full_view(ctx);
        }
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
                        .wheel_scroll_multiplier(Vec2::new(1., 15.))
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            for per_word in &self.search {
                                ui.label(&per_word.word);

                                for (dict, de) in &per_word.items {
                                    for de in de.definitions.iter().flatten() {
                                        display_debug(de, ui, 0);
                                    }
                                }
                            }
                        });
                    ui.horizontal(|ui| {
                        if ui.button("exit").clicked() {
                            self.ui.send(Msg::Hide(true)).unwrap();
                            ctx.request_repaint();

                            self.ui.send(Msg::Exit).unwrap();
                            ctx.request_repaint();
                        }
                        if ui.button("hide").clicked() {
                            self.ui.send(Msg::Hide(true)).unwrap();
                            ctx.request_repaint();
                        }
                    });
                })
            });

        if self.debug_view
            && let Some(ev) = &self.eview
        {
            egui::TopBottomPanel::new(egui::panel::TopBottomSide::Bottom, "console")
                .resizable(true)
                .default_height(400.)
                .show(ctx, |ui| {
                    ui.add(egui_tracing::Logs::new(ev.clone()));
                });
        }
    }

    fn full_view(&self, ctx: &Context) {
        let win = ctx.available_rect();

        egui::CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .inner_margin(Margin::same(15))
                    .fill(Color32::BLACK.gamma_multiply(0.5)),
            )
            .show(ctx, |ui| {
                let h = ui.available_height() - 30.;
                ui.vertical(|ui| {
                    ui.set_height(h);
                    ui.set_width(win.width());
                    scroll_area::ScrollArea::vertical()
                        .scroll_bar_visibility(scroll_area::ScrollBarVisibility::VisibleWhenNeeded)
                        .wheel_scroll_multiplier(Vec2::new(1., 15.))
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            for per_word in &self.search {
                                egui::frame::Frame::new()
                                    .fill(Color32::WHITE.gamma_multiply(0.2))
                                    .inner_margin(Margin::same(4))
                                    .outer_margin(Margin {
                                        left: -16,
                                        right: 0,
                                        top: 15,
                                        bottom: 0,
                                    })
                                    .show(ui, |ui| {
                                        ui.set_width(win.width());
                                        ui.label(
                                            RichText::new(&per_word.word).color(Color32::WHITE),
                                        );
                                        ui.spacing();
                                    });

                                for (dict, de) in &per_word.items {
                                    let dict_to_word = de.word.to_owned();
                                    for (p, de) in de.definitions.iter().flatten().enumerate() {
                                        display(
                                            de,
                                            ui,
                                            Inherited {
                                                word_title: dict_to_word.clone(),
                                                place: p as u32,
                                                depth: 0,
                                            },
                                        );
                                    }
                                }
                            }
                        });
                    ui.horizontal(|ui| {
                        if ui.button("exit").clicked() {
                            self.ui.send(Msg::Exit).unwrap();
                            ctx.request_repaint();
                        }
                        if ui.button("hide").clicked() {
                            self.ui.send(Msg::Hide(true)).unwrap();
                            ctx.request_repaint();
                        }
                    });
                })
            });

        if self.debug_view
            && let Some(ev) = &self.eview
        {
            egui::TopBottomPanel::new(egui::panel::TopBottomSide::Bottom, "console")
                .resizable(true)
                .default_height(400.)
                .show(ctx, |ui| {
                    ui.add(egui_tracing::Logs::new(ev.clone()));
                });
        }
    }
}

enum HighlightType {
    Noun,
    Verb,
    None,
}

struct Inherited {
    word_title: Option<String>,
    place: u32,
    depth: u32,
}

fn display(de: &Def, ui: &mut Ui, inherit: Inherited) {
    egui::containers::Frame::new()
        .inner_margin(Margin {
            left: inherit.depth as i8 * 5,
            ..Default::default()
        })
        .show(ui, |ui| {
            let mut title_highlight = HighlightType::None;
            let type_string = if let Some(cn) = &de.r#type {
                let cn = cn.to_lowercase();
                if cn.contains("verb") || cn.contains("动词") {
                    title_highlight = HighlightType::Verb;
                }
                if cn.contains("noun") || cn.contains("名词") {
                    title_highlight = HighlightType::Noun;
                }

                Some(cn)
            } else {
                None
            };

            let title = inherit.word_title.or(de.title.clone()).or(de.word.clone());
            if inherit.place == 0 {
                if let Some(cn) = type_string {
                    let title = RichText::new(cn.to_owned());
                    let title = title.color(
                        match title_highlight {
                            HighlightType::Noun => Color32::ORANGE,
                            HighlightType::Verb => Color32::CYAN,
                            HighlightType::None => Color32::GREEN,
                        }
                        .blend(Color32::GRAY.gamma_multiply(0.5)),
                    );
                    ui.label(title);
                }
            } else {
                if let Some(cn) = &title {
                    let title = RichText::new(cn.to_owned());
                    let title = title.color(
                        match title_highlight {
                            HighlightType::Noun => Color32::ORANGE,
                            HighlightType::Verb => Color32::CYAN,
                            HighlightType::None => Color32::GREEN,
                        }
                        .blend(Color32::GRAY.gamma_multiply(0.5)),
                    );
                    ui.label(title);
                }
            }

            if let Some(cn) = &de.CN {
                ui.label(RichText::new(cn.to_owned()));
            }
            if let Some(cn) = &de.EN {
                ui.label(RichText::new(cn.to_owned()));
            }
            if let Some(cn) = &de.info {
                ui.label(RichText::new(cn.to_owned()));
            }

            let number_t = format!(
                "{}{}",
                de.t1.clone().unwrap_or_default(),
                de.t2.clone().unwrap_or_default()
            );
            let number_t = number_t.trim();
            
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new(number_t));

                for ex in de.examples.iter().flatten() {
                    for ex in ex.clone().into_iter() {
                        match ex {
                            MaybeString::str(st) => {
                                ui.label(RichText::new(st));
                            }
                            MaybeString::obj(de) => {
                                if let Some(cn) = &de.CN {
                                    ui.label(RichText::new(cn.to_owned()));
                                }
                                if let Some(cn) = &de.EN {
                                    ui.label(RichText::new(cn.to_owned()));
                                }
                            }
                        }
                    }
                }
            });

            if let Some(cn) = &de.pronunciation {
                for cn in cn.clone().into_iter() {
                    ui.label(RichText::new(cn).font(FontId {
                        size: 12.,
                        family: FontFamily::Monospace,
                    }));
                }
            }
            if let Some(cn) = &de.etymology {
                for et in cn {
                    ui.label(RichText::new(et.to_owned()));
                }
            }

            for de in de.definitions.iter().flatten() {
                display(
                    de,
                    ui,
                    Inherited {
                        word_title: title.clone(),
                        depth: inherit.depth + 1,
                        ..inherit
                    },
                );
            }
        });
}

fn display_debug(de: &Def, ui: &mut Ui, depth: u32) {
    egui::containers::Frame::new()
        .inner_margin(Margin {
            left: depth as i8 * 5,
            ..Default::default()
        })
        .fill(Color32::WHITE.gamma_multiply(0.15))
        .show(ui, |ui| {
            ui.label(
                RichText::new(format!("d={}", depth)).color(Color32::WHITE.gamma_multiply(0.4)),
            );
            if let Some(cn) = &de.title {
                ui.label(RichText::new(cn.to_owned()));
            }
            if let Some(cn) = &de.r#type {
                ui.label(RichText::new(cn.to_owned()));
            }
            if let Some(cn) = &de.CN {
                ui.label(RichText::new(cn.to_owned()));
            }
            if let Some(cn) = &de.EN {
                ui.label(RichText::new(cn.to_owned()));
            }
            if let Some(cn) = &de.info {
                ui.label(RichText::new(cn.to_owned()));
            }
            if let Some(cn) = &de.t1 {
                ui.label(RichText::new(cn.to_owned()));
            }
            if let Some(cn) = &de.t2 {
                ui.label(RichText::new(cn.to_owned()));
            }

            for ex in de.examples.iter().flatten() {
                for ex in ex.clone().into_iter() {
                    match ex {
                        MaybeString::str(st) => {
                            ui.label(RichText::new(st));
                        }
                        MaybeString::obj(de) => {
                            if let Some(cn) = &de.CN {
                                ui.label(RichText::new(cn.to_owned()));
                            }
                            if let Some(cn) = &de.EN {
                                ui.label(RichText::new(cn.to_owned()));
                            }
                        }
                    }
                }
            }

            if let Some(cn) = &de.pronunciation {
                for cn in cn.clone().into_iter() {
                    ui.label(RichText::new(cn).font(FontId {
                        size: 12.,
                        family: FontFamily::Monospace,
                    }));
                }
            }
            if let Some(cn) = &de.etymology {
                for et in cn {
                    ui.label(RichText::new(et.to_owned()));
                }
            }

            for de in de.definitions.iter().flatten() {
                display_debug(de, ui, depth + 1);
            }
        });
}
