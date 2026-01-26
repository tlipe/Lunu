use clap::{Parser, Subcommand};
use std::path::PathBuf;
use lunu_builder::build_executable;

#[derive(Parser)]
#[command(name = "lunu-build")]
#[command(about = "Lunu Builder - Create standalone executables")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Build a Luau script into an executable
    Build {
        /// The entry point script (e.g., main.luau)
        script: PathBuf,

        /// Output filename (optional)
        #[arg(short, long)]
        output: Option<PathBuf>,

        /// Force rebuild ignoring cache
        #[arg(short, long)]
        force: bool,

        /// Open the output after successful build
        #[arg(long)]
        open: bool,

        /// Custom icon path for the executable
        #[arg(long)]
        icon: Option<PathBuf>,

        #[arg(long, num_args = 0..=1, default_missing_value = "true")]
        open_cmd: Option<bool>,
    },
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Build { script, output, force, open, icon, open_cmd } => {
            build_executable(&script, output, force, open, icon, open_cmd, None)?;
        }
    }
    Ok(())
}
