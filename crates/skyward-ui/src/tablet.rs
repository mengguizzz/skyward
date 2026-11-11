//! Tablet shell. Pages keep their own coordinate space and navigation rules;
//! only the shell translates panel touches and rotates the final image.
use super::*;
use crate::icons::{self, Icon};

pub(super) struct Tablet {
    physical: (u32, u32),
    pub(super) enabled: bool,
    rotation: u8,
    origin: (u32, u32),
    sidebar: u32,
    scroll: f32,
    gesture: Option<(f32, f32, bool)>,
    page_touch: bool,
    toolbar_touch: bool,
    page: Option<Canvas>,
    logical: Option<Canvas>,
    home_hits: Vec<Hit>,
    home_height: u32,
    home_x: f32,
}

impl Tablet {
    pub fn new(w: u32, h: u32, density: f32) -> Self {
        Self {
            physical: (w, h),
            enabled: w.min(h) as f32 / density >= 600.0,
            rotation: 0,
            origin: (0, 0),
            sidebar: 0,
            scroll: 0.0,
            gesture: None,
            page_touch: false,
            toolbar_touch: false,
            page: None,
            logical: None,
            home_hits: Vec::new(),
            home_height: 0,
            home_x: 0.0,
        }
    }

    fn size(&self, rotation: u8) -> (u32, u32) {
        let (w, h) = self.physical;
        if rotation % 2 == 0 { (w, h) } else { (h, w) }
    }

    fn point(&self, x: f32, y: f32) -> (f32, f32) {
        let (w, h) = self.physical;
        match self.rotation {
            1 => (h as f32 - 1.0 - y, x),
            2 => (w as f32 - 1.0 - x, h as f32 - 1.0 - y),
            3 => (y, w as f32 - 1.0 - x),
            _ => (x, y),
        }
    }
}

impl App {
    fn tablet_layout(&mut self) {
        if !self.tablet.enabled {
            return;
        }
        let rotation = self.state.rotation.min(3);
        let (width, height) = self.tablet.size(rotation);
        let header = (44.0 * self.px_per_dp).round() as u32;
        let can_navigate = self.stack.iter().any(|s| s.0 == Screen::Home);
        let sidebar = if width > height && can_navigate {
            (380.0 * self.px_per_dp).min(width as f32 * 0.38).round() as u32
        } else {
            0
        };
        let (w, h) = (width - sidebar, height - header);
        if self.w != w || self.h != h || self.tablet.rotation != rotation || self.tablet.sidebar != sidebar {
            self.w = w;
            self.h = h;
            self.tablet.rotation = rotation;
            self.tablet.sidebar = sidebar;
            self.tablet.origin = (sidebar, header);
            self.tablet.page = None;
            self.tablet.logical = None;
            self.tablet.home_hits.clear();
            self.tablet.gesture = None;
            self.tablet.page_touch = false;
            self.tablet.toolbar_touch = false;
            self.drag = Drag::Idle;
            self.tracker = Tracker::default();
            self.state.confirm = 0.0;
            self.end_turn();
            self.backdrop = None;
            self.morph = None;
            self.cue = None;
            self.chip = None;
            self.button_chip = None;
            self.search_words = None;
            self.charge_art = None;
            self.charge_canvas = None;
            self.dither = None;
            self.settle_page();
            self.refresh();
        }
    }

