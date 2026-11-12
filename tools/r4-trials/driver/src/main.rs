use std::process::ExitCode;

fn main() -> ExitCode {
    let started = r4_driver_trial::options::parse_args(std::env::args().skip(1))
        .and_then(|options| r4_driver_trial::builder(&options));
    match started {
        Ok(builder) => match r4_trial_app::run(builder, r4_trial_app::context()) {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("r4-driver-trial: {e}");
                ExitCode::from(1)
            }
        },
        Err(refusal) => {
            eprintln!("{refusal}");
            ExitCode::from(2)
        }
    }
}
