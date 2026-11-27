fn main() {
    std::process::exit(sts2bot::scripts::main(std::env::args().skip(1).collect()));
}
