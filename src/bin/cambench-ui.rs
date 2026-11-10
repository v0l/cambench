use std::path::{Path, PathBuf};

use cambench::gcode::{self, POSTS};
use cambench::{Report, run};
use egui::{
    Color32, ColorImage, Pos2, Rect, Sense, Stroke, TextureHandle, TextureOptions, Ui, Vec2,
};
use egui_bench::prelude::*;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tab {
    Stock,
    Findings,
    Gcode,
}

struct Loaded {
    report: Report,
    texture: TextureHandle,
    stock_size: [f64; 3],
    tags: Vec<usize>,
    times: Vec<f64>,
}

struct App {
    installed: bool,
    path: String,
    tab: Tab,
    loaded: Option<Result<Loaded, String>>,
    browser: Option<Browser>,
    post: usize,
    show_rapids: bool,
    scrub: usize,
    notice: Option<(String, bool)>,
}

fn stock_image(r: &Report) -> ColorImage {
    let s = &r.sim.stock;
    let mut pixels = Vec::with_capacity(s.nx * s.ny);
    for j in (0..s.ny).rev() {
        for i in 0..s.nx {
            let t = (s.h[j * s.nx + i] / s.bottom).clamp(0.0, 1.0);
            pixels.push(VALUE.lerp_to_gamma(WELL, t));
        }
    }
    ColorImage {
        size: [s.nx, s.ny],
        source_size: Vec2::new(s.nx as f32, s.ny as f32),
        pixels,
    }
}

impl App {
    fn new(ctx: &egui::Context, path: Option<String>) -> Self {
        install(ctx);
        let mut app = App {
            installed: true,
            path: path.unwrap_or_default(),
            tab: Tab::Stock,
            loaded: None,
            browser: None,
            post: 0,
            show_rapids: true,
            scrub: usize::MAX,
            notice: None,
        };
        if !app.path.is_empty() {
            app.load(ctx);
        }
        app
    }

    fn load(&mut self, ctx: &egui::Context) {
        let path = PathBuf::from(self.path.trim());
        let result = std::fs::read_to_string(&path)
            .map_err(|e| format!("reading {}: {e}", path.display()))
            .and_then(|src| {
                let base = path.parent().map(Path::to_path_buf).unwrap_or_default();
                let (job, report) = run(&src, &base)?;
                let texture =
                    ctx.load_texture("stock", stock_image(&report), TextureOptions::NEAREST);
                let tags = report.program.op_of_moves();
                let times = report.program.move_seconds();
                Ok(Loaded {
                    tags,
                    times,
                    report,
                    texture,
                    stock_size: [job.stock[0], job.stock[1], job.stock[2]],
                })
            });
        if let Err(e) = &result {
            self.notice = Some((e.clone(), true));
        }
        self.scrub = usize::MAX;
        self.loaded = Some(result);
    }

    fn header(&mut self, ui: &mut Ui) {
        ui.label(
            egui::RichText::new("CAMBENCH")
                .font(legend_font(16.0))
                .color(VALUE)
                .extra_letter_spacing(2.0),
        );
        ui.add_space(4.0);
        tabs(
            ui,
            &mut self.tab,
            &[
                (Tab::Stock, "stock"),
                (Tab::Findings, "findings"),
                (Tab::Gcode, "g-code"),
            ],
        );
    }

    fn file_row(&mut self, ui: &mut Ui) {
        let (mut load, mut browse) = (false, false);
        field_then(ui, &mut self.path, "path to a .gcam file", 190.0, |ui| {
            load = ui
                .add(Key::new("load").solid(READOUT).min_width(80.0))
                .clicked();
            browse = ui
                .add(Key::new("open").outline(VALUE).min_width(80.0))
                .clicked();
        });
        if browse {
            let dir = Path::new(self.path.trim())
                .parent()
                .filter(|p| p.is_dir())
                .map(Path::to_path_buf);
            let dir = dir
                .or_else(|| std::env::current_dir().ok())
                .unwrap_or_default();
            self.browser = Some(
                Browser::new("open a job", dir)
                    .suffixes(&[".gcam"])
                    .folders(false),
            );
        }
        if load {
            self.load(ui.ctx());
        }
    }

