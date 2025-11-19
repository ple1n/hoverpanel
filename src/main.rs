#![allow(unreachable_code)]
#![allow(unused)]

use std::{
    collections::HashSet,
    env,
    os::fd::{AsFd, AsRawFd, FromRawFd, IntoRawFd, OwnedFd},
    path::PathBuf,
    thread,
    time::{Duration, Instant},
};

use arc_swap::ArcSwap;
use crossbeam::atomic::AtomicCell;
use eyeball::Observable;
use offdictd::{
    self, AsyncReadExt, DefItemWrapped, Diverge, Offdict,
    def_bin::{Def, Example, MaybeString, MaybeStructuredText, Pronunciation, Tip, WrapperDef},
    init_db, process_cmd, stat,
    tests::{collect_defs, load_fixture},
    topk::Strprox,
};
use tokio::{
    net::UnixStream,
    sync::{
        mpsc::{self, UnboundedSender},
        watch,
    },
    time::sleep,
};
use tracing::{Level, error, info, level_filters::LevelFilter, warn};
use tracing_subscriber::{Layer, filter::targets, layer::SubscriberExt, util::SubscriberInitExt};
use wayland::eframe::{
    NativeOptions,
    egui::{FontData, FontDefinitions, FontFamily, FontId, Style, TextEdit, Widget, frame},
};
use wayland::{
    self, App,
    application::{EvRx, Msg, MsgQueue, WPEvent, WgpuLayerShellApp},
    async_bincode::{self, futures::AsyncBincodeStream},
    egui::{self, Color32, Context, Margin, RichText, Ui, Vec2, Visuals, scroll_area},
    egui_chinese_font::{self, load_chinese_font},
    errors::wrap_noncritical_sync,
    layer_shell::{Anchor, KeyboardInteractivity, LayerShellOptions, WgpuLayerShellState},
    proto::{DEFAULT_SERVE_PATH, KeyCode, Kind, ProtoGesture, TapDist},
    run_layer,
};

use anyhow::{Result, anyhow};

use hoverpanel::prelude::*;

static START_AS_DEBUG: bool = false;
static RECENT_QUERY: AtomicCell<Option<Instant>> = AtomicCell::new(None);

struct GlobalConfig {
    /// The amount of time you can dither to decide whether you want the panel to show up
    dither: Duration,
}

