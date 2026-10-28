//! The blossom welcome animation for onboarding, with a full-color final pose.
//! Visible time pauses while hidden. Fresh conversations start settled and can replay on a click.

mod geometry;
mod lighting;
mod paths;
mod policy;
mod renderer;
mod sequence;

use std::sync::OnceLock;
use std::time::Duration;
use std::time::Instant;

use crossterm::event::MouseButton;
use crossterm::event::MouseEvent;
use crossterm::event::MouseEventKind;
use ratatui::buffer::Buffer;
use ratatui::layout::Position;
use ratatui::layout::Rect;
use ratatui::style::Color;

use crate::motion::MotionMode;
use crate::terminal_palette;

fn ascii_mark_source() -> &'static str {
    include_str!("../assets/noname_logo.txt")
}

fn is_blank_char(c: char) -> bool {
    c == ' ' || c == '\u{2800}' || c == '\t'
}

fn line_is_blank(line: &str) -> bool {
    line.chars().all(is_blank_char)
}

fn crop_to_content(art: &str) -> Vec<&str> {
    let lines: Vec<&str> = art.lines().collect();
    let first = lines.iter().position(|l| !line_is_blank(l)).unwrap_or(lines.len());
    let trimmed_rows = &lines[first..];
    let last = trimmed_rows.iter().rposition(|l| !line_is_blank(l)).map_or(0, |i| i + 1);
    let mut rows = &trimmed_rows[..last];
    let mut left = usize::MAX;
    let mut right = 0usize;
    for line in rows {
        for (i, c) in line.chars().enumerate() {
            if !is_blank_char(c) {
                left = left.min(i);
                right = right.max(i + 1);
            }
        }
    }
    if left == usize::MAX || right <= left {
        rows = &[];
    }
    rows.iter()
        .copied()
        .map(|line| {
            let chars: Vec<char> = line.chars().collect();
            let _ = chars;
            line
        })
        .collect()
}

fn char_to_dots(c: char) -> [[bool; 2]; 4] {
    let b = (c as u32).wrapping_sub(0x2800);
    if c < char::from_u32(0x2800).unwrap_or(' ') || c > char::from_u32(0x28FF).unwrap_or(' ') {
        return [[false, false]; 4];
    }
    [
        [b & 0x01 != 0, b & 0x08 != 0],
        [b & 0x02 != 0, b & 0x10 != 0],
        [b & 0x04 != 0, b & 0x20 != 0],
        [b & 0x40 != 0, b & 0x80 != 0],
    ]
}

const DOT_BITS: [[u32; 2]; 4] = [[0x01, 0x08], [0x02, 0x10], [0x04, 0x20], [0x40, 0x80]];

fn paint_ascii_mark_full(buffer: &mut Buffer, art: &str, faded: bool) {
    let screen = buffer.area;
    let area = Rect::new(screen.x, screen.y, screen.width, screen.height.saturating_sub(4));
    paint_ascii_mark(area, buffer, art, faded);
}

