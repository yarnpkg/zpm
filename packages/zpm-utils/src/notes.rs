use colored::Colorize;

use crate::DataType;


pub enum Note {
    Warning(String),
    Info(String),
}

impl Note {
    pub fn print(&self) {
        match self {
            Note::Info(message) => {
                print_message("info", DataType::Info, message);
            },
            Note::Warning(message) => {
                print_message("warning", DataType::Warning, message);
            },
        }
    }
}

fn print_message(label: &str, data_type: DataType, message: &str) {
    println!("");

    let mut lines
        = message.trim().lines();

    let prefix
        = format!("{label}: ");
    let indent
        = " ".repeat(prefix.len());

    println!("{}{}", data_type.colorize(&prefix).bold(), lines.next().unwrap().bold());

    for line in lines {
        println!("{}{}", indent, line.trim());
    }
}
