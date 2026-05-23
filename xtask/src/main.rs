use std::env;
use std::ffi::OsStr;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;

const PRODUCT_SLUG: &str = "mimium-clap-plugin";
const CARGO_PACKAGE_NAME: &str = "mimium-clap-plugin";

#[derive(Clone, Copy, Eq, PartialEq)]
enum BuildProfile {
    Debug,
    Release,
}

impl BuildProfile {
    fn cargo_flag(self) -> Option<&'static str> {
        match self {
            Self::Debug => None,
            Self::Release => Some("--release"),
        }
    }

    fn dir_name(self) -> &'static str {
        match self {
            Self::Debug => "debug",
            Self::Release => "release",
        }
    }
}

struct Options {
    profile: BuildProfile,
    install: bool,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("error: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let workspace_root = workspace_root()?;
    load_dotenv(&workspace_root)?;

    let mut args = env::args().skip(1);
    let Some(command) = args.next() else {
        print_usage();
        return Ok(());
    };

    match command.as_str() {
        "package" => package(parse_package_args(args)?)?,
        "help" | "--help" | "-h" => print_usage(),
        other => return Err(format!("unknown xtask command: {other}")),
    }

    Ok(())
}

fn parse_package_args(args: impl Iterator<Item = String>) -> Result<Options, String> {
    let mut profile = BuildProfile::Release;
    let mut install = false;

    for arg in args {
        match arg.as_str() {
            "--release" => profile = BuildProfile::Release,
            "--debug" => profile = BuildProfile::Debug,
            "--install" => install = true,
            "--help" | "-h" => {
                print_usage();
                std::process::exit(0);
            }
            other => return Err(format!("unknown package flag: {other}")),
        }
    }

    Ok(Options { profile, install })
}

fn package(options: Options) -> Result<(), String> {
    let workspace_root = workspace_root()?;
    let target_root = workspace_root.join("target");
    let package_dir = target_root.join("package").join(options.profile.dir_name());

    cargo_build_plugin(&workspace_root, options.profile)?;
    let clap_binary = built_clap_binary_path(&workspace_root, options.profile)?;

    fs::create_dir_all(&package_dir).map_err(io_error)?;
    let artifact_path = package_dir.join(format!("{PRODUCT_SLUG}.clap"));

    if artifact_path.exists() {
        remove_path_if_exists(&artifact_path)?;
    }
    fs::copy(&clap_binary, &artifact_path).map_err(io_error)?;

    println!("built clap: {}", artifact_path.display());

    if options.install {
        install_artifact(&artifact_path)?;
        println!("installed clap into ~/Library/Audio/Plug-Ins/CLAP");
    }

    Ok(())
}

fn cargo_build_plugin(workspace_root: &Path, profile: BuildProfile) -> Result<(), String> {
    let mut command = Command::new("cargo");
    command.current_dir(workspace_root);
    command.arg("build").arg("-p").arg(CARGO_PACKAGE_NAME);
    if let Some(flag) = profile.cargo_flag() {
        command.arg(flag);
    }
    run_command(command, "cargo build")
}

fn built_clap_binary_path(workspace_root: &Path, profile: BuildProfile) -> Result<PathBuf, String> {
    let artifact_path = workspace_root
        .join("target")
        .join(profile.dir_name())
        .join(rust_cdylib_name());

    if !artifact_path.exists() {
        return Err(format!(
            "missing Rust plugin binary at {}. Run cargo build first.",
            artifact_path.display()
        ));
    }

    Ok(artifact_path)
}

fn install_artifact(artifact_path: &Path) -> Result<(), String> {
    let install_root = home_dir()?.join("Library").join("Audio").join("Plug-Ins").join("CLAP");
    fs::create_dir_all(&install_root).map_err(io_error)?;
    let destination = install_root.join(
        artifact_path
            .file_name()
            .ok_or_else(|| "invalid artifact path".to_string())?,
    );

    if destination.exists() {
        remove_path_if_exists(&destination)?;
    }
    fs::copy(artifact_path, &destination).map_err(io_error)?;

    Ok(())
}

fn rust_cdylib_name() -> String {
    if cfg!(target_os = "macos") {
        format!("lib{}.dylib", CARGO_PACKAGE_NAME.replace('-', "_"))
    } else if cfg!(target_os = "windows") {
        format!("{}.dll", CARGO_PACKAGE_NAME.replace('-', "_"))
    } else {
        format!("lib{}.so", CARGO_PACKAGE_NAME.replace('-', "_"))
    }
}

fn run_command(mut command: Command, description: &str) -> Result<(), String> {
    let status = command.status().map_err(io_error)?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("{description} failed with status {status}"))
    }
}

fn workspace_root() -> Result<PathBuf, String> {
    let mut path = env::current_dir().map_err(io_error)?;

    loop {
        if path.join("Cargo.toml").exists() {
            return Ok(path);
        }

        if !path.pop() {
            return Err("failed to locate workspace root".to_string());
        }
    }
}

fn home_dir() -> Result<PathBuf, String> {
    env::var_os("HOME")
        .map(PathBuf::from)
        .ok_or_else(|| "HOME is not set".to_string())
}

fn load_dotenv(workspace_root: &Path) -> Result<(), String> {
    let dotenv_path = workspace_root.join(".env");
    if dotenv_path.exists() {
        dotenvy::from_path(&dotenv_path).map_err(|error| error.to_string())?;
    }
    Ok(())
}

fn remove_path_if_exists(path: &Path) -> Result<(), String> {
    if !path.exists() {
        return Ok(());
    }

    let metadata = fs::metadata(path).map_err(io_error)?;
    if metadata.is_dir() {
        fs::remove_dir_all(path).map_err(io_error)
    } else {
        fs::remove_file(path).map_err(io_error)
    }
}

fn io_error(error: io::Error) -> String {
    error.to_string()
}

fn print_usage() {
    println!(
        "Usage:\n  cargo xtask package [--release|--debug] [--install]\n\nCommands:\n  package     Build and stage a .clap plugin binary"
    );
}

#[allow(dead_code)]
fn _is_ci() -> bool {
    env::var_os("CI").is_some()
}

#[allow(dead_code)]
fn _arg_has_prefix(arg: &OsStr, prefix: &str) -> bool {
    arg.to_str().is_some_and(|value| value.starts_with(prefix))
}
