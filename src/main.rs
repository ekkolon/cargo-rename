fn main() {
    if let Err(err) = cargo_rename::run() {
        eprintln!("{err}");
        std::process::exit(1);
    }
}