fn main() -> Result<()> {
    let has_args = std::env::args().len() > 1;
    let app_conf_ = Arc::new(Observable::new(GlobalConfig {
        dither: Duration::from_millis(4000),
    }));

    let db_path = env::current_dir()?.join("./data");
    if has_args {
        process_cmd(|| {
            let db = init_db(db_path.clone())?;
            db.load_index(db_path)?;
            Ok(db)
        })?;
        return Ok(());
    }

    let opts = LayerShellOptions {
        width: if START_AS_DEBUG { 1000 } else { 400 },
        height: 1200,
        anchor: Some(Anchor::BOTTOM | Anchor::LEFT),
        margin: (0, 0, 0, 0),
        namespace: "hoverpanel".to_string(),
        keyboard_interactivity: Some(KeyboardInteractivity::OnDemand),
        ..Default::default()
    };

    tracing_subscriber::registry()
        .with(
            tracing_subscriber::fmt::layer()
                .without_time()
                .with_filter(LevelFilter::WARN),
        )
        .try_init()?;

    info!("{:?}", FontDefinitions::default().families);

    tracing::info!("logger set up");

    let query_rx = ArcSw::from(ArcSwap::from_pointee(vec![]));
    let dict: ArcSw<Option<Offdict<Strprox>>> = ArcSw::from(ArcSwap::from_pointee(None));
    let dict2 = dict.clone();
    let query_rx2 = query_rx.clone();
    let (wsx, mut wrx) = mpsc::unbounded_channel::<String>();
    let wsx2 = wsx.clone();
    let wsx3 = wsx.clone();

    let (sx, mut evrx, wayland) = WgpuLayerShellApp::new(
        opts,
        Box::new(move |ctx, sx, evrx| {
            let defs = load_fixture()?;
            let defs: Vec<Def> = defs.into_iter().map(|x| x.normalize_def().into()).collect();
            let wrapped = collect_defs(defs);

            let mut dict_load = Offdict::<Strprox>::open_db(db_path.clone())?;
            dict_load.load_index(db_path)?;
            dict.store(Some(dict_load).into());

            let app = HoverPanelApp {
                ui: sx,
                dict,
                search: wrapped.values().map(|x| x.to_owned()).collect(),
                status: SearchStatus::Initial,
                stat: None,
                debug_view: START_AS_DEBUG,
                query: query_rx,
                text: String::new(),
                wsx: wsx2,
                evrx,
                bg_opacity: 1.,
            };
            Ok(Box::new(app))
        }),
    );

    let msg2 = sx.clone();
    let msg3 = sx.clone();
    use futures::StreamExt;
    use wayland::async_bincode::tokio::*;

    let app_conf = app_conf_.clone();
    std::thread::spawn(move || {
        wrap_noncritical_sync(|| {
            let rt = tokio::runtime::Builder::new_multi_thread()
                .enable_io()
                .build()?;
            // the thread for word lookup
            rt.spawn(async move {
                loop {
                    if let Ok(ev) = evrx.recv_async().await {
                        match ev {
                            WPEvent::Fd(fd) => {
                                let mut rx =
                                    tokio::net::unix::pipe::Receiver::from_owned_fd(unsafe {
                                        OwnedFd::from_raw_fd(fd.into_raw_fd())
                                    })?;
                                let mut buf = vec![];
                                rx.read_to_end(&mut buf).await?;
                                let parse = String::from_utf8(buf);
                                if let Ok(stx) = parse {
                                    let len = stx.len();
                                    if len < 20 {
                                        wsx3.send(stx.clone()).unwrap();
                                        let stx: String = stx.chars().take(10).collect();
                                        warn!(len = len, "select {}", stx);
                                    }
                                }
                            }
                        }
                    }
                }

                aok(())
            });
            rt.spawn(async move {
                let mut last_string = None;
                loop {
                    if let Some(stx) = wrx.recv().await {
                        info!("query {:?}", &stx);
                        if last_string
                            .as_ref()
                            .map(|k: &String| k.as_str() == stx.as_str())
                            .unwrap_or_default()
                        {
                            continue;
                        }
                        let dict = dict2.load();
                        if let Some(ref dict) = **dict {
                            let dict: &Offdict<Strprox> = dict;
                            RECENT_QUERY.store(Some(Instant::now()));

                            let rx = dict.search(&stx, 5, false)?;
                            info!("searched {} with {} results", &stx, rx.len());
                            let mut new_rx = Vec::new();

                            for per_word in rx {
                                let mut top = SectionTop {
                                    title_l1: per_word.word,
                                    sections: vec![],
                                };
                                // L1: word
                                for (dict, de) in per_word.items {
                                    let mut sec = SectionsR::default();
                                    sec.title_l2 = Some(dict.clone());
                                    let mut ctx: LayerContext<'_> = LayerContext {
                                        top: &mut top,
                                        l2: &mut sec,
                                    };
                                    render_def(de, &mut ctx, 0);
                                    top.sections.push_dedup(sec);
                                }
                                new_rx.push_dedup(top);
                            }

                            query_rx2.store(new_rx.into());
                            msg2.send(Msg::Repaint)?;
                            last_string = Some(stx);
                        }
                    } else {
                        error!("new word recver stopped");
                        break;
                    }
                }
                anyhow::Ok(())
            });
            rt.block_on(async move {
                loop {
                    let conn = UnixStream::connect(DEFAULT_SERVE_PATH).await?;
                    let mut fm: AsyncBincodeStream<
                        tokio::net::UnixStream,
                        ProtoGesture,
                        ProtoGesture,
                        async_bincode::AsyncDestination,
                    > = AsyncBincodeStream::from(conn).for_async();
                    let mut tap_count = 0;
                    let mut used = None;
                    loop {
                        let k = fm.next().await;
                        if let Some(ges) = k {
                            let ges = ges?;
                            if ges.key == KeyCode::KEY_LEFTCTRL {
                                let recent = RECENT_QUERY.load();
                                let interim = recent.map(|k| k.elapsed());
                                let allow_show = interim.map(|k| k <= app_conf.dither);
                                let mut allow_show = allow_show.unwrap_or_default();
                                if used == recent {
                                    allow_show = false;
                                }
                                if allow_show {
                                    used = recent;
                                }
                                match ges.kind {
                                    Kind::Taps(TapDist::First(_)) => {
                                        tap_count = 0;
                                        if allow_show {
                                            msg3.send(Msg::Toggle)?;
                                        } else {
                                            msg3.send(Msg::Hide(true))?;
                                        }
                                    }
                                    Kind::Taps(TapDist::Seq(_)) => {
                                        tap_count += 1;
                                        if tap_count % 2 == 0 {
                                            if allow_show {
                                                msg3.send(Msg::Toggle)?;
                                            } else {
                                                msg3.send(Msg::Hide(true))?;
                                            }
                                        }
                                    }
                                    _ => {}
                                }
                            }

                            if ges.is_unordered(KeyCode::KEY_LEFTCTRL, KeyCode::KEY_LEFTMETA) {
                                msg3.send(Msg::Toggle)?;
                            }
                        }
                    }
                    sleep(Duration::from_secs(5)).await;
                }
                aok(())
            })?;
            aok(())
        });
    });

    wayland.run_forever()?;

    anyhow::Ok(())
}

