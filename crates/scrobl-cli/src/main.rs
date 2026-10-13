//! The `scrobl` binary. Everything it does is in the library next to it.

use std::process::ExitCode;

use scrobl::ApiKey;
use scrobl_cli::cli;

fn main() -> ExitCode {
    let api_key = std::env::var("SCROBL_API_KEY").ok().map(ApiKey::new);
    let args = std::env::args_os()
        .skip(1)
        .map(|arg| arg.to_string_lossy().into_owned());
    let (mut out, mut err) = (std::io::stdout(), std::io::stderr());

    // The client spawns nothing, so one thread is all it needs.
    let runtime = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(_) => return ExitCode::FAILURE,
    };
    ExitCode::from(runtime.block_on(cli::run(args, api_key, &mut out, &mut err)))
}
