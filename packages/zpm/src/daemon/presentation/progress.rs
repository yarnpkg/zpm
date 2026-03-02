use std::collections::BTreeSet;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

use zpm_utils::DataType;

fn interpolate_gradient(keyframes: &[(u8, u8, u8)], steps_between: usize) -> Vec<(u8, u8, u8)> {
    let mut colors = Vec::with_capacity(keyframes.len() * steps_between);

    for i in 0..keyframes.len() {
        let (r1, g1, b1) = keyframes[i];
        let (r2, g2, b2) = keyframes[(i + 1) % keyframes.len()];

        for step in 0..steps_between {
            let t = step as f32 / steps_between as f32;

            let r = (r1 as f32 + (r2 as f32 - r1 as f32) * t) as u8;
            let g = (g1 as f32 + (g2 as f32 - g1 as f32) * t) as u8;
            let b = (b1 as f32 + (b2 as f32 - b1 as f32) * t) as u8;

            colors.push((r, g, b));
        }
    }

    colors
}

fn generate_gradient_frames(text: &str) -> Vec<String> {
    let keyframes: [(u8, u8, u8); 4] = [
        (100, 149, 237),
        (65, 105, 225),
        (30, 144, 255),
        (0, 191, 255),
    ];

    let gradient_colors = interpolate_gradient(&keyframes, 8);

    let chars: Vec<char> = text.chars().collect();

    (0..gradient_colors.len())
        .map(|frame| {
            let mut result = String::with_capacity(text.len() * 20);

            for (i, ch) in chars.iter().enumerate() {
                let color_idx = (i * 2 + gradient_colors.len() - frame) % gradient_colors.len();

                let (r, g, b) = gradient_colors[color_idx];

                result.push_str(&format!("\x1b[38;2;{};{};{}m{}", r, g, b, ch));
            }

            result.push_str("\x1b[0m");
            result
        })
        .collect()
}

pub struct ProgressState {
    pub total: AtomicUsize,
    pub completed: AtomicUsize,
    pub running_tasks: Mutex<BTreeSet<String>>,
    gradient_frames: Vec<String>,
}

impl ProgressState {
    pub fn new(total: usize) -> Self {
        let gradient_frames = generate_gradient_frames("Running dependencies");

        Self {
            total: AtomicUsize::new(total),
            completed: AtomicUsize::new(0),
            running_tasks: Mutex::new(BTreeSet::new()),
            gradient_frames,
        }
    }

    pub fn add_to_total(&self, count: usize) {
        self.total.fetch_add(count, Ordering::Relaxed);
    }

    pub fn add_task(&self, task_name: &str) {
        self.running_tasks.lock().unwrap().insert(task_name.to_string());
    }

    pub fn remove_task(&self, task_name: &str) {
        self.running_tasks.lock().unwrap().remove(task_name);
        self.completed.fetch_add(1, Ordering::Relaxed);
    }

    pub fn format_progress(&self, frame_idx: usize) -> String {
        let total = self.total.load(Ordering::Relaxed);
        let completed = self.completed.load(Ordering::Relaxed);
        let running = self.running_tasks.lock().unwrap().len();
        let scheduled = total.saturating_sub(running).saturating_sub(completed);

        let label = &self.gradient_frames[frame_idx % self.gradient_frames.len()];

        format!(
            "{} {}",
            label,
            DataType::Custom(128, 128, 128).colorize(&format!(
                "· running {} · scheduled {} · completed {}",
                running,
                scheduled,
                completed
            ))
        )
    }
}
