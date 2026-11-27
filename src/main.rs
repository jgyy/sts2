fn main() {
    std::process::exit(sts2bot::cli::main(std::env::args().skip(1).collect()));
}
