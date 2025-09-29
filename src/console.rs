use eframe::{NativeOptions, UserEvent, egui};
use egui_tracing::EventCollector;
use wayland::errors::wrap_noncritical_sync;
use winit::{event_loop::EventLoop, platform::wayland::EventLoopBuilderExtWayland};

pub fn thread_console(ev: EventCollector) {
    let mut opts = NativeOptions::default();
    opts.event_loop_builder = Some(Box::new(|ev| {
        ev.with_any_thread(true);
    }));

    eframe::run_native(
        "console",
        opts,
        Box::new(|ctx| Result::Ok(Box::new(ConsoleApp { ev }))),
    )
    .unwrap();
}

pub fn console_over_ev(ev: &EventLoop<UserEvent>) {
    let opts = NativeOptions::default();

    eframe::create_native(
        "console",
        opts,
        Box::new(|ctx| {
            Result::Ok(Box::new(ConsoleApp {
                ev: EventCollector::default(),
            }))
        }),
        ev,
    );
}

pub struct ConsoleApp {
    ev: EventCollector,
}

impl eframe::App for ConsoleApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default()
            .show(ctx, |ui| ui.add(egui_tracing::Logs::new(self.ev.clone())));
    }
}
