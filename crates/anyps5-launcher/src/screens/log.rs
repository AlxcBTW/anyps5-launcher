//! The log view, the diagnosis presentation and "Make a Report" (docs/DESIGN.md 5.12 and 5.13).
//!
//! * [`LogView`]: a full-screen overlay with the game's log in a selectable monospaced box that
//!   starts at the bottom, the diagnosis sentence above it, and the buttons "Make a Report" and
//!   "Close". Back and select close it.
//! * [`DiagnosisView`]: the UI's own copy of the pipeline's diagnosis categories with the sentence
//!   in the launcher's language. The host maps the pipeline's `Diagnosis` onto it.
//! * [`save_report_with_dialog`]: the report text goes through a native save dialog.

use std::path::{Path, PathBuf};

use egui::{Align2, Color32, Context, Layout, Rect, RichText, ScrollArea, Ui, UiBuilder, pos2, vec2};

use crate::screens::dialogs;
use crate::screens::side::commands;
use crate::screens::strings::Extra;
use crate::screens::ToastRequest;
use crate::ui::Shell;
use crate::ui::chrome::fullscreen_panel;
use crate::ui::components::ConsoleButton;
use crate::ui::glass::{GlassStyle, paint_glass};
use crate::ui::i18n::{Arg, Key, Language};
use crate::ui::icons::Icon;
use crate::ui::input::{Command, InputEvent};
use crate::ui::sound::Cue;
use crate::ui::text;
use crate::ui::theme::{self, color, pad, ty, white};

/// Longest engine message shown in a sentence, cut with an ellipsis.
pub const ENGINE_MESSAGE_LIMIT: usize = 200;

/// Why a game stopped, as the UI shows it. One variant per category of the pipeline's diagnosis.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DiagnosisView {
    /// A module (DLL or PRX) could not be loaded.
    MissingModule(String),
    /// The game called a system function the engine does not implement yet.
    Unsupported(String),
    GpuLost,
    OutOfMemory,
    /// Any other error the engine stopped on, with its message.
    EngineError(String),
    /// The process died by an exception, for example `0xC0000005 (ACCESS_VIOLATION)`.
    Crashed(String),
    /// The host CPU lacks extensions the game needs.
    HostCpu(String),
}

impl DiagnosisView {
    /// One sentence in the launcher's language.
    pub fn summary(&self, language: Language) -> String {
        match self {
            DiagnosisView::MissingModule(name) => {
                language.format(Key::DiagnosisMissingModule, &[Arg::Str(name)])
            }
            DiagnosisView::Unsupported(function) => {
                language.format(Key::DiagnosisUnsupported, &[Arg::Str(function)])
            }
            DiagnosisView::GpuLost => language.text(Key::DiagnosisGpuLost).to_owned(),
            DiagnosisView::OutOfMemory => language.text(Key::DiagnosisOutOfMemory).to_owned(),
            DiagnosisView::EngineError(message) => language.format(
                Key::DiagnosisEngineError,
                &[Arg::Str(&cut_message(message, ENGINE_MESSAGE_LIMIT))],
            ),
            DiagnosisView::Crashed(what) => {
                language.format(Key::DiagnosisCrashed, &[Arg::Str(what)])
            }
            DiagnosisView::HostCpu(list) => {
                Extra::DiagnosisHostCpu.format(language, &[Arg::Str(list)])
            }
        }
    }

    /// The toast after a game ended: `"<game>: <sentence>"` with a warning glyph.
    pub fn toast(&self, game_name: &str, language: Language) -> ToastRequest {
        ToastRequest::warning(format!("{game_name}: {}", self.summary(language)))
    }
}

/// Cuts a message at `limit` characters and adds an ellipsis.
pub fn cut_message(message: &str, limit: usize) -> String {
    if message.chars().count() <= limit {
        message.to_owned()
    } else {
        let cut: String = message.chars().take(limit).collect();
        format!("{cut}…")
    }
}

