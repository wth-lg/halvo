fn main() {
    std::process::exit(halvo_runner::main(std::env::args_os().skip(1).collect()));
}