    /// Compose in logical tablet coordinates, then rotate back to the panel.
    pub fn compose(&mut self, out: &mut Canvas, f: &Fonts, info: &DeviceInfo, clock: &str, now: u64) {
        if !self.tablet.enabled {
            return self.compose_content(out, f, info, clock, now);
        }
        self.tablet_layout();
        let t = self.theme();
        let d = self.px_per_dp;
        let (width, height) = self.tablet.size(self.tablet.rotation);
        let (ox, oy) = self.tablet.origin;
        let mut page = self.tablet.page.take().unwrap_or_else(|| Canvas::new(self.w, self.h));
        if ox > 0 && self.screen() == Screen::Home && !self.charge_shown() {
            // Home already occupies the left pane. The right pane becomes
            // populated when an entry is selected, rather than duplicating it.
            page.clear(t.pal.bg);
        } else {
            self.compose_content(&mut page, f, info, clock, now);
        }
        let mut logical = self.tablet.logical.take().unwrap_or_else(|| Canvas::new(width, height));
        logical.clear(t.pal.bg);
        logical.lay(&page, ox as i32, oy as i32, 1.0);
        let mut header_theme = t;
        header_theme.m.w = width;
        header_theme.m.h = height;
        widgets::status_bar(&mut logical, &header_theme, f, &self.bar(clock, info), t.pal.status);
        let label = self.state.lang.pick(["旋转屏幕", "旋轉螢幕", "Rotate screen"]);
        let center = width as f32 / 2.0;
        logical.text(&f.body, label, center - 80.0 * d, 28.0 * d, 14.0 * d, t.pal.subtitle, Some(108.0 * d));
        icons::glyph(&mut logical, Icon::Rotate, center + 60.0 * d, 22.0 * d, 24.0 * d, t.pal.accent);
        if ox > 0 {
            // Render the very same Home page, with live device state and its
            // original coloured tiles, rather than maintaining a second menu.
            let mut home_theme = t;
            home_theme.m.w = ox;
            let cx = Ctx { t: &home_theme, f, info, state: &self.state, term: None, band: None };
            let mut home = pages::home(&cx);
            home.canvas.text_top(&f.title, home.title, home_theme.m.title_x,
                home_theme.m.title_top, home_theme.m.title_px, t.pal.text, None);
            if let Some(fold) = &home.fold {
                widgets::search_capsule(&mut home.canvas, &home_theme, f, fold.bar, fold.hint);
            }
            self.tablet.home_height = home.canvas.h;
            let max = home.canvas.h.saturating_sub(self.h) as f32;
            self.tablet.scroll = self.tablet.scroll.clamp(0.0, max);
            let mut nav = Canvas::filled(ox, self.h, t.pal.bg);
            nav.lay(&home.canvas, 0, -(self.tablet.scroll.round() as i32), 1.0);
            self.tablet.home_hits = home.hits;
            logical.lay(&nav, 0, oy as i32, 1.0);
            logical.fill_rect(ox as f32 - d, oy as f32, d, self.h as f32, t.pal.track);
        }
        // Content already follows the screen curtain; shell chrome follows it too.
        let level = self.curtain.level(now);
        if level < 1.0 {
            let top = logical.crop(0, 0, width, oy);
            let mut top = top;
            top.dim(level);
            logical.lay(&top, 0, 0, 1.0);
            if ox > 0 {
                let mut nav = logical.crop(0, oy, ox, self.h);
                nav.dim(level);
                logical.lay(&nav, 0, oy as i32, 1.0);
            }
        }
        rotate_into(&logical, out, self.tablet.rotation);
        self.tablet.page = Some(page);
        self.tablet.logical = Some(logical);
    }