/// The amber diagnosis line of the hero (5.3): a warning glyph and the sentence, 14 regular, at
/// most two lines and 520 wide.
pub fn diagnosis_line(ui: &mut Ui, diagnosis: &DiagnosisView, language: Language) -> egui::Response {
    let sentence = diagnosis.summary(language);
    let painter = ui.painter().clone();
    let style = ty::DETAIL_LARGE;
    let width = 520.0;
    let glyph = 16.0;
    let galley = {
        let mut job = egui::text::LayoutJob::simple(
            sentence.clone(),
            style.font(),
            Color32::PLACEHOLDER,
            width - glyph - 8.0,
        );
        job.wrap.max_rows = 2;
        job.sections[0].format.line_height = Some(style.line_height());
        painter.layout_job(job)
    };
    let (rect, response) = ui.allocate_exact_size(
        vec2(glyph + 8.0 + galley.size().x, galley.size().y.max(glyph)),
        egui::Sense::hover(),
    );
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Label, true, sentence.as_str()));
    if ui.is_rect_visible(rect) {
        painter.text(
            pos2(rect.min.x + glyph * 0.5, rect.min.y + style.line_height() * 0.5),
            Align2::CENTER_CENTER,
            Icon::WARNING.glyph(),
            Icon::WARNING.font(glyph, true),
            color::warning_text(),
        );
        painter.galley(
            pos2(rect.min.x + glyph + 8.0, rect.min.y),
            galley,
            color::warning_text(),
        );
    }
    response
}

/// What the host has to do after a frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LogAction {
    /// Back, select or the Close button.
    Close,
    /// The "Make a Report" button.
    MakeReport,
}

/// The log, borrowed for one frame.
pub struct LogData<'a> {
    pub game_name: Option<&'a str>,
    pub diagnosis: Option<&'a DiagnosisView>,
    /// The in-memory log, the failure message and the file's tail, as one text.
    pub text: &'a str,
    pub language: Language,
}

/// The log overlay.
#[derive(Default)]
pub struct LogView {
    open: bool,
    closing: bool,
}

impl LogView {
    pub fn open(&mut self) {
        self.open = true;
        self.closing = false;
    }

    pub fn close(&mut self) {
        if self.open {
            self.open = false;
            self.closing = true;
        }
    }

    pub fn is_active(&self) -> bool {
        self.open || self.closing
    }

    pub fn show(
        &mut self,
        shell: &mut Shell,
        ctx: &Context,
        events: &[InputEvent],
        data: &LogData<'_>,
    ) -> Vec<LogAction> {
        let mut actions = Vec::new();
        if !self.is_active() {
            return actions;
        }
        // Back and select close it; everything else is swallowed by the host.
        if self.open
            && commands(events)
                .iter()
                .any(|c| matches!(c, Command::Back | Command::Select))
        {
            shell.play(Cue::Back);
            self.close();
            actions.push(LogAction::Close);
        }
        let mut clicked = None;
        let drawn = fullscreen_panel(ctx, "log-view", self.open, GlassStyle::log_panel(), |ui, _| {
            draw(ui, data, &mut clicked)
        });
        if drawn.is_none() {
            self.closing = false;
        }
        if self.open {
            match clicked {
                Some(LogAction::Close) => {
                    shell.play(Cue::Back);
                    self.close();
                    actions.push(LogAction::Close);
                }
                Some(LogAction::MakeReport) => {
                    shell.play(Cue::Select);
                    actions.push(LogAction::MakeReport);
                }
                None => {}
            }
        }
        actions
    }
}

/// The text shown for an empty log.
const EMPTY_LOG: &str = "—";

