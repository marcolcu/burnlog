fn main() {
    if let Err(error) = burnlog::run() {
        eprintln!("{error}");
        std::process::exit(1);
    }
}
