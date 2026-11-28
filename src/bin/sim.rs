fn main() {
    // Output cut short by `| head` ends the program quietly.
    unsafe { libc::signal(libc::SIGPIPE, libc::SIG_DFL) };
    std::process::exit(sts2bot::sim::cli::main(std::env::args().skip(1).collect()));
}
