use fltk::prelude::*;
use std::cell::RefCell;
use std::convert::TryInto;
use std::rc::Rc;

// ==========================================
// UTILS
// ==========================================
#[inline(always)]
fn as_usize(val: u64) -> usize {
    val.try_into().expect("future error handling")
}

// ==========================================
// 1. STATE
// ==========================================
pub struct State {
    pub doc: Rc<RefCell<editor_state::document::Document>>,
    pub cursor_visible: bool,
    pub scroll_offset: usize,
    pub scrolloff: usize,
    pub last_interaction: std::time::Instant,
}

// ==========================================
// 2. MAIN COMPONENT API
// ==========================================
pub struct TextEditor {
    pub group: fltk::group::Group,
    pub canvas: fltk::widget::Widget,
    pub scrollbar: fltk::valuator::Scrollbar,
    pub state: Rc<RefCell<State>>,
    pub line_height: i32,
}

impl TextEditor {
    pub fn new(
        x: i32,
        y: i32,
        w: i32,
        h: i32,
        doc: Rc<RefCell<editor_state::document::Document>>,
    ) -> Self {
        let grp = fltk::group::Group::default().with_pos(x, y).with_size(w, h);

        let state = Rc::new(RefCell::new(State {
            doc,
            cursor_visible: false,
            scroll_offset: 0,
            scrolloff: 5,
            last_interaction: std::time::Instant::now(),
        }));

        let line_height = 16;

        let mut canvas = fltk::widget::Widget::default()
            .with_pos(x, y)
            .with_size(w - 15, h);
        let mut scrollbar = fltk::valuator::Scrollbar::default()
            .with_pos(x + w - 15, y)
            .with_size(15, h);

        scrollbar.set_type(fltk::valuator::ScrollbarType::VerticalNice);
        scrollbar.set_color(fltk::enums::Color::from_rgb(200, 200, 200));
        scrollbar.set_selection_color(fltk::enums::Color::from_rgb(100, 100, 100));
        scrollbar.set_step(0.5, 1);

        grp.resizable(&canvas);
        grp.end();

        LayoutSync::apply_to_scrollbar(
            &mut state.borrow_mut(),
            &mut scrollbar,
            canvas.height(),
            line_height,
        );
        Renderer::wire(&mut canvas, state.clone(), line_height);
        Controller::wire(&mut canvas, &mut scrollbar, state.clone(), line_height);

        Self {
            group: grp,
            canvas,
            scrollbar,
            state,
            line_height,
        }
    }

    pub fn on_content_changed(&mut self) {
        LayoutSync::apply_to_scrollbar(
            &mut self.state.borrow_mut(),
            &mut self.scrollbar,
            self.canvas.height(),
            self.line_height,
        );
        self.canvas.redraw();
    }
}

// ==========================================
// 3. LAYOUT & SCROLL MATH
// ==========================================
struct LayoutSync;

impl LayoutSync {
    fn apply_to_scrollbar(
        state: &mut State,
        scrollbar: &mut fltk::valuator::Scrollbar,
        canvas_h: i32,
        line_h: i32,
    ) {
        let doc_lines = state.doc.borrow().get_line_count();
        let visible_lines = (canvas_h / line_h).max(1) as usize;
        let max_scroll = doc_lines.saturating_sub(visible_lines);

        state.scroll_offset = state.scroll_offset.clamp(0, max_scroll);
        scrollbar.set_bounds(0.0, max_scroll as f64);
        scrollbar.set_slider_size((visible_lines as f32 / doc_lines.max(1) as f32).clamp(0.0, 1.0));
        scrollbar.set_value(state.scroll_offset as f64);
    }

    fn sync_view_to_cursor(state: &mut State, canvas_h: i32, line_h: i32) {
        let visible_lines = (canvas_h / line_h).max(1) as usize;
        let actual_scrolloff = state.scrolloff.min(visible_lines.saturating_sub(1) / 2);
        let head_row = state.doc.borrow().cursor.head.row;
        let top = state.scroll_offset + actual_scrolloff;
        let bottom = state.scroll_offset + visible_lines.saturating_sub(1) - actual_scrolloff;

        if head_row < top {
            state.scroll_offset = head_row.saturating_sub(actual_scrolloff);
        } else if head_row > bottom {
            state.scroll_offset = head_row + actual_scrolloff + 1 - visible_lines;
        }
    }