    pub fn touch(&mut self, ev: Touch, f: &Fonts, now: u64) -> Vec<Effect> {
        if !self.tablet.enabled {
            return self.touch_content(ev, f, now);
        }
        self.tablet_layout();
        let mapped = match ev {
            Touch::Down(x, y) => {
                let (x, y) = self.tablet.point(x, y);
                Touch::Down(x, y)
            }
            Touch::Move(x, y) => {
                let (x, y) = self.tablet.point(x, y);
                Touch::Move(x, y)
            }
            Touch::Up => Touch::Up,
        };
        // Preserve wake/charging guards before allowing any shell action.
        if !self.display_on || self.charge_shown() || (matches!(mapped, Touch::Down(..)) && self.just_woke(now)) {
            self.tablet.page_touch = false;
            self.tablet.gesture = None;
            self.tablet.toolbar_touch = false;
            return self.touch_content(mapped, f, now);
        }
        let (ox, oy) = self.tablet.origin;
        let d = self.px_per_dp;
        let (width, _) = self.tablet.size(self.tablet.rotation);
        let rotate = |x: f32, y: f32| (x - width as f32 / 2.0).abs() <= 90.0 * d && y >= 0.0 && y < oy as f32;
        match mapped {
            Touch::Down(x, y) => {
                self.stir(now);
                self.tablet.home_x = x;
                self.tablet.toolbar_touch = rotate(x, y);
                self.tablet.page_touch = x >= ox as f32 && y >= oy as f32;
                self.tablet.gesture = if ox > 0 && x < ox as f32 && y >= oy as f32 {
                    Some((y, y, false))
                } else {
                    None
                };
            }
            Touch::Move(x, y) => {
                self.tablet.toolbar_touch &= rotate(x, y);
                if let Some((start, last, moved)) = self.tablet.gesture {
                    let moved = moved || (y - start).abs() > 8.0 * d || x >= ox as f32;
                    self.tablet.scroll = (self.tablet.scroll + last - y)
                        .clamp(0.0, self.tablet.home_height.saturating_sub(self.h) as f32);
                    self.tablet.gesture = Some((start, y, moved));
                    self.dirty = true;
                }
            }
            Touch::Up => {
                if std::mem::take(&mut self.tablet.toolbar_touch) {
                    self.state.rotation = (self.state.rotation + 1) % 4;
                    self.tablet_layout();
                    let (rows, cols) = self.term_size(f);
                    if let Some(term) = &mut self.term {
                        term.resize(rows, cols);
                        if term.running {
                            return vec![Effect::ResizeShell { rows, cols }];
                        }
                    }
                    return Vec::new();
                }
                if let Some((start, _, moved)) = self.tablet.gesture.take() {
                    if !moved {
                        let y = start - oy as f32 + self.tablet.scroll;
                        if let Some(Action::Open(screen)) = self.tablet.home_hits.iter()
                            .find(|hit| hit.rect.contains(self.tablet.home_x, y))
                            .map(|hit| hit.action) {
                            return self.tablet_navigate(screen, now);
                        }
                    }
                    return Vec::new();
                }
            }
        }
        if !self.tablet.page_touch {
            return Vec::new();
        }
        if ox > 0 && self.screen() == Screen::Home {
            self.tablet.page_touch = false;
            return Vec::new();
        }
        if matches!(mapped, Touch::Up) {
            self.tablet.page_touch = false;
        }
        let local = match mapped {
            Touch::Down(x, y) => Touch::Down(x - ox as f32, y - oy as f32),
            Touch::Move(x, y) => Touch::Move(x - ox as f32, y - oy as f32),
            Touch::Up => Touch::Up,
        };
        self.touch_content(local, f, now)
    }

    fn tablet_navigate(&mut self, screen: Screen, now: u64) -> Vec<Effect> {
        if self.screen() == screen {
            return Vec::new();
        }
        let mut effects = Vec::new();
        // Use normal Back semantics: writes in progress and unsaved edits retain
        // their existing guards; shell navigation never truncates the stack.
        while self.stack.len() > 1 {
            let before = self.stack.len();
            effects.extend(self.back(now));
            if self.stack.len() == before {
                return effects;
            }
        }
        self.end_turn();
        self.morph = None;
        if screen != Screen::Home {
            effects.extend(self.act(Action::Open(screen), now));
        }
        effects
    }
}

