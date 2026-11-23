#![forbid(unsafe_code)]
//! The program `halvo-model`: the door of `halvo_model::run` on this process's arguments, standard
//! input, standard output and standard error. It reads no variable of its environment.

fn main() {
    let args: Vec<String> = std::env::args_os()
        .skip(1)
        .map(|arg| arg.into_string().unwrap_or_default())
        .collect();
    let code = halvo_model::run(
        &args,
        std::io::stdin().lock(),
        std::io::stdout().lock(),
        std::io::stderr().lock(),
    );
    std::process::exit(code);
}
