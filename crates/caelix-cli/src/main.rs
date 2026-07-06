use std::process::ExitCode;

fn main() -> ExitCode {
    match caelix_cli::run_from_env() {
        Ok(outcome) => match outcome {
            caelix_cli::CliOutcome::Output(output) => {
                print!("{output}");
                ExitCode::SUCCESS
            }
            caelix_cli::CliOutcome::Exit(code) => exit_code(code),
        },
        Err(err) => {
            eprintln!("error: {err}");
            ExitCode::FAILURE
        }
    }
}

fn exit_code(code: i32) -> ExitCode {
    match u8::try_from(code) {
        Ok(code) => ExitCode::from(code),
        Err(_) => ExitCode::FAILURE,
    }
}
