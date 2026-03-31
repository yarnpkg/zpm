mod progress;

pub use progress::ProgressState;

use zpm_utils::DataType;

static PREFIX_COLORS: [DataType; 5] = [
    DataType::Custom(46, 134, 171),
    DataType::Custom(162, 59, 114),
    DataType::Custom(241, 143, 1),
    DataType::Custom(199, 62, 29),
    DataType::Custom(204, 226, 163),
];

pub fn prefix_colors() -> impl Iterator<Item = &'static DataType> {
    PREFIX_COLORS.iter().cycle()
}
