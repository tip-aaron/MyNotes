pub fn main() {
    // Open the file provided as the first CLI argument, or start with empty content.
    let content: Option<String> = std::env::args()
        .nth(1)
        .and_then(|path| std::fs::read_to_string(path).ok());

    ui::run(content.as_deref());
}