    fn sync_cursor_to_view(state: &mut State, canvas_h: i32, line_h: i32) {
        let visible_lines = (canvas_h / line_h).max(1) as usize;
        let actual_scrolloff = state.scrolloff.min(visible_lines.saturating_sub(1) / 2);

        let top = state.scroll_offset + actual_scrolloff;
        let bottom = state.scroll_offset + visible_lines.saturating_sub(1) - actual_scrolloff;

        let mut d = state.doc.borrow_mut();
        let total_lines = d.get_line_count();
        let mut r = d.cursor.head.row;

        while r < top && r + 1 < total_lines {
            // FIX: Extract length calculation to avoid simultaneous mutable & immutable borrows
            let target_len = as_usize(d.get_visible_line_len_at(r + 1).unwrap_or(0));
            let is_last = r + 2 >= total_lines;
            d.cursor.move_down(target_len, is_last, false);
            r = d.cursor.head.row;
        }

        while r > bottom && r > 0 {
            // FIX: Extract length calculation
            let target_len = as_usize(d.get_visible_line_len_at(r - 1).unwrap_or(0));
            d.cursor.move_up(target_len, false);
            r = d.cursor.head.row;
        }
    }
}

// ==========================================
// 4. RENDERER (View)
// ==========================================
struct Renderer;

impl Renderer {
    const FONT_SIZE: i32 = 16;
    const LEFT_PAD: i32 = 6;
    const MARGIN_W: i32 = 45;

    fn wire(canvas: &mut fltk::widget::Widget, state: Rc<RefCell<State>>, line_h: i32) {
        canvas.draw({
            let state = state.clone();
            move |w| {
                // 1. Lock drawing strictly to the canvas dimensions!
                // This prevents text from bleeding into the scrollbar area.
                let be = state.borrow();
                let d = be.doc.borrow();

                Self::draw_bg(w);
                Self::draw_selection(w, &be, &d, line_h);
                Self::draw_text(w, &be, &d, line_h);
                Self::draw_cursor(w, &be, &d, line_h);
            }
        });

        let mut t_canvas = canvas.clone();
        fltk::app::add_timeout3(0.5, move |handle| {
            let mut be = state.borrow_mut();
            if be.last_interaction.elapsed().as_millis() >= 500 {
                be.cursor_visible = !be.cursor_visible;
                t_canvas.redraw();
            } else {
                be.cursor_visible = true;
            }
            fltk::app::repeat_timeout3(0.5, handle);
        });
    }

    fn draw_bg(w: &mut fltk::widget::Widget) {
        fltk::draw::draw_rect_fill(
            w.x(),
            w.y(),
            w.width(),
            w.height(),
            fltk::enums::Color::from_rgb(40, 44, 52),
        );
    }

    fn draw_selection(
        w: &mut fltk::widget::Widget,
        be: &State,
        d: &editor_state::document::Document,
        line_h: i32,
    ) {
        let (start, end) = d.cursor.range();

        if start == end {
            return;
        }

        fltk::draw::set_font(fltk::enums::Font::Courier, Self::FONT_SIZE);

        let char_w = fltk::draw::width("a") as i32;
        let base_x = w.x() + Self::MARGIN_W + Self::LEFT_PAD;
        // Define the color once
        let selection_color = fltk::enums::Color::from_rgb(62, 68, 81);

        for i in start.row..=end.row {
            if i < be.scroll_offset || i > be.scroll_offset + (w.height() / line_h) as usize + 1 {
                continue;
            }

            let y = w.y() + ((i - be.scroll_offset) as i32 * line_h);

            let start_col = if i == start.row { start.col as i32 } else { 0 };
            let end_col = if i == end.row {
                end.col as i32
            } else {
                as_usize(d.get_visible_line_len_at(i).unwrap_or(0)) as i32 + 1
            };

            let rect_x = base_x + (start_col * char_w);
            let rect_w = (end_col - start_col) * char_w;

            // Pass the color directly as the 5th argument
            fltk::draw::draw_rect_fill(rect_x, y, rect_w, line_h, selection_color);
        }
    }

