//! The `scrobl` binary. Everything it does is in the library next to it.

use std::process::ExitCode;

use scrobl::ApiKey;
use scrobl_cli::cli::{self, API_KEY_VARIABLE, Environment};
use scrobl_cli::time;

fn main() -> ExitCode {
    let environment = Environment {
        api_key: std::env::var(API_KEY_VARIABLE).ok().map(ApiKey::new),
        now: time::now(),
    };
    let (mut out, mut err) = (std::io::stdout(), std::io::stderr());

    // The client spawns nothing, so one thread is all it needs.
    let runtime = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(_) => return ExitCode::FAILURE,
    };
    let code = runtime.block_on(cli::run(
        std::env::args_os().skip(1),
        &environment,
        &mut out,
        &mut err,
    ));
    ExitCode::from(code)
}
