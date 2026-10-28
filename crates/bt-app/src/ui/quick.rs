//! The command palette (CommandPaletteWindow + CommandPaletteHelper):
//! a 460 DIP search box over a filtered command list, centred on the
//! screen, closed by Escape, by running a command or by losing focus.
//! Its colours are fixed in the original (#E6202020 with a #55FFFFFF
//! border, white text), whatever the theme.
use bt_app::win32::{
    gfx::{Align, Gfx, Rect, Rgba, Surface, TextStyle},
    wnd::{self, Button, Event, Window},
};
use std::rc::Rc;

const FONT: &str = "Segoe UI";
const WIDTH: f32 = 460.0;
const CARET: usize = 1;
const LIST_MAX: f32 = 340.0;
const ITEM_PAD_X: f32 = 10.0;
const ITEM_PAD_Y: f32 = 7.0;

// Virtual keys.
const VK_BACK: u16 = 0x08;
const VK_RETURN: u16 = 0x0d;
const VK_ESCAPE: u16 = 0x1b;
const VK_END: u16 = 0x23;
const VK_HOME: u16 = 0x24;
const VK_LEFT: u16 = 0x25;
const VK_UP: u16 = 0x26;
const VK_RIGHT: u16 = 0x27;
const VK_DOWN: u16 = 0x28;
const VK_DELETE: u16 = 0x2e;

/// What a palette entry does; the UI runs it after the palette closes.
#[derive(Clone, Debug, PartialEq)]
pub enum Command {
    Undo,
    Redo,
    Mixer,
    Settings,
    ControlPanel(&'static str),
    SoundSettings,
    /// Device id, display name.
    ToggleDeviceMute(String, String),
    SetDefault(String),
    /// Device id, app key, display name.
    ToggleAppMute(String, String, String),
    Exit,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    pub name: String,
    pub shortcut: String,
    pub tokens: Vec<String>,
    pub command: Command,
}

impl Entry {
    pub fn new(name: impl Into<String>, shortcut: &str, tokens: &[&str], command: Command) -> Self {
        Self {
            name: name.into(),
            shortcut: shortcut.into(),
            tokens: tokens.iter().map(|t| (*t).to_owned()).collect(),
            command,
        }
    }
}

/// FuzzyMatch: the name or a token contains the query, or the query's
/// characters appear in order in the name. `query` is trimmed lowercase.
pub fn matches(entry: &Entry, query: &str) -> bool {
    let name = entry.name.to_lowercase();
    if name.contains(query)
        || entry
            .tokens
            .iter()
            .any(|t| t.to_lowercase().contains(query))
    {
        return true;
    }
    let mut wanted = query.chars().peekable();
    for c in name.chars() {
        if wanted.peek() == Some(&c) {
            wanted.next();
        }
    }
    wanted.peek().is_none()
}

pub enum Reply {
    None,
    Run(Command),
}

pub struct Quick {
    pub window: Window,
    gfx: Rc<Gfx>,
    surface: Option<Surface>,
    pub visible: bool,
    entries: Vec<Entry>,
    /// Indices into `entries` shown for the current query.
    shown: Vec<usize>,
    query: String,
    /// Caret position in chars.
    caret: usize,
    caret_on: bool,
    selected: Option<usize>,
    hover: Option<usize>,
    scroll: f32,
    scale: f32,
    /// Top-left in screen pixels; the top stays put as the list shrinks.
    position: (i32, i32),
}

fn search_style() -> TextStyle {
    TextStyle::new(FONT, 14.0)
}
fn name_style() -> TextStyle {
    TextStyle::new(FONT, 13.0)
}
fn shortcut_style() -> TextStyle {
    TextStyle::new(FONT, 11.0)
}

impl Quick {
    pub fn new(gfx: Rc<Gfx>) -> windows::core::Result<Self> {
        let window = wnd::create(
            "Command Palette",
            wnd::Options {
                activate: true,
                click_through: false,
            },
        )?;
        Ok(Self {
            window,
            gfx,
            surface: None,
            visible: false,
            entries: vec![],
            shown: vec![],
            query: String::new(),
            caret: 0,
            caret_on: true,
            selected: None,
            hover: None,
            scroll: 0.0,
            scale: 1.0,
            position: (0, 0),
        })
    }

