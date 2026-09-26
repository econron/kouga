use std::{error::Error, io, path::Path, time::Duration};

use clap::ValueEnum;
use kouga_migration::{
    AdminTarget, Environment, MigrationSet, MigrationState, Migrator, MigratorOptions, RepairState,
};

use crate::{DbCommand, check_app, invalid, run_app};

#[derive(Clone, Copy, Debug, ValueEnum)]
pub(super) enum RepairTarget {
    Applied,
    Pending,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
pub(super) enum TargetEnvironment {
    Development,
    Test,
    Production,
}

impl TargetEnvironment {
    fn migration(self) -> Environment {
        match self {
            Self::Development => Environment::Development,
            Self::Test => Environment::Test,
            Self::Production => Environment::Production,
        }
    }
}

fn migrator(url: &str, runtime: &tokio::runtime::Runtime) -> Result<Migrator, Box<dyn Error>> {
    let db = runtime.block_on(kouga_db::connect(url, 5, Duration::from_secs(5)))?;
    let set = MigrationSet::load(app_dir().join("migrations"))?;
    Ok(Migrator::new(db, set, MigratorOptions::default()))
}

fn app_dir() -> &'static Path {
    if Path::new("apps/http/Cargo.toml").is_file() && !Path::new("src/lib.rs").is_file() {
        Path::new("apps/http")
    } else {
        Path::new(".")
    }
}

fn seed_registered() -> Result<(), Box<dyn Error>> {
    if !app_dir().join("src/bin/task-seed.rs").is_file() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "seed task not registered; create src/bin/task-seed.rs in the HTTP application",
        )
        .into());
    }
    Ok(())
}

fn database_url() -> Result<String, Box<dyn Error>> {
    std::env::var("DATABASE_URL").map_err(|_| invalid("DATABASE_URL must be set").into())
}

pub(super) fn run(command: DbCommand) -> Result<(), Box<dyn Error>> {
    check_app()?;
    match command {
        DbCommand::Create => run_app("db-create"),
        DbCommand::Migrate => {
            let url = database_url()?;
            let runtime = tokio::runtime::Runtime::new()?;
            let count = runtime.block_on(migrator(&url, &runtime)?.migrate())?;
            println!("Applied {count} migration(s)");
            Ok(())
        }
        DbCommand::Seed => {
            seed_registered()?;
            run_app("task-seed")
        }
        DbCommand::Schema { output } => {
            if output.as_os_str().is_empty() {
                return Err(invalid("schema output path is required").into());
            }
            kouga_migration::dump_schema(&database_url()?, &output)?;
            println!("Wrote {}", output.display());
            Ok(())
        }
        DbCommand::Status
        | DbCommand::Rollback { .. }
        | DbCommand::Repair { .. }
        | DbCommand::Reset { .. } => {
            let url = database_url()?;
            let runtime = tokio::runtime::Runtime::new()?;
            match command {
                DbCommand::Status => {
                    let statuses = runtime.block_on(migrator(&url, &runtime)?.status())?;
                    for status in statuses {
                        let state = match status.state {
                            MigrationState::Pending => "pending",
                            MigrationState::Applied => "applied",
                            MigrationState::Dirty => "dirty",
                        };
                        let reversibility = if status.reversible {
                            "reversible"
                        } else {
                            "irreversible"
                        };
                        println!(
                            "{}\t{}\t{}\t{}",
                            status.version, state, reversibility, status.name
                        );
                    }
                }
                DbCommand::Rollback { steps } => {
                    let count =
                        runtime.block_on(migrator(&url, &runtime)?.rollback(steps as usize))?;
                    println!("Rolled back {count} migration(s)");
                }
                DbCommand::Repair {
                    version,
                    state,
                    reason,
                } => {
                    let state = match state {
                        RepairTarget::Applied => RepairState::Applied,
                        RepairTarget::Pending => RepairState::Pending,
                    };
                    runtime.block_on(migrator(&url, &runtime)?.repair(&version, state, &reason))?;
                    println!("Repaired migration {version}");
                }
                DbCommand::Reset {
                    database,
                    environment,
                    allow_destructive,
                    allow_production,
                    seed,
                } => {
                    // An explicit environment and exact database name are mandatory. Never infer
                    // production safety from an optional KOUGA_ENV setting.
                    if !allow_destructive {
                        return Err(invalid("reset requires --allow-destructive").into());
                    }
                    if matches!(environment, TargetEnvironment::Production) && !allow_production {
                        return Err(
                            invalid("production reset also requires --allow-production").into()
                        );
                    }
                    if let Ok(configured) = std::env::var("KOUGA_ENV") {
                        let selected = match environment {
                            TargetEnvironment::Development => "development",
                            TargetEnvironment::Test => "test",
                            TargetEnvironment::Production => "production",
                        };
                        if configured != selected {
                            return Err(invalid("--environment must match KOUGA_ENV").into());
                        }
                    }
                    if seed {
                        seed_registered()?;
                    }
                    let set = MigrationSet::load(app_dir().join("migrations"))?;
                    let target = AdminTarget {
                        url: &url,
                        expected_database: &database,
                        environment: environment.migration(),
                        allow_destructive,
                        allow_production,
                    };
                    println!("Resetting database {database} ({environment:?})");
                    let count = runtime.block_on(kouga_migration::reset_database(
                        &target,
                        set,
                        MigratorOptions::default(),
                    ))?;
                    println!("Applied {count} migration(s)");
                    if seed {
                        run_app("task-seed")?;
                    }
                }
                _ => unreachable!(),
            }
            Ok(())
        }
    }
}