fn draw(ui: &mut Ui, data: &LogData<'_>, clicked: &mut Option<LogAction>) {
    let language = data.language;
    let painter = ui.painter().clone();
    let area = ui.max_rect().shrink(pad::LOG);
    let mut y = area.min.y;

    // Header: the title, then the two buttons on the right.
    let title = match data.game_name {
        Some(name) => format!("{} · {name}", language.text(Key::LogTitle)),
        None => language.text(Key::LogTitle).to_owned(),
    };
    let header_h = 54.0;
    let header = Rect::from_min_size(pos2(area.min.x, y), vec2(area.width(), header_h));
    {
        let max = (area.width() - 520.0).max(200.0);
        let shown = text::middle_truncate(&painter, ty::LOG_TITLE, &title, max);
        text::paint(
            &painter,
            pos2(header.min.x, header.center().y),
            Align2::LEFT_CENTER,
            ty::LOG_TITLE,
            &shown,
            Color32::WHITE,
        );
    }
    let mut buttons = ui.new_child(
        UiBuilder::new()
            .max_rect(header)
            .layout(Layout::right_to_left(egui::Align::Center)),
    );
    buttons.spacing_mut().item_spacing.x = 14.0;
    // Right to left: Close first, then the report button to its left.
    if ConsoleButton::new(language.text(Key::Close))
        .icon(Icon::X)
        .focused(true)
        .show(&mut buttons)
        .clicked()
    {
        *clicked = Some(LogAction::Close);
    }
    if data.game_name.is_some()
        && ConsoleButton::new(language.text(Key::MakeReport))
            .icon(Icon::STETHOSCOPE)
            .show(&mut buttons)
            .clicked()
    {
        *clicked = Some(LogAction::MakeReport);
    }
    y += header_h + 18.0;

    // The diagnosis box.
    if let Some(diagnosis) = data.diagnosis {
        let sentence = diagnosis.summary(language);
        let inner_w = area.width() - 32.0 - 26.0;
        let galley = text::layout(&painter, ty::DIAGNOSIS_LOG, &sentence, Color32::PLACEHOLDER, inner_w);
        let h = galley.size().y.max(18.0) + 32.0;
        let rect = Rect::from_min_size(pos2(area.min.x, y), vec2(area.width(), h));
        paint_glass(ui.ctx(), &painter, rect, GlassStyle::diagnosis_box());
        painter.text(
            pos2(rect.min.x + 16.0 + 9.0, rect.min.y + 16.0 + ty::DIAGNOSIS_LOG.line_height() * 0.5),
            Align2::CENTER_CENTER,
            Icon::WARNING.glyph(),
            Icon::WARNING.font(17.0, true),
            color::warning_text(),
        );
        painter.galley(
            pos2(rect.min.x + 16.0 + 26.0, rect.min.y + 16.0),
            galley,
            color::warning_text(),
        );
        y += h + 18.0;
    }

    // The log box.
    let log_rect = Rect::from_min_max(pos2(area.min.x, y), area.max);
    paint_glass(ui.ctx(), &painter, log_rect, GlassStyle::log_box());
    let inner = log_rect.shrink(18.0);
    let mut child = ui.new_child(UiBuilder::new().max_rect(inner));
    let body = if data.text.is_empty() { EMPTY_LOG } else { data.text };
    ScrollArea::vertical()
        .id_salt("log-text")
        .auto_shrink([false, false])
        .stick_to_bottom(true)
        .scroll_bar_visibility(egui::containers::scroll_area::ScrollBarVisibility::VisibleWhenNeeded)
        .show(&mut child, |ui| {
            ui.add(
                egui::Label::new(
                    RichText::new(body)
                        .font(theme::font_mono(12.0))
                        .color(white(0.85)),
                )
                .selectable(true)
                .wrap(),
            );
        });
}

// ---------------------------------------------------------------------------------------------
// Report
// ---------------------------------------------------------------------------------------------

/// `TITLEID-yyyyMMdd-HHmmss.txt`
pub fn report_file_name(title_id: &str, now: time::OffsetDateTime) -> String {
    let safe: String = title_id
        .chars()
        .map(|c| if c.is_alphanumeric() || c == '-' { c } else { '_' })
        .collect();
    format!(
        "{safe}-{:04}{:02}{:02}-{:02}{:02}{:02}.txt",
        now.year(),
        u8::from(now.month()),
        now.day(),
        now.hour(),
        now.minute(),
        now.second()
    )
}

