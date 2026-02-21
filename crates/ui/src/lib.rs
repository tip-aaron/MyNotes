use fltk::{
    app,
    enums::{Color, Font},
    prelude::*,
    text::{TextBuffer, TextEditor},
    window::Window,
};

/// Runs the MyNotes editor window.
///
/// Creates a lightweight FLTK window containing a text editor widget.
/// When `content` is provided the editor is pre-populated with that text;
/// otherwise it starts empty.
///
/// Blocks until the user closes the window.  Designed for very fast startup
/// (FLTK is statically linked and requires no Pango/image subsystems).
pub fn run(content: Option<&str>) {
    let app = app::App::default();

    let mut win = Window::new(0, 0, 900, 650, "MyNotes");
    win.set_color(Color::from_rgb(30, 30, 30));

    let mut editor = TextEditor::new(4, 4, 892, 642, "");
    let mut buf = TextBuffer::default();

    if let Some(text) = content {
        buf.set_text(text);
    }

    editor.set_buffer(buf);
    editor.set_text_font(Font::Courier);
    editor.set_text_size(15);
    editor.set_text_color(Color::from_rgb(220, 220, 220));
    editor.set_color(Color::from_rgb(30, 30, 30));
    editor.set_cursor_color(Color::from_rgb(200, 200, 200));
    editor.set_linenumber_width(50);
    editor.set_linenumber_size(13);
    editor.set_linenumber_fgcolor(Color::from_rgb(120, 120, 120));

    win.end();
    win.show();
    app.run().ok();
}