    // ---------- layout (DIPs) ----------

    fn box_height(&self) -> f32 {
        self.gfx.measure("Ag", &search_style()).1.ceil() + 12.0
    }
    fn item_height(&self) -> f32 {
        self.gfx.measure("Ag", &name_style()).1.ceil() + 2.0 * ITEM_PAD_Y
    }
    fn list_top(&self) -> f32 {
        12.0 + self.box_height() + 6.0 + 1.0 + 6.0
    }
    fn content_height(&self) -> f32 {
        self.shown.len() as f32 * self.item_height()
    }
    fn list_height(&self) -> f32 {
        self.content_height().min(LIST_MAX)
    }
    fn height(&self) -> f32 {
        self.list_top() + self.list_height() + 8.0
    }
    fn max_scroll(&self) -> f32 {
        (self.content_height() - self.list_height()).max(0.0)
    }

    /// Opens the palette with `entries`, or brings an open one forward.
    pub fn show(&mut self, entries: Vec<Entry>) {
        if self.visible {
            self.window.focus();
            return;
        }
        self.entries = entries;
        self.query.clear();
        self.caret = 0;
        self.selected = None;
        self.hover = None;
        self.scroll = 0.0;
        self.filter();
        // Nothing is selected until the first keystroke.
        self.selected = None;
        if self.surface.is_none() {
            self.surface = Surface::new(self.gfx.clone()).ok();
        }
        let (cx, cy) = wnd::cursor_pos();
        let monitor = wnd::monitor_at(cx, cy);
        self.scale = monitor.scale;
        let (mx, my, mw, mh) = monitor.work;
        let w = (WIDTH * self.scale).round() as i32;
        let h = (self.height() * self.scale).round() as i32;
        self.position = (mx + (mw - w) / 2, my + ((mh - h) / 2).max(0));
        self.visible = true;
        self.caret_on = true;
        self.paint();
        self.window.show(true);
        self.window.focus();
        self.window.set_timer(CARET, 530);
    }

    pub fn hide(&mut self) {
        if !self.visible {
            return;
        }
        self.visible = false;
        self.window.kill_timer(CARET);
        self.window.hide();
        self.entries.clear();
        self.shown.clear();
        if let Some(surface) = &mut self.surface {
            surface.release();
        }
    }

    fn filter(&mut self) {
        let query = self.query.trim().to_lowercase();
        self.shown = (0..self.entries.len())
            .filter(|&i| query.is_empty() || matches(&self.entries[i], &query))
            .collect();
        self.scroll = 0.0;
        self.hover = None;
        // SearchBox_TextChanged selects the first result.
        self.selected = (!self.shown.is_empty()).then_some(0);
    }

    fn scroll_into_view(&mut self) {
        let Some(i) = self.selected else { return };
        let item = self.item_height();
        let top = i as f32 * item;
        if top < self.scroll {
            self.scroll = top;
        } else if top + item > self.scroll + self.list_height() {
            self.scroll = top + item - self.list_height();
        }
    }

    fn item_at(&self, x: f32, y: f32) -> Option<usize> {
        let top = self.list_top();
        if x < 4.0 || x > WIDTH - 4.0 || y < top || y >= top + self.list_height() {
            return None;
        }
        let i = ((y - top + self.scroll) / self.item_height()) as usize;
        (i < self.shown.len()).then_some(i)
    }

    fn run_selected(&mut self) -> Reply {
        let command = self
            .selected
            .and_then(|i| self.shown.get(i))
            .map(|&i| self.entries[i].command.clone());
        match command {
            Some(command) => {
                self.hide();
                Reply::Run(command)
            }
            None => Reply::None,
        }
    }

    fn edit(&mut self, change: impl FnOnce(&mut Vec<char>, &mut usize)) {
        let mut chars: Vec<char> = self.query.chars().collect();
        let mut caret = self.caret.min(chars.len());
        change(&mut chars, &mut caret);
        let text: String = chars.into_iter().collect();
        self.caret = caret;
        self.caret_on = true;
        if text != self.query {
            self.query = text;
            self.filter();
        }
    }