fn paint_ascii_mark(area: Rect, buffer: &mut Buffer, art: &str, faded: bool) {
    let lines: Vec<&str> = art
        .lines()
        .skip_while(|line| line.chars().all(|c| c == ' ' || c == '\u{2800}'))
        .collect();
    let lines: Vec<&str> = lines
        .into_iter()
        .rev()
        .skip_while(|line| line.chars().all(|c| c == ' ' || c == '\u{2800}'))
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    // Trim leading padding columns.
    let mut left_pad = usize::MAX;
    for line in &lines {
        for (i, c) in line.chars().enumerate() {
            if c != ' ' && c != '\u{2800}' {
                left_pad = left_pad.min(i);
                break;
            }
        }
    }
    if left_pad == usize::MAX {
        left_pad = 0;
    }
    let trimmed: Vec<String> = lines
        .iter()
        .map(|line| line.chars().skip(left_pad).collect::<String>())
        .collect();
    if trimmed.is_empty() {
        return;
    }
    let in_rows = trimmed.len();
    let in_cols = trimmed.iter().map(|l| l.chars().count()).max().unwrap_or(0);
    if in_rows == 0 || in_cols == 0 || area.is_empty() {
        return;
    }
    // Scale down so the art fits comfortably and looks smaller.
    let factor = in_rows
        .div_ceil(12)
        .max(in_cols.div_ceil(40))
        .max(2usize);
    let out_rows = (in_rows / factor).max(1);
    let out_cols = (in_cols / factor).max(1);
    let mut out: Vec<String> = vec![String::new(); out_rows];
    for (ry, row) in out.iter_mut().enumerate() {
        let mut s = String::new();
        for rx in 0..out_cols {
            let sy = (ry * factor).min(in_rows - 1);
            let sx = (rx * factor).min(in_cols - 1);
            let ch = trimmed[sy].chars().nth(sx).unwrap_or(' ');
            s.push(ch);
        }
        *row = s;
    }
    let rows = out_rows as u16;
    let cols = out_cols as u16;
    let top = area.y + area.height.saturating_sub(rows) / 2;
    let left = area.x + area.width.saturating_sub(cols) / 2;
    let bottom = area.y + area.height;
    let right = area.x + area.width;
    let color = if faded { Color::Rgb(70, 70, 70) } else { Color::Rgb(110, 110, 110) };
    for (dy, line) in out.iter().enumerate() {
        let y = top + dy as u16;
        if y >= bottom {
            break;
        }
        for (dx, ch) in line.chars().enumerate() {
            let x = left + dx as u16;
            if x >= right {
                break;
            }
            let cell = &mut buffer[(x, y)];
            cell.set_char(ch);
            cell.set_fg(color);
        }
    }
}
use lighting::Lighting;
pub(crate) use policy::Presentation;
pub(crate) use policy::is_startup_cell;
use renderer::MAX_COLUMNS;
use renderer::MAX_ROWS;
use renderer::Renderer;

pub(crate) const FRAME_INTERVAL: Duration = Duration::from_millis(/*millis*/ 50);
// The preview's first visible pose (10.8s / 7.2s), also the start of an idle-screen replay.
const SETTLED_BLOSSOM: f64 = 0.5;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum ComposerState {
    Empty,
    Draft,
}

#[derive(Default)]
pub(crate) struct EmptyStateAnimation {
    eligible: bool,
    spin_elapsed: Duration,
    last_frame: Option<Instant>,
    fade_elapsed: Duration,
    static_mark: Option<bool>,
    opacity: f32,
    fade_from: f32,
    renderer: Option<Renderer>,
    stage: Option<Rect>,
    replaying: bool,
}

impl EmptyStateAnimation {
    pub(crate) fn is_eligible(&self) -> bool {
        self.eligible
    }

    pub(crate) fn start_fresh(&mut self) {
        self.cancel_replay();
        self.eligible = true;
        self.spin_elapsed = Duration::ZERO;
        self.last_frame = None;
        self.fade_elapsed = Duration::ZERO;
        self.static_mark = None;
        self.opacity = 1.0;
    }

    /// Keep the provisional pose when handing off to the live thread.
    pub(crate) fn continue_from(&mut self, source: &mut Self) {
        let mut previous = std::mem::take(source);
        if !previous.is_eligible() {
            previous.start_fresh();
        }
        *self = previous;
    }

    pub(crate) fn dismiss(&mut self) {
        self.eligible = false;
        self.cancel_replay();
    }

    pub(crate) fn cancel_replay(&mut self) {
        self.stage = None;
        self.replaying = false;
        self.pause_clock();
    }

