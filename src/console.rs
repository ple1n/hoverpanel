use std::time::Instant;

use wayland::eframe::{
    self, NativeOptions, UserEvent, egui::{self, ViewportBuilder, ViewportId}
};
use egui_tracing::EventCollector;
use tracing::warn;
use wayland::flume::Receiver;
use winit::{
    event_loop::EventLoop,
    platform::{run_on_demand::EventLoopExtRunOnDemand, wayland::EventLoopBuilderExtWayland},
    raw_window_handle::HasWindowHandle,
};

/// Commands sent to the console window.
#[derive(Debug, Clone)]
pub enum ConsoleCmd {
    Show(Instant),
}

pub fn thread_console(ev: EventCollector, rx: Receiver<ConsoleCmd>) {
    let mut last_close = Instant::now();
    let mut evloop = EventLoop::<_>::with_user_event()
        .with_any_thread(true)
        .build()
        .unwrap();

    let rx = rx.clone();
    let ev = ev.clone();

    let mut opts = NativeOptions::default();
    evloop.set_control_flow(winit::event_loop::ControlFlow::Poll);
    let mut window = eframe::create_native(
        "console",
        opts,
        Box::new(move |_ctx| {
            Result::Ok(Box::new(ConsoleApp {
                ev,
                rx,
                visible: true,
            }))
        }),
        &evloop,
    );
    evloop.run_app_on_demand(&mut window).unwrap();

    warn!("console thread exit");
}

pub fn console_over_ev(ev: &EventLoop<UserEvent>) {}

pub struct ConsoleApp {
    ev: EventCollector,
    rx: Receiver<ConsoleCmd>,
    visible: bool,
}

impl eframe::App for ConsoleApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if ctx.input(|x| x.viewport().close_requested()) {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            ctx.send_viewport_cmd_to(ctx.viewport_id(), egui::ViewportCommand::Visible(false));
            ctx.send_viewport_cmd_to(ViewportId::ROOT, egui::ViewportCommand::Visible(false));
            ctx.request_repaint();
        }

              ctx.send_viewport_cmd_to(ViewportId::ROOT, egui::ViewportCommand::Visible(false));

        egui::CentralPanel::default()
            .show(ctx, |ui| ui.add(egui_tracing::Logs::new(self.ev.clone())));
    }
}