    pub fn event(&mut self, event: Event) -> Reply {
        if !self.visible {
            return Reply::None;
        }
        let reply = match event {
            Event::Timer(CARET) => {
                self.caret_on = !self.caret_on;
                Reply::None
            }
            // Deactivated: the original closes.
            Event::Activate(false) => {
                self.hide();
                return Reply::None;
            }
            Event::Char(c) => {
                self.edit(|chars, caret| {
                    chars.insert(*caret, c);
                    *caret += 1;
                });
                Reply::None
            }
            Event::Key(key, _) => match key {
                VK_DOWN | VK_UP => {
                    if !self.shown.is_empty() {
                        let last = self.shown.len() - 1;
                        self.selected = Some(match (key, self.selected) {
                            (VK_DOWN, Some(i)) => (i + 1).min(last),
                            (VK_DOWN, None) => 0,
                            (_, Some(i)) => i.saturating_sub(1),
                            (_, None) => 0,
                        });
                        self.scroll_into_view();
                    }
                    Reply::None
                }
                VK_RETURN => self.run_selected(),
                VK_ESCAPE => {
                    self.hide();
                    return Reply::None;
                }
                VK_BACK => {
                    self.edit(|chars, caret| {
                        if *caret > 0 {
                            *caret -= 1;
                            chars.remove(*caret);
                        }
                    });
                    Reply::None
                }
                VK_DELETE => {
                    self.edit(|chars, caret| {
                        if *caret < chars.len() {
                            chars.remove(*caret);
                        }
                    });
                    Reply::None
                }
                VK_LEFT => {
                    self.edit(|_, caret| *caret = caret.saturating_sub(1));
                    Reply::None
                }
                VK_RIGHT => {
                    self.edit(|chars, caret| *caret = (*caret + 1).min(chars.len()));
                    Reply::None
                }
                VK_HOME => {
                    self.edit(|_, caret| *caret = 0);
                    Reply::None
                }
                VK_END => {
                    self.edit(|chars, caret| *caret = chars.len());
                    Reply::None
                }
                _ => return Reply::None,
            },
            Event::MouseMove(x, y) => {
                let hover = self.item_at(x, y);
                if hover == self.hover {
                    return Reply::None;
                }
                self.hover = hover;
                Reply::None
            }
            Event::MouseLeave => {
                self.hover = None;
                Reply::None
            }
            Event::MouseDown(Button::Left, x, y, _) => {
                if let Some(i) = self.item_at(x, y) {
                    self.selected = Some(i);
                }
                Reply::None
            }
            Event::DoubleClick(x, y) => match self.item_at(x, y) {
                Some(i) => {
                    self.selected = Some(i);
                    self.run_selected()
                }
                None => Reply::None,
            },
            Event::Wheel(delta, _, _) => {
                let step = self.item_height() * 3.0 * f32::from(delta) / 120.0;
                self.scroll = (self.scroll - step).clamp(0.0, self.max_scroll());
                Reply::None
            }
            Event::DpiChanged => {
                self.scale = self.window.scale();
                Reply::None
            }
            _ => return Reply::None,
        };
        if self.visible {
            self.paint();
        }
        reply
    }

    /// Draws the palette with `entries` without showing it.
    pub fn render(&mut self, entries: Vec<Entry>, query: &str) -> Option<(u32, u32, Vec<u8>)> {
        self.entries = entries;
        self.query = query.into();
        self.caret = self.query.chars().count();
        self.filter();
        if query.is_empty() {
            self.selected = None;
        }
        self.scale = wnd::monitor_at(0, 0).scale;
        self.surface = Some(Surface::new(self.gfx.clone()).ok()?);
        self.draw();
        let image = self.surface.as_ref()?.rgba();
        self.surface = None;
        image
    }

    fn paint(&mut self) {
        self.draw();
        if let Some(surface) = &self.surface {
            surface.present(self.window.raw(), self.position.0, self.position.1, 1.0);
        }
    }