    fn draw_text(
        w: &mut fltk::widget::Widget,
        be: &State,
        d: &editor_state::document::Document,
        line_h: i32,
    ) {
        fltk::draw::set_font(fltk::enums::Font::Courier, Self::FONT_SIZE);
        let end = std::cmp::min(
            d.get_line_count(),
            be.scroll_offset + (w.height() / line_h) as usize + 1,
        );

        for i in be.scroll_offset..end {
            if let Some(text) = d.get_line_stripped(i) {
                let y = w.y() + ((i - be.scroll_offset) as i32 * line_h);
                fltk::draw::set_draw_color(fltk::enums::Color::from_rgb(120, 120, 120));
                fltk::draw::draw_text2(
                    &format!("{:3}", i + 1),
                    w.x(),
                    y,
                    Self::MARGIN_W - 5,
                    line_h,
                    fltk::enums::Align::RightTop,
                );
                fltk::draw::set_draw_color(fltk::enums::Color::White);
                fltk::draw::draw_text2(
                    &text,
                    w.x() + Self::MARGIN_W + Self::LEFT_PAD,
                    y,
                    w.width() - Self::MARGIN_W,
                    line_h,
                    fltk::enums::Align::Left,
                );
            }
        }
    }

    fn draw_cursor(
        w: &mut fltk::widget::Widget,
        be: &State,
        d: &editor_state::document::Document,
        line_h: i32,
    ) {
        if !be.cursor_visible {
            return;
        }
        let head = d.cursor.head;

        if head.row >= be.scroll_offset
            && head.row <= be.scroll_offset + (w.height() / line_h) as usize
        {
            let x = w.x()
                + Self::MARGIN_W
                + Self::LEFT_PAD
                + (head.col as i32 * fltk::draw::width("a") as i32);
            let y = w.y() + ((head.row - be.scroll_offset) as i32 * line_h);

            fltk::draw::draw_rect_fill(
                x,
                y + (line_h - fltk::draw::height()) / 2,
                2,
                fltk::draw::height(),
                fltk::enums::Color::White,
            );
        }
    }
}

// ==========================================
// 5. CONTROLLER (Input & Events)
// ==========================================
struct Controller;

impl Controller {
    fn wire(
        canvas: &mut fltk::widget::Widget,
        sb: &mut fltk::valuator::Scrollbar,
        state: Rc<RefCell<State>>,
        lh: i32,
    ) {
        sb.set_callback({
            let state = state.clone();
            let mut c = canvas.clone();
            let mut sbc = sb.clone();
            move |s| {
                state.borrow_mut().scroll_offset = s.value() as usize;
                Self::refresh_view(&mut state.borrow_mut(), &mut c, &mut sbc, lh);
            }
        });

        let st = state.clone();
        let mut handle_sb = sb.clone();

        canvas.handle(move |c, ev| match ev {
            fltk::enums::Event::Enter => {
                if let Some(mut w) = c.window() {
                    w.set_cursor(fltk::enums::Cursor::Insert);
                }
                true
            }
            fltk::enums::Event::Leave => {
                if let Some(mut w) = c.window() {
                    w.set_cursor(fltk::enums::Cursor::Default);
                }
                true
            }
            fltk::enums::Event::MouseWheel => {
                Self::on_mouse_wheel(c, &mut st.borrow_mut(), &mut handle_sb, lh)
            }
            fltk::enums::Event::Resize => {
                Self::on_resize(c, &mut st.borrow_mut(), &mut handle_sb, lh)
            }
            fltk::enums::Event::Push => Self::on_push(c, &mut st.borrow_mut(), &mut handle_sb, lh),
            fltk::enums::Event::Drag => Self::on_drag(c, &mut st.borrow_mut(), &mut handle_sb, lh),
            fltk::enums::Event::Shortcut => {
                let event_key = fltk::app::event_key();
                println!("{:#?}", event_key);

                if event_key == fltk::enums::Key::from_char('v') {
                    fltk::app::paste(c);
                } else if event_key == fltk::enums::Key::from_char('c') {
                    return Self::on_copy(c, &mut st.borrow_mut(), &mut handle_sb, lh);
                } else if event_key == fltk::enums::Key::from_char('x') {
                    return Self::on_cut(c, &mut st.borrow_mut(), &mut handle_sb, lh);
                }

                true
            }
            fltk::enums::Event::Paste => {
                Self::on_paste(c, &mut st.borrow_mut(), &mut handle_sb, lh)
            }
            fltk::enums::Event::KeyDown => {
                Self::on_keydown(c, &mut st.borrow_mut(), &mut handle_sb, lh)
            }
            fltk::enums::Event::Focus | fltk::enums::Event::Unfocus => true,
            _ => false,
        });
    }