struct HoverPanelApp {
    ui: MsgQueue,
    /// allows for loading rocksdb after ui is shown
    dict: ArcSw<Option<Offdict<Strprox>>>,
    search: Vec<DefItemWrapped>,
    status: SearchStatus,
    stat: Option<stat>,
    debug_view: bool,
    /// results from last query
    query: ArcSw<Vec<SectionTop>>,
    /// current input
    text: String,
    wsx: UnboundedSender<String>,
    evrx: EvRx,
    /// Opacity for base window
    bg_opacity: f32,
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
            self.render(ctx);
        }
    }
    fn sync(&mut self, layer: &WgpuLayerShellState) {
        if layer.current_layer == wayland::Layer::Background {
            self.bg_opacity = 0.;
        } else {
            if layer.has_blur {
                self.bg_opacity = 0.5;
            } else {
                self.bg_opacity = 1.;
            }
        }
    }
    fn init(&self, ctx: &egui::Context, layer: &WgpuLayerShellState) {
        wrap_noncritical_sync(|| {
            let mut li = Visuals::dark();
            li.override_text_color = Some(Color32::WHITE.gamma_multiply(1.));
            li.weak_text_alpha = 0.9;
            ctx.set_visuals(li);

            let mut fonts = FontDefinitions::default();

            let chinese_font_data = load_chinese_font()?;

            fonts
                .font_data
                .insert("chinese".to_owned(), chinese_font_data.into());

            let font_list = [
                "/usr/share/fonts/dejavu-sans-fonts/DejaVuSans.ttf",
                "/usr/share/fonts/TTF/DejaVuSansMNerdFont-Regular.ttf",
            ];
            let mut data: Option<Vec<u8>> = None;
            for p in font_list {
                match std::fs::read(p) {
                    Ok(d) => data = Some(d),
                    Err(_) => (),
                }
            }
            let loaded = FontData::from_owned(data.ok_or(anyhow!("cannot load a font for IPA"))?);
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
            aok(())
        });
    }
}

#[derive(Clone, PartialEq, Eq)]
struct SectionTop {
    /// word string, or source name depending on grouping
    title_l1: String,
    sections: Vec<SectionsR>,
}