    fn draw(&mut self) {
        let height = self.height();
        let pixels = (
            (WIDTH * self.scale).round() as i32,
            (height * self.scale).round() as i32,
        );
        let Some(mut surface) = self.surface.take() else {
            return;
        };
        let white = Rgba::WHITE;
        let box_h = self.box_height();
        let item_h = self.item_height();
        let list_top = self.list_top();
        let list_h = self.list_height();
        let caret_x = {
            let before: String = self.query.chars().take(self.caret).collect();
            self.gfx.measure(&before, &search_style()).0
        };
        let rows: Vec<(usize, &Entry)> = self
            .shown
            .iter()
            .enumerate()
            .map(|(row, &i)| (row, &self.entries[i]))
            .collect();
        let (query, selected, hover, scroll, caret_on) = (
            &self.query,
            self.selected,
            self.hover,
            self.scroll,
            self.caret_on && self.visible,
        );
        surface.draw(pixels.0, pixels.1, self.scale, |canvas| {
            let outer = Rect::new(0.0, 0.0, WIDTH, height);
            canvas.round_rect(outer, 8.0, Rgba::rgba(0x20, 0x20, 0x20, 230.0 / 255.0));
            canvas.stroke_round_rect(
                outer.inset(0.5, 0.5),
                7.5,
                1.0,
                Rgba::rgba(255, 255, 255, 0x55 as f32 / 255.0),
            );
            // Search box.
            let search = Rect::new(12.0, 12.0, WIDTH - 24.0, box_h);
            canvas.rect(search, Rgba::rgba(255, 255, 255, 0x33 as f32 / 255.0));
            let text = search.inset(8.0, 6.0);
            canvas.push_layer(1.0, Some((search, 0.0)));
            canvas.text(query, text, &search_style(), white);
            if caret_on {
                canvas.rect(Rect::new(text.x + caret_x, text.y, 1.0, text.h), white);
            }
            canvas.pop();
            // Separator.
            canvas.rect(
                Rect::new(12.0, 12.0 + box_h + 6.0, WIDTH - 24.0, 1.0),
                Rgba::rgba(255, 255, 255, 0x33 as f32 / 255.0),
            );
            // Results.
            let list = Rect::new(4.0, list_top, WIDTH - 8.0, list_h);
            canvas.push_layer(1.0, Some((list, 0.0)));
            for (row, entry) in rows {
                let y = list_top + row as f32 * item_h - scroll;
                if y + item_h < list_top || y > list_top + list_h {
                    continue;
                }
                let item = Rect::new(4.0, y, WIDTH - 8.0, item_h);
                if selected == Some(row) {
                    canvas.rect(item, Rgba::rgba(255, 255, 255, 0x3c as f32 / 255.0));
                } else if hover == Some(row) {
                    canvas.rect(item, Rgba::rgba(255, 255, 255, 0x22 as f32 / 255.0));
                }
                let content = item.inset(ITEM_PAD_X, 0.0);
                let (name_w, _) = canvas.gfx().measure(&entry.name, &name_style());
                canvas.text(&entry.name, content, &name_style().ellipsis(), white);
                if !entry.shortcut.is_empty() {
                    let rest = Rect::new(
                        content.x + name_w + 12.0,
                        content.y,
                        (content.w - name_w - 12.0).max(0.0),
                        content.h,
                    );
                    canvas.text(
                        &entry.shortcut,
                        rest,
                        &shortcut_style().align(Align::Leading),
                        Rgba::rgba(255, 255, 255, 0x88 as f32 / 255.0),
                    );
                }
            }
            canvas.pop();
        });
        self.surface = Some(surface);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fuzzy_matching_follows_the_original() {
        let entry = Entry::new(
            "Open Playback Devices",
            "",
            &["playback", "output", "speakers"],
            Command::ControlPanel("playback"),
        );
        assert!(matches(&entry, "playback"));
        assert!(matches(&entry, "speak"));
        assert!(matches(&entry, "opd"));
        assert!(!matches(&entry, "xyz"));
        assert!(!matches(&entry, "dvo"));
    }
}
