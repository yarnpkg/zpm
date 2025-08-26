use colored::{Color, Colorize};

const INFO_COLOR: Color
    = Color::TrueColor { r: 87, g: 163, b: 255 };

const WARNING_COLOR: Color
    = Color::TrueColor { r: 255, g: 87, b: 51 };

pub enum Note {
    Warning(String),
    Info(String),
}

impl Note {
    pub fn print(&self) {
        match self {
            Note::Info(message) => {
                print_message("info", INFO_COLOR, message);
            },
            Note::Warning(message) => {
                print_message("warning", WARNING_COLOR, message);
            },
        }
    }
}

fn print_message(label: &str, color: Color, message: &str) {
    println!("");

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