#[derive(Default, Clone, PartialEq, Eq)]
struct SectionsR {
    title_l2: Option<String>,
    /// Expect IPA to always be present on L2
    ipa: Option<Pronunciation>,
    kind: Option<WordType>,
    content: Vec<SectionT>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum WordTypeID {
    Noun,
    Verb,
    Adv,
    Other,
    Adj,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct WordType {
    label: WordTypeID,
    text: String,
}

#[derive(Clone, Hash, PartialEq, Eq, Debug)]
enum SectionT {
    Etymology {
        text: MaybeStructuredText,
    },
    Example {
        text: Example,
    },
    Tip {
        text: Tip,
    },
    Explain {
        en: MaybeStructuredText,
        cn: MaybeStructuredText,
    },
    Related {
        text: MaybeStructuredText,
    },
    Info {
        text: MaybeStructuredText,
    },
}

impl HoverPanelApp {
    /// Must be generic to actual storage
    fn render_items(&self, mut render: impl FnMut(SectionTop)) {
        let read = self.query.load();
        for item in read.iter().map(|k| k.clone()).into_iter() {
            render(item)
        }
    }

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
    }

    fn render(&mut self, ctx: &Context) {
        let win = ctx.available_rect();
        let mut li = Visuals::dark();
        li.override_text_color = Some(Color32::WHITE.gamma_multiply(1.));
        li.weak_text_alpha = 0.6;
        ctx.set_visuals(li);

        if self.bg_opacity > 0.0 {
            egui::CentralPanel::default()
                .frame(
                    egui::Frame::new()
                        .inner_margin(Margin {
                            bottom: 4,
                            ..Margin::same(15)
                        })
                        .fill(
                            Color32::BLACK
                                .blend(Color32::WHITE.gamma_multiply(0.2))
                                .blend(Color32::LIGHT_YELLOW.gamma_multiply(0.1))
                                .gamma_multiply(self.bg_opacity),
                        ),
                )
                .show(ctx, |ui| {
                    let mut st = Style::default();
                    st.text_styles.insert(
                        egui::TextStyle::Body,
                        FontId {
                            size: 18.,
                            family: FontFamily::Proportional,
                        },
                    );
                    ctx.set_style(st);
                    let h = ui.available_height() - 30.;
                    ui.vertical(|ui| {
                        ui.set_height(h);
                        ui.set_width(win.width());
                        scroll_area::ScrollArea::vertical()
                            .scroll_bar_visibility(
                                scroll_area::ScrollBarVisibility::VisibleWhenNeeded,
                            )
                            .wheel_scroll_multiplier(Vec2::new(1., 15.))
                            .auto_shrink([false, false])
                            .show(ui, |ui| {
                                self.render_items(|top| {
                                    egui::frame::Frame::new()
                                        .fill(Color32::WHITE.gamma_multiply(0.08))
                                        .inner_margin(Margin::same(10))
                                        .outer_margin(Margin {
                                            left: -16,
                                            right: 0,
                                            top: 15,
                                            bottom: 0,
                                        })
                                        .show(ui, |ui| {
                                            ui.set_width(win.width());
                                            ui.label(
                                                RichText::new(top.title_l1).color(Color32::WHITE),
                                            );
                                            ui.spacing();

                                            for sec2 in top.sections {
                                                let mut color = None;
                                                if let Some(word_kind) = sec2.kind {
                                                    color = Some(
                                                        match word_kind.label {
                                                            WordTypeID::Noun => Color32::ORANGE,
                                                            WordTypeID::Verb => Color32::LIGHT_BLUE,
                                                            WordTypeID::Adv => Color32::LIGHT_GREEN,
                                                            WordTypeID::Other => Color32::WHITE,
                                                            WordTypeID::Adj => Color32::YELLOW,
                                                        }
                                                        .blend(Color32::GRAY.gamma_multiply(0.2))
                                                        .blend(Color32::WHITE.gamma_multiply(0.5)),
                                                    );
                                                    ui.label(
                                                        RichText::new(word_kind.text)
                                                            .color(color.unwrap()),
                                                    );
                                                }
                                                if let Some(ipa) = sec2.ipa {
                                                    ui.horizontal(|ui| {
                                                        for tn in ipa.into_iter() {
                                                            ui.label(
                                                                RichText::new(tn).background_color(
                                                                    Color32::LIGHT_BLUE
                                                                        .gamma_multiply(0.35),
                                                                ),
                                                            );
                                                        }
                                                    });
                                                }
                                                for sec3 in sec2.content {
                                                    self.show_section_t(sec3, ui);
                                                }
                                            }
                                        });
                                });
                            });
                        ui.add_space(4.);
                        ui.horizontal(|ui| {
                            let text = TextEdit::singleline(&mut self.text)
                                .background_color(Color32::BLACK.gamma_multiply(0.2))
                                .vertical_align(egui::Align::Center)
                                .desired_width(220.)
                                .ui(ui);
                            if text.changed() {
                                info!("input = {}", self.text);
                                let _ = self.wsx.send(self.text.clone());
                            }
                            if true {
                                if ui.button("exit").clicked() {
                                    self.ui.send(Msg::Exit).unwrap();
                                    ctx.request_repaint();
                                }
                                if ui.button("hide").clicked() {
                                    self.ui.send(Msg::Hide(true)).unwrap();
                                    ctx.request_repaint();
                                }
                            }
                        });
                    })
                });
        }
    }

    fn show_section_t(&self, sec3: SectionT, ui: &mut Ui) {
        egui::frame::Frame::new()
            .inner_margin(Margin {
                left: 15,
                bottom: 5,
                right: 15,
                ..Default::default()
            })
            .outer_margin(Margin {
                left: 0,
                right: 0,
                top: 0,
                bottom: 0,
            })
            .show(ui, |ui| match sec3 {
                SectionT::Explain { en, cn } => {
                    for tn in en.into_iter() {
                        ui.label(RichText::new(tn));
                    }
                    for tn in cn.into_iter() {
                        ui.label(RichText::new(tn));
                    }
                }
                SectionT::Example { text } => {
                    for tn in text.into_iter() {
                        match tn {
                            MaybeString::Str(tn) => {
                                ui.label(
                                    RichText::new(tn).color(Color32::WHITE.gamma_multiply(0.8)),
                                );
                            }
                            MaybeString::Obj(ex) => {
                                if let Some(tn) = ex.EN {
                                    ui.label(
                                        RichText::new(tn).color(Color32::WHITE.gamma_multiply(0.8)),
                                    );
                                }
                                if let Some(tn) = ex.CN {
                                    ui.label(
                                        RichText::new(tn).color(Color32::WHITE.gamma_multiply(0.8)),
                                    );
                                }
                            }
                        }
                    }
                }
                SectionT::Etymology { text } => {
                    for tn in text.into_iter() {
                        ui.label(tn);
                    }
                }
                SectionT::Tip { text } => {
                    for tn in text.into_iter() {
                        match tn {
                            MaybeString::Str(tn) => {
                                ui.label(tn);
                            }
                            MaybeString::Obj(ex) => {
                                if let Some(tn) = ex.EN {
                                    ui.label(RichText::new(tn));
                                }
                                if let Some(tn) = ex.CN {
                                    ui.label(tn);
                                }
                            }
                        }
                    }
                }
                _ => {}
            });
    }

    fn full_view(&self, ctx: &Context) {
        let win = ctx.available_rect();
        egui::CentralPanel::default()
            .frame(
                egui::Frame::new().inner_margin(Margin::same(15)).fill(
                    Color32::BLACK
                        .blend(Color32::WHITE.gamma_multiply(0.25))
                        .gamma_multiply(0.65),
                ),
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
                ui.label(
                    RichText::new(number_t).background_color(Color32::ORANGE.gamma_multiply(0.2)),
                );

                for ex in de.examples.iter().flatten() {
                    for ex in ex.clone().into_iter() {
                        match ex {
                            MaybeString::Str(st) => {
                                ui.label(RichText::new(st).underline());
                            }
                            MaybeString::Obj(de) => {
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
                        MaybeString::Str(st) => {
                            ui.label(RichText::new(st));
                        }
                        MaybeString::Obj(de) => {
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

struct LayerContext<'k> {
    top: &'k mut SectionTop,
    l2: &'k mut SectionsR,
}

trait VecExt<T> {
    fn push_dedup(&mut self, last: T);
}

impl<T: Eq> VecExt<T> for Vec<T> {
    fn push_dedup(&mut self, last: T) {
        if self.last() != Some(&last) {
            self.push(last);
        }
    }
}

fn render_def(de: Def, ctx: &mut LayerContext, depth: u32) {
    if let Some(pn) = de.pronunciation {
        info!("pronunciation {:?} {:?}", &de.word, &ctx.l2.kind);
        ctx.l2.ipa = Some(pn);
    };
    if let Some(cn) = de.etymology {
        let et = SectionT::Etymology {
            text: MaybeStructuredText::Vec(cn.into_iter().map(Option::Some).collect()),
        };
        ctx.l2.content.push_dedup(et);
    }
    if let Some(inf) = de.info {
        ctx.l2.content.push_dedup(SectionT::Info {
            text: MaybeStructuredText::Str(inf),
        });
    }
    if de.CN.is_some() || de.EN.is_some() {
        ctx.l2.content.push_dedup(SectionT::Explain {
            en: de.EN.into(),
            cn: de.CN.into(),
        });
    }
    if let Some(exs) = de.examples {
        for ex in exs {
            ctx.l2.content.push_dedup(SectionT::Example { text: ex });
        }
    }
    if let Some(ty) = de.r#type {
        let lower = ty.to_lowercase();
        let mut label = WordTypeID::Other;
        if lower.contains("verb")
            || lower.contains("动词")
            || lower.contains("v.")
            || lower.contains("vt.")
            || lower.contains("vi.")
        {
            label = WordTypeID::Verb;
        }
        if lower.contains("noun") || lower.contains("n.") || lower.contains("名词") {
            label = WordTypeID::Noun;
        }
        if lower.contains("adj") || lower.contains("adj.") || lower.contains("形容词") {
            label = WordTypeID::Adj;
        }
        if lower.contains("adv") || lower.contains("副词") {
            label = WordTypeID::Adv;
        }
        ctx.l2.kind = Some(WordType { label, text: lower });
    }
    for de in de.definitions.into_iter().flatten() {
        render_def(de, ctx, depth + 1);
    }
}