    /// Replay only on the drawn blossom; unrelated gestures keep their normal owner.
    pub(crate) fn handle_mouse(&mut self, mouse: MouseEvent) -> bool {
        let inside = self
            .stage
            .is_some_and(|stage| stage.contains(Position::new(mouse.column, mouse.row)))
            && mouse.modifiers.is_empty();
        match mouse.kind {
            MouseEventKind::Down(MouseButton::Left) if inside => {
                self.start_fresh();
                self.spin_elapsed =
                    Duration::from_secs_f64(SETTLED_BLOSSOM * sequence::LOOP_SECONDS);
                self.opacity = sequence::STATIC_OPACITY;
                self.static_mark = Some(true);
                self.replaying = true;
                true
            }
            _ => false,
        }
    }

    /// Stop visible time at the last painted frame, including when no hidden frame was drawn.
    /// Resume (after process suspension) may call this after the interruption; phase is retained.
    pub(crate) fn pause_clock(&mut self) {
        self.last_frame = None;
    }

    /// Paint the shared logo sequence inside a caller-owned, reserved rectangle.
    /// The caller clears the stage and keeps its layout stable after motion finishes.
    pub(crate) fn render_in(
        &mut self,
        area: Rect,
        buffer: &mut Buffer,
        presentation: Presentation,
    ) -> Option<Duration> {
        let now = Instant::now();
        self.render_in_at(area, buffer, presentation, now)
    }

    fn render_in_at(
        &mut self,
        area: Rect,
        buffer: &mut Buffer,
        presentation: Presentation,
        now: Instant,
    ) -> Option<Duration> {
        if !self.eligible
            || presentation == Presentation::Hidden
            || area.is_empty()
            || area.width > MAX_COLUMNS
            || area.height > MAX_ROWS
            || area.intersection(buffer.area) != area
        {
            self.pause_clock();
            return None;
        }
        let static_mark = presentation == Presentation::Faded;
        // Replace the blossom morph with the user's static ascii mark.
        let ascii_mark = include_str!("../assets/noname_logo.txt");
        paint_ascii_mark_full(buffer, ascii_mark, static_mark);
        self.pause_clock();
        return None;
        let previous_frame = self.last_frame.replace(now);
        if self.static_mark != Some(static_mark) {
            self.fade_elapsed = if self.static_mark.is_none() {
                sequence::STATIC_FADE
            } else {
                Duration::ZERO
            };
            self.fade_from = self.opacity;
        } else if let Some(previous) = previous_frame {
            let elapsed = now.saturating_duration_since(previous);
            self.fade_elapsed += elapsed;
            if !static_mark {
                self.spin_elapsed += elapsed;
            }
        }
        self.static_mark = Some(static_mark);
        let finished = self.spin_elapsed >= sequence::SPIN_DURATION;
        let phase =
            self.spin_elapsed.min(sequence::SPIN_DURATION).as_secs_f64() / sequence::LOOP_SECONDS;
        let settling = !finished && static_mark && self.fade_elapsed < sequence::STATIC_FADE;
        self.opacity = if finished && self.replaying {
            sequence::static_opacity(
                self.spin_elapsed - sequence::SPIN_DURATION,
                /*from*/ 1.0,
            )
        } else if finished {
            1.0
        } else if static_mark {
            sequence::static_opacity(self.fade_elapsed, self.fade_from)
        } else {
            1.0
        };
        if !static_mark && !finished {
            self.opacity = self.fade_from
                + (self.opacity - self.fade_from)
                    * sequence::progress(self.fade_elapsed, sequence::STATIC_FADE) as f32;
        }
        self.paint_frame(area, buffer, phase, self.opacity);
        ((!finished && (!static_mark || settling))
            || (self.replaying
                && self.spin_elapsed < sequence::SPIN_DURATION + sequence::STATIC_FADE))
            .then_some(FRAME_INTERVAL)
    }