fn rotate_into(src: &Canvas, dst: &mut Canvas, rotation: u8) {
    assert_eq!(src.data.len(), dst.data.len());
    if rotation == 0 {
        dst.data.copy_from_slice(&src.data);
        return;
    }
    for y in 0..dst.h {
        for x in 0..dst.w {
            let (sx, sy) = match rotation {
                1 => (src.w - 1 - y, x),
                2 => (src.w - 1 - x, src.h - 1 - y),
                3 => (y, src.h - 1 - x),
                _ => unreachable!(),
            };
            let from = ((sy * src.w + sx) * 4) as usize;
            let to = ((y * dst.w + x) * 4) as usize;
            dst.data[to..to + 4].copy_from_slice(&src.data[from..from + 4]);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pixels_and_touch_coordinates_agree_in_all_four_directions() {
        for r in 0..4 {
            let mut tablet = Tablet::new(6, 10, 0.01);
            tablet.rotation = r;
            let (w, h) = tablet.size(r);
            let mut src = Canvas::new(w, h);
            for (i, px) in src.data.chunks_exact_mut(4).enumerate() {
                px.copy_from_slice(&[i as u8, 0, 0, 255]);
            }
            let mut dst = Canvas::new(6, 10);
            rotate_into(&src, &mut dst, r);
            for y in 0..10 {
                for x in 0..6 {
                    let (sx, sy) = tablet.point(x as f32, y as f32);
                    assert_eq!(dst.data[((y * 6 + x) * 4) as usize], src.data[((sy as u32 * w + sx as u32) * 4) as usize]);
                }
            }
        }
    }

    #[test]
    fn tablet_layout_changes_page_size_without_losing_navigation() {
        let mut app = App::new(Screen::Home, 1800, 2880, 2.0);
        app.tablet_layout();
        assert_eq!((app.w, app.h), (1800, 2792));
        app.push(Screen::Mount);
        app.state.rotation = 1;
        app.tablet_layout();
        assert_eq!((app.w, app.h), (2120, 1712));
        assert_eq!(app.screen(), Screen::Mount);
        assert_eq!(app.tablet.origin, (760, 88));
        app.state.rotation = 2;
        app.tablet_layout();
        assert_eq!(app.tablet.sidebar, 0);
        assert_eq!(app.screen(), Screen::Mount);
    }

    #[test]
    fn phones_keep_the_existing_viewport() {
        let mut app = App::new(Screen::Home, 1080, 2400, 3.0);
        app.state.rotation = 1;
        app.tablet_layout();
        assert_eq!((app.w, app.h), (1080, 2400));
        assert!(!app.tablet.enabled);
    }

    #[test]
    fn initial_unlock_does_not_offer_sidebar_navigation() {
        let mut app = App::new(Screen::Decrypt, 1800, 2880, 2.0);
        app.state.rotation = 1;
        app.tablet_layout();
        assert_eq!(app.tablet.sidebar, 0);
    }

    #[test]
    fn sidebar_cannot_leave_an_install_while_opening_the_package() {
        let mut app = App::new(Screen::Home, 1800, 2880, 2.5);
        app.push(Screen::Installing);
        app.state.install = Install::Opening;
        let effects = app.tablet_navigate(Screen::Mount, 1000);
        assert!(effects.is_empty());
        assert_eq!(app.screen(), Screen::Installing);
    }

    #[test]
    fn sidebar_uses_the_normal_page_entry_effects() {
        let mut app = App::new(Screen::Home, 1800, 2880, 2.5);
        app.push(Screen::Settings);
        let effects = app.tablet_navigate(Screen::Mount, 1000);
        assert_eq!(app.screen(), Screen::Mount);
        assert_eq!(app.stack.len(), 2);
        assert!(!effects.is_empty(), "mounts must be refreshed through arrive_at");
    }

    #[test]
    fn home_pane_keeps_colours_and_nested_navigation_in_the_detail_pane() {
        let f = crate::font::bundled();
        let info = DeviceInfo {
            model: "Xiaomi Pad 6".into(), codename: "pipa".into(), soc: None,
            cpu: skyward_info::Cpu { cores: 8, max_khz: None }, ram_kib: 8_000_000,
            panel: Some((1800, 2880)), screen: Some(11.0), os_version: None,
            android: None, locale: None, security_patch: None, kernel: "test".into(),
            storage: skyward_info::Storage { total: 128_000_000_000, free: None },
            battery: None, slot: None, snapshot: skyward_vab::SnapshotState::None,
            boot: skyward_info::Boot { unlocked: None, verified: None },
        };
        let mut app = App::new(Screen::Home, 1800, 2880, 2.5);
        app.state.rotation = 1;
        let mut out = Canvas::new(1800, 2880);
        app.compose(&mut out, &f, &info, "12:00", 1000);
        assert!(app.tablet.home_hits.iter().any(|hit| hit.action == Action::Open(Screen::Search)));
        let hit = app.tablet.home_hits.iter()
            .find(|hit| hit.action == Action::Open(Screen::Settings)).unwrap().rect;
        app.tablet.scroll = (hit.y + hit.h - app.h as f32 + 24.0).max(0.0);
        app.compose(&mut out, &f, &info, "12:00", 1100);
        let x = hit.x + hit.w / 2.0;
        let y = app.tablet.origin.1 as f32 + hit.y + hit.h / 2.0 - app.tablet.scroll;
        app.touch(Touch::Down(y, 2879.0 - x), &f, 1200);
        app.touch(Touch::Up, &f, 1250);
        assert_eq!(app.screen(), Screen::Settings);
        app.act(Action::Open(Screen::Language), 1300);
        assert_eq!(app.screen(), Screen::Language);
        assert_eq!(app.stack.len(), 3);
        app.back(1400);
        assert_eq!(app.screen(), Screen::Settings);
        assert!(app.tablet.scroll > 0.0, "detail navigation must preserve Home scroll");
        app.compose(&mut out, &f, &info, "12:00", 1600);
        let nav = app.tablet.logical.as_ref().unwrap()
            .crop(0, app.tablet.origin.1, app.tablet.sidebar, app.h);
        assert!(nav.data.chunks_exact(4).any(|p| p[1] > 140 && p[0] < 80 && p[2] < 120),
            "Home must retain its coloured install icon");
    }

    #[test]
    fn rotation_button_is_reachable_in_each_orientation_and_cancels_confirmation() {
        let f = crate::font::bundled();
        let mut app = App::new(Screen::Home, 1800, 2880, 2.5);
        for r in 0..4 {
            app.tablet_layout();
            let (width, _) = app.tablet.size(r);
            let (x, y) = (width as f32 / 2.0, 55.0);
            let (px, py) = match r {
                1 => (y, 2879.0 - x),
                2 => (1799.0 - x, 2879.0 - y),
                3 => (1799.0 - y, x),
                _ => (x, y),
            };
            app.state.confirm = 0.8;
            assert!(app.touch(Touch::Down(px, py), &f, 1000).is_empty());
            assert!(app.touch(Touch::Up, &f, 1050).is_empty());
            assert_eq!(app.state.rotation, (r + 1) % 4);
            assert_eq!(app.state.confirm, 0.0);
            assert_eq!(app.screen(), Screen::Home);
        }
    }

    #[test]
    fn rotating_a_running_terminal_requests_the_new_pty_size() {
        let f = crate::font::bundled();
        let mut app = App::new(Screen::Home, 1800, 2880, 2.5);
        app.push(Screen::Terminal);
        app.tablet_layout();
        let before = app.term_size(&f);
        app.term_started(before.0, before.1);
        app.touch(Touch::Down(900.0, 55.0), &f, 1000);
        let effects = app.touch(Touch::Up, &f, 1050);
        let (rows, cols) = app.term_size(&f);
        assert_ne!(before, (rows, cols));
        assert_eq!(effects, vec![Effect::ResizeShell { rows, cols }]);
        assert_eq!(app.screen(), Screen::Terminal);
    }
}
