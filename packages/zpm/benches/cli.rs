use tokio::runtime::Runtime;
use zpm_utils::Path;

fn main() {
    divan::main();
}

#[divan::bench(sample_count = 10)]
fn search_vector(bencher: divan::Bencher) {
    let rt
        = Runtime::new().unwrap();

    let temp_directory
        = Path::temp_dir().unwrap();

    std::env::set_current_dir(temp_directory.to_path_buf()).unwrap();

    rt.block_on(async {
        zpm::commands::run_default(Some(vec!["debug".to_string(), "bench".to_string(), "install-full-cold".to_string(), "--prepare".to_string(), "gatsby".to_string()])).await;
    });

    bencher.bench_local(|| {
        rt.block_on(async {
            zpm::commands::run_default(Some(vec!["debug".to_string(), "bench".to_string(), "install-full-cold".to_string(), "--cleanup".to_string()])).await;
            zpm::commands::run_default(Some(vec!["debug".to_string(), "bench".to_string(), "install-full-cold".to_string(), "--iteration".to_string()])).await;
        });
    });
}