    /// Draw only in space the caller has cleared and owns. The centered blossom returns
    /// on clear, without restarting or scheduling animation.
    pub(crate) fn render_first_screen(
        &mut self,
        available: Rect,
        buffer: &mut Buffer,
        composer: Option<ComposerState>,
        motion: MotionMode,
    ) -> Option<Duration> {
        const STAGE_ROWS: u16 = 21;
        const MIN_STAGE_ROWS: u16 = 14;
        let screen = buffer.area;
        let width = screen.width.saturating_sub(/*rhs*/ 4).min(MAX_COLUMNS);
        let height = width * STAGE_ROWS / MAX_COLUMNS;
        if motion == MotionMode::Reduced
            || composer != Some(ComposerState::Empty)
            || height < MIN_STAGE_ROWS
            || height > screen.height
        {
            self.cancel_replay();
            return None;
        }
        let stage = Rect::new(
            screen.x + (screen.width - width) / 2,
            screen.y + (screen.height - height) / 2,
            width,
            height,
        );
        if stage.intersection(available) != stage {
            self.cancel_replay();
            return None;
        }
        self.stage = Some(stage);
        if self.replaying {
            if let Some(delay) = self.render_in(stage, buffer, Presentation::Animated) {
                return Some(delay);
            }
            // The replay has faded back to the idle pose and no longer needs redraws.
            self.replaying = false;
        }
        self.paint_frame(stage, buffer, SETTLED_BLOSSOM, sequence::STATIC_OPACITY);
        None
    }

    fn paint_frame(&mut self, area: Rect, buffer: &mut Buffer, phase: f64, opacity: f32) {
        // Replace the blossom morph with the user's static ascii mark.
        let ascii_mark = include_str!("../assets/noname_logo.txt");
        paint_ascii_mark_full(buffer, ascii_mark, opacity < 0.999);
        return;
        let background = terminal_palette::default_bg();
        let color_level = if background.is_some() {
            terminal_palette::effective_stdout_color_level()
        } else {
            terminal_palette::StdoutColorLevel::Unknown
        };
        let background = background.unwrap_or((15, 20, 37));
        let light = Lighting::terminal(
            terminal_palette::default_fg().unwrap_or((210, 221, 235)),
            background,
        );
        let cells = self.renderer.get_or_insert_with(Renderer::default).frame(
            area.width,
            area.height,
            phase,
            &light,
        );
        for (i, cell) in cells.iter().enumerate().filter(|(_, cell)| cell.dots != 0) {
            let [_, r, g, b] = cell.rgb.to_be_bytes();
            let target = &mut buffer[(
                area.x + i as u16 % area.width,
                area.y + i as u16 / area.width,
            )];
            target.set_char(char::from_u32(0x2800 + u32::from(cell.dots)).unwrap_or(' '));
            let color = crate::color::blend((r, g, b), background, opacity);
            let color = if color_level == terminal_palette::StdoutColorLevel::Ansi256 {
                // Four bits per channel bound the cache and avoid palette searches each frame.
                static COLORS: [OnceLock<Color>; 4096] = [const { OnceLock::new() }; 4096];
                let (r, g, b) = (color.0 >> 4, color.1 >> 4, color.2 >> 4);
                let index = usize::from(r) * 256 + usize::from(g) * 16 + usize::from(b);
                *COLORS[index].get_or_init(|| {
                    terminal_palette::best_color_for_level(
                        (r * 16 + 8, g * 16 + 8, b * 16 + 8),
                        color_level,
                    )
                })
            } else {
                terminal_palette::best_color_for_level(color, color_level)
            };
            target.set_fg(color);
            // Default-color terminals can only step between normal and dim intensity.
            if opacity < 0.5
                && matches!(
                    color_level,
                    terminal_palette::StdoutColorLevel::Ansi16
                        | terminal_palette::StdoutColorLevel::Unknown
                )
            {
                target.set_style(target.style().dim());
            }
        }
    }
}

#[cfg(test)]
#[path = "empty_state_animation_tests.rs"]
mod tests;
