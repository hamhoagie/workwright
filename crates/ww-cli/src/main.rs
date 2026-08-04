fn main() {
    let cli = cli::Cli::parse();
    let client = client::Client::from_env();

    let result = match cli.command {
        cli::Commands::Tasks(args) => client.list_tasks(&args),
        cli::Commands::Task(args) => client.create_task(&args),
        cli::Commands::Crit(args) => client.submit_crit(&args),
        cli::Commands::Taste(args) => client.show_taste(&args),
        cli::Commands::Status => client.show_status(),
        cli::Commands::Me => client.show_me(),
        cli::Commands::Users => client.list_users(),
    };

    if let Err(e) = result {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}

mod cli;
mod client;
mod output;

use clap::Parser;