/// Asks where to save `text` (the report) and writes it. `None` when the dialog was cancelled;
/// otherwise the file and the toast to show (saved or failed).
pub fn save_report_with_dialog(
    shell: &mut Shell,
    language: Language,
    title_id: &str,
    text: &str,
    start_dir: Option<&Path>,
) -> Option<(Option<PathBuf>, ToastRequest)> {
    let now = time::OffsetDateTime::now_local().unwrap_or_else(|_| time::OffsetDateTime::now_utc());
    let path = dialogs::save_file(
        shell,
        Extra::SaveReport.text(language),
        &report_file_name(title_id, now),
        ("Text", &["txt"]),
        start_dir,
    )?;
    Some(match std::fs::write(&path, text) {
        Ok(()) => (
            Some(path),
            ToastRequest::new(Icon::FILE_TEXT, language.text(Key::ReportSaved)),
        ),
        Err(_) => (None, ToastRequest::warning(language.text(Key::ReportFailed))),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn date(y: i32, mo: u8, d: u8, h: u8, mi: u8, sec: u8) -> time::OffsetDateTime {
        let day = time::Date::from_calendar_date(y, time::Month::try_from(mo).unwrap(), d).unwrap();
        let t = time::Time::from_hms(h, mi, sec).unwrap();
        time::PrimitiveDateTime::new(day, t).assume_utc()
    }

    #[test]
    fn every_category_has_a_sentence_in_both_languages() {
        let all = [
            DiagnosisView::MissingModule("libgcc_s_seh-1.dll".into()),
            DiagnosisView::Unsupported("sceFoo".into()),
            DiagnosisView::GpuLost,
            DiagnosisView::OutOfMemory,
            DiagnosisView::EngineError("boom".into()),
            DiagnosisView::Crashed("0xC0000005 (ACCESS_VIOLATION)".into()),
            DiagnosisView::HostCpu("AVX2".into()),
        ];
        for language in Language::ALL {
            for d in &all {
                let s = d.summary(language);
                assert!(!s.is_empty() && !s.contains("%@"), "{d:?} {language:?}: {s}");
            }
        }
    }

    #[test]
    fn arguments_land_in_the_sentence() {
        assert_eq!(
            DiagnosisView::MissingModule("libX.dll".into()).summary(Language::English),
            "Missing module: libX.dll. Unlocking the game again may fix it."
        );
        assert_eq!(
            DiagnosisView::Crashed("0xC0000005 (ACCESS_VIOLATION)".into()).summary(Language::English),
            "The game crashed (0xC0000005 (ACCESS_VIOLATION))."
        );
        assert_eq!(
            DiagnosisView::HostCpu("AVX2".into()).summary(Language::English),
            "This CPU lacks AVX2, which the game needs."
        );
    }

    #[test]
    fn long_engine_messages_are_cut_at_200_characters() {
        let long = "x".repeat(300);
        let sentence = DiagnosisView::EngineError(long).summary(Language::English);
        assert_eq!(sentence, format!("Engine error: {}…", "x".repeat(200)));
        assert_eq!(cut_message("short", 200), "short");
    }

    #[test]
    fn the_toast_names_the_game() {
        let toast = DiagnosisView::GpuLost.toast("Stray", Language::English);
        assert!(toast.text.starts_with("Stray: "));
        assert_eq!(toast.icon, Icon::WARNING);
    }

    #[test]
    fn report_files_are_named_like_the_original() {
        let t = date(2026, 10, 17, 21, 14, 9);
        assert_eq!(report_file_name("PPSA02100", t), "PPSA02100-20261017-211409.txt");
        assert_eq!(report_file_name("a b/c", t), "a_b_c-20261017-211409.txt");
    }
}
