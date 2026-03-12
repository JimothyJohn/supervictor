use std::path::PathBuf;

use clap::{Parser, Subcommand};

use qs::commands::{certs, dev, edge, onboard, ping, prod, staging};
use qs::config::ProjectConfig;
use qs::error;
use qs::output;
use qs::runner::{self, RealRunner};

#[derive(Parser)]
#[command(
    name = "qs",
    about = "Supervictor CLI — dev, staging, and prod pipelines"
)]
struct Cli {
    #[arg(short, long, global = true, help = "Show full command output")]
    verbose: bool,

    #[arg(long, global = true, help = "Print commands without executing")]
    dry_run: bool,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Local dev cycle: unit tests + sam local + integration tests
    Dev {
        #[arg(long, help = "Start sam local as a background daemon")]
        serve: bool,
        #[arg(long, help = "Stop a running sam local daemon")]
        stop: bool,
    },

    /// Build, flash, and monitor the embedded device
    Edge,

    /// Dev gate + deploy to dev stack + remote tests
    Staging,

    /// Full pipeline + confirmation + prod deployment
    Prod,

    /// Generate and verify mTLS certificates
    Certs {
        #[command(subcommand)]
        command: CertsCommands,
    },

    /// mTLS GET to verify the server is up
    Ping {
        #[arg(long, help = "Directory with client.pem and client.key")]
        certs: Option<PathBuf>,
        #[arg(long, help = "CA certificate")]
        ca: Option<PathBuf>,
        #[arg(long, default_value = "localhost", help = "Server host")]
        host: String,
        #[arg(long, default_value_t = 443, help = "Server port")]
        port: u16,
    },

    /// End-to-end device onboarding
    Onboard {
        #[arg(long, required = true)]
        device_name: String,
        #[arg(long, required = true)]
        owner_id: String,
        #[arg(long, default_value = "onprem", value_parser = ["onprem", "aws"])]
        mode: String,
        #[arg(long, default_value_t = 0, help = "Resume from phase N")]
        start_at: usize,
        #[arg(long, num_args = 0.., help = "Skip phase numbers")]
        skip: Vec<usize>,
    },
}

#[derive(Subcommand)]
enum CertsCommands {
    /// Initialize the root CA
    Ca,
    /// Issue a device client certificate
    Device {
        name: String,
        #[arg(long, help = "Validity in days")]
        days: Option<u32>,
    },
    /// Issue a server/TLS certificate
    Server {
        name: String,
        #[arg(long, default_value = "127.0.0.1", help = "SAN IP")]
        host_ip: String,
        #[arg(long, help = "Validity in days")]
        days: Option<u32>,
    },
    /// List all issued certificates
    List,
    /// Verify the mTLS certificate chain
    Verify {
        #[arg(long, default_value = "esp32", help = "Device cert to verify")]
        device_name: String,
        #[arg(long, default_value = "caddy", help = "Server cert to verify")]
        server_name: String,
    },
    /// Simulate mTLS handshake against a running server
    Handshake {
        #[arg(long, default_value = "localhost", help = "Server host")]
        host: String,
        #[arg(long, default_value = "443", help = "Server port")]
        port: String,
        #[arg(long, default_value = "esp32", help = "Device cert to use")]
        device_name: String,
        #[arg(long, default_value = "tls1_3", help = "TLS version flag")]
        tls_version: Option<String>,
        #[arg(long, help = "Also test without client cert")]
        test_no_client: bool,
    },
}

fn find_repo_root() -> PathBuf {
    let mut p = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    loop {
        if p.join(".git").exists() {
            return p;
        }
        if !p.pop() {
            break;
        }
    }
    // Fallback: assume we're in the repo
    std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
}

fn main() {
    let cli = Cli::parse();
    output::set_verbose(cli.verbose);

    let root = find_repo_root();
    let config = ProjectConfig::from_repo_root(&root);
    let runner = RealRunner;

    let code = match run(cli, &config, &runner) {
        Ok(code) => code,
        Err(e) => {
            output::error(&format!("{}", e));
            1
        }
    };

    std::process::exit(code);
}

fn run(cli: Cli, config: &ProjectConfig, r: &dyn runner::Runner) -> Result<i32, error::CliError> {
    match cli.command {
        Commands::Dev { serve, stop } => dev::run_dev(
            &dev::DevArgs {
                verbose: cli.verbose,
                dry_run: cli.dry_run,
                serve,
                stop,
            },
            config,
            r,
        ),

        Commands::Edge => edge::run_edge(
            &edge::EdgeArgs {
                verbose: cli.verbose,
                dry_run: cli.dry_run,
            },
            config,
            r,
        ),

        Commands::Staging => staging::run_staging(
            &staging::StagingArgs {
                verbose: cli.verbose,
                dry_run: cli.dry_run,
            },
            config,
            r,
            false,
        ),

        Commands::Prod => prod::run_prod(
            &prod::ProdArgs {
                verbose: cli.verbose,
                dry_run: cli.dry_run,
            },
            config,
            r,
        ),

        Commands::Certs { command } => {
            let certs_cmd = match command {
                CertsCommands::Ca => certs::CertsCommand::Ca,
                CertsCommands::Device { name, days } => certs::CertsCommand::Device { name, days },
                CertsCommands::Server {
                    name,
                    host_ip,
                    days,
                } => certs::CertsCommand::Server {
                    name,
                    host_ip,
                    days,
                },
                CertsCommands::List => certs::CertsCommand::List,
                CertsCommands::Verify {
                    device_name,
                    server_name,
                } => certs::CertsCommand::Verify {
                    device_name,
                    server_name,
                },
                CertsCommands::Handshake {
                    host,
                    port,
                    device_name,
                    tls_version,
                    test_no_client,
                } => certs::CertsCommand::Handshake {
                    host,
                    port,
                    device_name,
                    tls_version,
                    test_no_client,
                },
            };
            certs::run_certs(
                &certs::CertsArgs {
                    verbose: cli.verbose,
                    dry_run: cli.dry_run,
                    command: certs_cmd,
                },
                config,
                r,
            )
        }

        Commands::Ping {
            certs: certs_path,
            ca,
            host,
            port,
        } => ping::run_ping(
            &ping::PingArgs {
                certs: certs_path,
                ca,
                host,
                port,
                dry_run: cli.dry_run,
            },
            config,
        ),

        Commands::Onboard {
            device_name,
            owner_id,
            mode,
            start_at,
            skip,
        } => onboard::run_onboard(
            &onboard::OnboardArgs {
                device_name,
                owner_id,
                mode,
                verbose: cli.verbose,
                dry_run: cli.dry_run,
                start_at,
                skip,
            },
            config,
            r,
        ),
    }
}