    // --- Utility Input Math ---

    fn mouse_to_pos(c: &fltk::widget::Widget, be: &State, lh: i32) -> (usize, usize) {
        fltk::draw::set_font(fltk::enums::Font::Courier, Renderer::FONT_SIZE);
        let row = be.scroll_offset + ((fltk::app::event_y() - c.y()) / lh).max(0) as usize;
        let rel_x = fltk::app::event_x() - (c.x() + Renderer::MARGIN_W + Renderer::LEFT_PAD);
        let col = if rel_x < 0 {
            0
        } else {
            (rel_x / fltk::draw::width("a") as i32) as usize
        };

        let d = be.doc.borrow();
        let max_row = d.get_line_count().saturating_sub(1);
        let t_row = row.min(max_row);
        let line_len = as_usize(d.get_visible_line_len_at(t_row).unwrap_or(0));
        let t_col = col.min(line_len);

        (t_row, t_col)
    }

    // --- Event Handlers ---

    fn on_mouse_wheel(
        c: &mut fltk::widget::Widget,
        be: &mut State,
        sb: &mut fltk::valuator::Scrollbar,
        lh: i32,
    ) -> bool {
        let dy = fltk::app::event_dy_value();
        if dy == 0 {
            return false;
        }

        let old_off = be.scroll_offset;
        be.scroll_offset = (old_off as isize).saturating_add((dy * 3) as isize).max(0) as usize;

        if be.scroll_offset != old_off {
            // Only enforce scrolloff (moving the cursor to stay visible) if we are NOT selecting
            if !fltk::app::event_state().contains(fltk::enums::EventState::Button1) {
                LayoutSync::sync_cursor_to_view(be, c.height(), lh);
            }

            LayoutSync::apply_to_scrollbar(be, sb, c.height(), lh);
            c.redraw();
            sb.redraw();

            be.last_interaction = std::time::Instant::now();
        }
        true
    }

    fn on_resize(
        c: &mut fltk::widget::Widget,
        be: &mut State,
        sb: &mut fltk::valuator::Scrollbar,
        lh: i32,
    ) -> bool {
        LayoutSync::sync_view_to_cursor(be, c.height(), lh);
        LayoutSync::apply_to_scrollbar(be, sb, c.height(), lh);
        false
    }

    fn on_push(
        c: &mut fltk::widget::Widget,
        be: &mut State,
        sb: &mut fltk::valuator::Scrollbar,
        lh: i32,
    ) -> bool {
        c.take_focus().unwrap();
        let (row, col) = Self::mouse_to_pos(c, be, lh);

        let mut d = be.doc.borrow_mut();
        d.cursor.head.row = row;
        d.cursor.head.col = col;
        d.cursor.anchor.row = row;
        d.cursor.anchor.col = col;
        drop(d);

        Self::refresh_cursor(be, c, sb, lh)
    }

    fn on_drag(
        c: &mut fltk::widget::Widget,
        be: &mut State,
        sb: &mut fltk::valuator::Scrollbar,
        lh: i32,
    ) -> bool {
        let (row, col) = Self::mouse_to_pos(c, be, lh);

        let mut d = be.doc.borrow_mut();
        d.cursor.head.row = row;
        d.cursor.head.col = col;
        drop(d);

        Self::refresh_cursor(be, c, sb, lh)
    }

