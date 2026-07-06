use std::process::ExitCode;

fn main() -> ExitCode {
    match caelix_cli::run_from_env() {
        Ok(output) => {
            print!("{output}");
            ExitCode::SUCCESS
        }
        Err(err) => {
            eprintln!("error: {err}");
            ExitCode::FAILURE
        }
    }
}
