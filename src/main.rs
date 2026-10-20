//! Snags — a Padagonia-backed issues-encountered index for agents and users.

mod cli;
mod model;
mod repo;
mod report;

fn main() {
    if let Err(err) = cli::run() {
        eprintln!("error: {err:?}");
        std::process::exit(1);
    }
}