    fn browse(&mut self, ctx: &egui::Context) {
        let Some(b) = &mut self.browser else { return };
        match b.show(ctx) {
            Picked::Browsing => {}
            Picked::Cancelled => self.browser = None,
            Picked::File(p) | Picked::Folder(p) => {
                self.browser = None;
                self.path = p.display().to_string();
                self.load(ctx);
            }
        }
    }

    fn stock_view(&mut self, ui: &mut Ui) {
        let Some(Ok(l)) = &self.loaded else { return };
        let avail = ui.available_size();
        let (w, h) = (l.stock_size[0] as f32, l.stock_size[1] as f32);
        let scale = ((avail.x - 4.0) / w)
            .min((avail.y - 120.0).max(120.0) / h)
            .max(1.0);
        let (rect, _) = ui.allocate_exact_size(Vec2::new(w * scale, h * scale), Sense::hover());
        let painter = ui.painter_at(rect);
        painter.image(
            l.texture.id(),
            rect,
            Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
            Color32::WHITE,
        );
        let at = |p: [f64; 3]| {
            Pos2::new(
                rect.min.x + p[0] as f32 * scale,
                rect.max.y - p[1] as f32 * scale,
            )
        };
        let moves = l.report.program.moves();
        let last = moves.len().saturating_sub(1);
        let upto = self.scrub.min(last);
        let palette = [READOUT, TRACE, OK, WARN, FAULT, SAFE, VALUE];
        let mut prev: Option<[f64; 3]> = None;
        for (i, m) in moves.iter().enumerate().take(upto + 1) {
            if let Some(p) = prev {
                let flat_rapid = m.rapid && p[0] == m.to[0] && p[1] == m.to[1];
                if !flat_rapid && (!m.rapid || self.show_rapids) {
                    let colour = palette[l.tags[i] % palette.len()];
                    let colour = if m.rapid {
                        colour.gamma_multiply(0.35)
                    } else {
                        colour
                    };
                    painter.line_segment([at(p), at(m.to)], Stroke::new(1.0, colour));
                }
            }
            prev = Some(m.to);
        }
        if let Some(p) = prev {
            painter.circle_stroke(at(p), 5.0, Stroke::new(1.5, FAULT));
        }
        ui.add_space(6.0);
        let mut at_move = upto;
        Line::new()
            .legend("timeline")
            .column(ui, 98.0)
            .measured(&format!(
                "{} of {}  {}m{:02}s of {}m{:02}s",
                upto + 1,
                moves.len(),
                (l.times[upto] / 60.0) as u32,
                (l.times[upto] % 60.0) as u32,
                (l.times[last] / 60.0) as u32,
                (l.times[last] % 60.0) as u32
            ))
            .show(ui);
        ui.spacing_mut().slider_width = (ui.available_width() - 80.0).max(120.0);
        ui.add(egui::Slider::new(&mut at_move, 0..=last).show_value(false));
        self.scrub = at_move;
        ui.horizontal_wrapped(|ui| {
            for (k, op) in l.report.program.ops.iter().enumerate() {
                chip(
                    ui,
                    &format!("{} {}", op.line, op.verb),
                    palette[k % palette.len()],
                    true,
                );
            }
        });
        ui.add_space(6.0);
        switch(
            ui,
            "rapids",
            &mut self.show_rapids,
            "show rapid moves",
            "Rapids are the dashed travel between cuts.",
        );
    }

    fn summary(&self, ui: &mut Ui) {
        let Some(Ok(l)) = &self.loaded else { return };
        let r = &l.report;
        let secs = r.program.estimate_seconds();
        section(ui, "job", "from the planner and the simulation", |ui| {
            Line::new()
                .legend("operations")
                .column(ui, 98.0)
                .measured(r.program.ops.len().to_string())
                .show(ui);
            Line::new()
                .legend("moves")
                .column(ui, 98.0)
                .measured(r.program.moves().len().to_string())
                .show(ui);
            Line::new()
                .legend("removed")
                .column(ui, 98.0)
                .measured(format!("{:.0} mm3", r.sim.removed_mm3))
                .show(ui);
            Line::new()
                .legend("time")
                .column(ui, 98.0)
                .measured(format!(
                    "{}m{:02}s",
                    (secs / 60.0) as u32,
                    (secs % 60.0) as u32
                ))
                .show(ui);
            status(
                ui,
                r.sim.findings.ok(),
                &format!("{} findings", r.sim.findings.count()),
            );
        });
    }

