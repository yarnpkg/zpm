use colored::{Color, Colorize};

const WARNING_COLOR: Color
    = Color::TrueColor { r: 255, g: 87, b: 51 };

pub enum Note {
    Warning(String),
}

impl Note {
    pub fn print(&self) {
        match self {
            Note::Warning(message) => {
                print_message("warning", WARNING_COLOR, message);
            },
        }
    }
}

fn print_message(label: &str, color: Color, message: &str) {
    let mut lines
        = message.trim().lines();

    let prefix
        = format!("{label}: ");
    let indent
        = " ".repeat(prefix.len());

    println!("{}{}", prefix.bold().color(color), lines.next().unwrap().bold());

    for line in lines {
        println!("{}{}", indent, line.trim());
    }
}
