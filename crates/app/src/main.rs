use fltk::prelude::{GroupExt, MenuExt, WidgetExt};

pub fn main() {
    let app = fltk::app::App::default();
    let mut win = fltk::window::Window::default()
        .with_size(400, 300)
        .with_label("MyNotes");
    let backend = std::rc::Rc::new(std::cell::RefCell::new(
        editor_state::document::Document::new(editor_core::text::TextBuffer::new().unwrap()),
    ));
    let mut text_editor = ui::TextEditor::new(0, 30, 400, 270, backend.clone());
    let mut menu = fltk::menu::MenuBar::default().with_size(400, 30);
    let menu_backend = backend.clone();

    win.resizable(&text_editor.group);

    menu.add(
        "File/Open...",
        fltk::enums::Shortcut::Ctrl | 'o',
        fltk::menu::MenuFlag::Normal,
        move |_| {
            if let Some(file_path) =
                fltk::dialog::file_chooser("Open File", "*.{txt,rs,md,log}", ".", false)
            {
                menu_backend.borrow_mut().open_file(file_path).unwrap();
                text_editor.on_content_changed();

                fltk::app::redraw();
            }
        },
    );

    win.end();
    win.show();

    app.run().unwrap();
}