    fn findings(&self, ui: &mut Ui) {
        let Some(Ok(l)) = &self.loaded else { return };
        let f = &l.report.sim.findings;
        if f.ok() {
            status(ui, true, "no gouges, remaining stock, rapid or holder hits");
            return;
        }
        for line in f.lines() {
            status(ui, false, &line);
        }
    }

    fn gcode(&mut self, ui: &mut Ui) {
        let Some(Ok(l)) = &self.loaded else { return };
        let (name, kind) = POSTS[self.post];
        ui.horizontal(|ui| {
            for (i, (n, _)) in POSTS.iter().enumerate() {
                if chip(ui, n, READOUT, i == self.post)
                    .interact(Sense::click())
                    .clicked()
                {
                    self.post = i;
                }
            }
        });
        ui.add_space(6.0);
        match gcode::emit(&l.report.program, kind) {
            Err(e) => status(ui, false, &e),
            Ok(text) => {
                match gcode::backplot_check(&l.report.program, &text) {
                    Ok(n) => status(ui, true, &format!("{n} moves reproduce the toolpath")),
                    Err(e) => status(ui, false, &e),
                }
                let save = ui
                    .add(Key::new("save").outline(VALUE).min_width(100.0))
                    .clicked();
                let head: String = text.lines().take(400).collect::<Vec<_>>().join("\n");
                ui.add_space(6.0);
                ui.label(egui::RichText::new(head).font(mono(11.0)).color(VALUE));
                if save {
                    let out = Path::new(self.path.trim()).with_extension(format!("{name}.nc"));
                    self.notice = Some(match std::fs::write(&out, &text) {
                        Ok(()) => (format!("wrote {}", out.display()), false),
                        Err(e) => (format!("writing {}: {e}", out.display()), true),
                    });
                }
            }
        }
    }

    fn show(&mut self, ui: &mut Ui) {
        if !self.installed {
            install(ui.ctx());
            self.installed = true;
        }
        egui::Panel::top("header")
            .frame(egui::Frame::NONE.fill(CHASSIS).inner_margin(egui::Margin {
                left: 12,
                right: 12,
                top: 8,
                bottom: 6,
            }))
            .show(ui, |ui| self.header(ui));
        if let Some((msg, fault)) = self.notice.clone() {
            let tone = if fault { Tone::Fault } else { Tone::Ok };
            if toast(ui.ctx(), "notice", &msg, tone) {
                self.notice = None;
            }
        }
        egui::CentralPanel::no_frame().show(ui, |ui| {
            let r = ui.max_rect();
            ui.painter().rect_filled(r, 0.0, CHASSIS);
            ui.painter()
                .line_segment([r.left_top(), r.right_top()], Stroke::new(1.0, ETCH));
            egui::Frame::NONE
                .inner_margin(egui::Margin::same(10))
                .show(ui, |ui| {
                    egui::ScrollArea::both()
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            ui.spacing_mut().item_spacing.y = 8.0;
                            self.file_row(ui);
                            match &self.loaded {
                                None => note(
                                    ui,
                                    "Open a .gcam file to plan, simulate and post it.",
                                    LEGEND,
                                ),
                                Some(Err(e)) => status(ui, false, e),
                                Some(Ok(_)) => {
                                    self.summary(ui);
                                    match self.tab {
                                        Tab::Stock => self.stock_view(ui),
                                        Tab::Findings => self.findings(ui),
                                        Tab::Gcode => self.gcode(ui),
                                    }
                                }
                            }
                        });
                });
        });
        self.browse(ui.ctx());
    }
}

struct Shell(App);

impl eframe::App for Shell {
    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        self.0.show(ui);
    }
}

fn main() -> eframe::Result<()> {
    let path = std::env::args().nth(1);
    let viewport = egui::ViewportBuilder::default()
        .with_inner_size([1000.0, 800.0])
        .with_min_inner_size([480.0, 480.0])
        .with_title("cambench")
        .with_app_id("cambench");
    eframe::run_native(
        "cambench",
        eframe::NativeOptions {
            viewport,
            ..Default::default()
        },
        Box::new(|cc| Ok(Box::new(Shell(App::new(&cc.egui_ctx, path))))),
    )
}