    fn on_keydown(
        c: &mut fltk::widget::Widget,
        be: &mut State,
        sb: &mut fltk::valuator::Scrollbar,
        lh: i32,
    ) -> bool {
        let key = fltk::app::event_key();
        let shift = fltk::app::event_state().contains(fltk::enums::EventState::Shift);

        let d = be.doc.borrow_mut();
        let row = d.cursor.head.row;
        let is_last = row + 1 >= d.get_line_count();

        drop(d);

        let handled = match key {
            fltk::enums::Key::Up if row > 0 => {
                let mut d = be.doc.borrow_mut();
                // FIX: Extract length
                let prev_len = as_usize(d.get_visible_line_len_at(row - 1).unwrap_or(0));
                d.cursor.move_up(prev_len, shift);
                true
            }
            fltk::enums::Key::Down if !is_last => {
                let mut d = be.doc.borrow_mut();
                // FIX: Extract length
                let next_len = as_usize(d.get_visible_line_len_at(row + 1).unwrap_or(0));
                d.cursor.move_down(next_len, is_last, shift);
                true
            }
            fltk::enums::Key::Left => {
                let mut d = be.doc.borrow_mut();
                // FIX: Extract length
                let prev_len = if row > 0 && d.cursor.head.col == 0 {
                    as_usize(d.get_visible_line_len_at(row - 1).unwrap_or(0))
                } else {
                    0
                };
                d.cursor.move_left(prev_len, shift);
                true
            }
            fltk::enums::Key::Right => {
                let mut d = be.doc.borrow_mut();
                // FIX: Extract length
                let curr_len = as_usize(d.get_visible_line_len_at(row).unwrap_or(0));
                d.cursor.move_right(curr_len, is_last, shift);
                true
            }
            fltk::enums::Key::BackSpace => {
                let mut d = be.doc.borrow_mut();
                d.delete(true);

                true
            }
            fltk::enums::Key::Delete => {
                let mut d = be.doc.borrow_mut();
                d.delete(false);

                true
            }
            fltk::enums::Key::Enter => {
                let mut d = be.doc.borrow_mut();
                d.insert("\n");

                true
            }
            fltk::enums::Key::Tab => {
                let mut d = be.doc.borrow_mut();
                d.insert("\t");

                true
            }
            _ => false,
        };

        if !handled {
            let text = fltk::app::event_text();
            if !text.is_empty() && !text.chars().any(|c| c.is_control()) {
                println!("Typed: {}", text); // TODO: Insert text

                let mut d = be.doc.borrow_mut();

                d.insert(&text);

                drop(d);

                return Self::refresh_cursor(be, c, sb, lh);
            }
            return false;
        }

        Self::refresh_cursor(be, c, sb, lh)
    }

    fn on_paste(
        c: &mut fltk::widget::Widget,
        be: &mut State,
        sb: &mut fltk::valuator::Scrollbar,
        lh: i32,
    ) -> bool {
        let text = fltk::app::event_text();
        println!("Pasted: {}", text);

        println!("Pasted: {}", text);

        if text.is_empty() {
            return false;
        }

        let mut d = be.doc.borrow_mut();

        d.insert(&text);

        drop(d);

        Self::refresh_view(be, c, sb, lh);

        true
    }

    pub fn on_copy(
        c: &mut fltk::widget::Widget,
        be: &mut State,
        sb: &mut fltk::valuator::Scrollbar,
        lh: i32,
    ) -> bool {
        let ctrl = fltk::app::event_state().contains(fltk::enums::EventState::Ctrl);
        let text = fltk::app::event_text();
        let d = be.doc.borrow();
        let selected = d.get_selected_text();

        drop(d);

        if !selected.is_empty() {
            fltk::app::copy(&selected);

            return true;
        }

        false
    }

    pub fn on_cut(
        c: &mut fltk::widget::Widget,
        be: &mut State,
        sb: &mut fltk::valuator::Scrollbar,
        lh: i32,
    ) -> bool {
        // ---- 1. READ selection (immutable borrow) ----
        let mut d = be.doc.borrow_mut();
        let selected = d.get_selected_text();

        if selected.is_empty() {
            return true;
        }

        fltk::app::copy(&selected);
        d.delete(true);

        true
    }

    // --- UI Refresh Helpers ---

    fn refresh_view(
        be: &mut State,
        c: &mut fltk::widget::Widget,
        sb: &mut fltk::valuator::Scrollbar,
        lh: i32,
    ) {
        be.cursor_visible = true;
        be.last_interaction = std::time::Instant::now();
        LayoutSync::sync_cursor_to_view(be, c.height(), lh);
        LayoutSync::apply_to_scrollbar(be, sb, c.height(), lh);
        c.redraw();
        sb.redraw();
    }

    fn refresh_cursor(
        be: &mut State,
        c: &mut fltk::widget::Widget,
        sb: &mut fltk::valuator::Scrollbar,
        lh: i32,
    ) -> bool {
        be.cursor_visible = true;
        be.last_interaction = std::time::Instant::now();
        LayoutSync::sync_view_to_cursor(be, c.height(), lh);
        LayoutSync::apply_to_scrollbar(be, sb, c.height(), lh);
        c.redraw();
        true
    }
}
