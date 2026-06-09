use std::env;
use std::ffi::OsStr;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;

const PRODUCT_SLUG: &str = "mimium-audio-plugin";
const PRODUCT_NAME: &str = "Mimium Audio Plugin";
const CARGO_PACKAGE_NAME: &str = "mimium-audio-plugin";
const COMPILER_WORKER_PACKAGE_NAME: &str = "mimium-compiler-worker";
const BUNDLE_IDENTIFIER: &str = "org.mimium.mimium-audio-plugin";

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

    fn cmake_name(self) -> &'static str {
        match self {
            Self::Debug => "Debug",
            Self::Release => "Release",
        }
    }
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum PackageFormat {
    Clap,
    Vst3,
    Au,
}

impl PackageFormat {
    fn parse(value: &str) -> Result<Self, String> {
        match value {
            "clap" => Ok(Self::Clap),
            "vst3" => Ok(Self::Vst3),
            "au" => Ok(Self::Au),
            other => Err(format!("unsupported format: {other}")),
        }
    }

    fn target_name(self) -> Option<&'static str> {
        match self {
            Self::Clap => None,
            Self::Vst3 => Some("mimium_audio_plugin_vst3"),
            Self::Au => Some("mimium_audio_plugin_auv2"),
        }
    }

    fn bundle_extension(self) -> &'static str {
        match self {
            Self::Clap => "clap",
            Self::Vst3 => "vst3",
            Self::Au => "component",
        }
    }

    fn requires_wrapper(self) -> bool {
        matches!(self, Self::Vst3 | Self::Au)
    }

    fn install_dir(self) -> &'static str {
        match self {
            Self::Clap => "CLAP",
            Self::Vst3 => "VST3",
            Self::Au => "Components",
        }
    }
}

struct Options {
    profile: BuildProfile,
    formats: Vec<PackageFormat>,
    install: bool,
}

struct BuiltArtifact {
    format: PackageFormat,
    path: PathBuf,
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
    let mut formats = Vec::new();
    let mut install = false;
    let mut iter = args.peekable();

    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--release" => profile = BuildProfile::Release,
            "--debug" => profile = BuildProfile::Debug,
            "--install" => install = true,
            "--all-formats" => {
                push_format(&mut formats, PackageFormat::Clap);
                push_format(&mut formats, PackageFormat::Vst3);
                if cfg!(target_os = "macos") {
                    push_format(&mut formats, PackageFormat::Au);
                }
            }
            "--format" => {
                let value = iter
                    .next()
                    .ok_or_else(|| "--format requires a value".to_string())?;
                let format = PackageFormat::parse(&value)?;
                push_format(&mut formats, format);
            }
            "--help" | "-h" => {
                print_usage();
                std::process::exit(0);
            }
            other => return Err(format!("unknown package flag: {other}")),
        }
    }

    if formats.is_empty() {
        formats.push(PackageFormat::Clap);
    }

    Ok(Options {
        profile,
        formats,
        install,
    })
}

fn package(options: Options) -> Result<(), String> {
    if !cfg!(target_os = "macos") && options.formats.contains(&PackageFormat::Au) {
        return Err("au packaging is only supported on macOS".to_string());
    }

    let workspace_root = workspace_root()?;
    let target_root = workspace_root.join("target");
    let package_dir = target_root.join("package").join(options.profile.dir_name());
    let wrapper_stage_dir = target_root.join("wrapper-stage").join(options.profile.dir_name());
    let wrapper_build_dir = target_root.join("wrapper-build").join(options.profile.dir_name());

    build_webview(&workspace_root)?;
    cargo_build_plugin(&workspace_root, options.profile)?;
    let compiler_worker_binary = build_compiler_worker(&workspace_root, options.profile)?;
    let clap_binary = built_clap_binary_path(&workspace_root, options.profile)?;

    let clap_artifact = if options.formats.contains(&PackageFormat::Clap) {
        Some(stage_clap_artifact(&clap_binary, &compiler_worker_binary, &package_dir)?)
    } else {
        None
    };

    let clap_bundle = if options.formats.iter().any(|format| format.requires_wrapper()) {
        Some(stage_clap_bundle(
            &clap_binary,
            &compiler_worker_binary,
            &wrapper_stage_dir,
        )?)
    } else {
        None
    };

    if options.formats.iter().any(|format| format.requires_wrapper()) {
        let bundle = clap_bundle
            .as_deref()
            .ok_or_else(|| "missing staged CLAP bundle for wrapper build".to_string())?;
        configure_wrapper_project(
            &workspace_root,
            &wrapper_build_dir,
            bundle,
            &options.formats,
            options.profile,
        )?;
        build_wrapper_targets(&wrapper_build_dir, &options.formats, options.profile)?;
        sync_embedded_clap_bundles(&wrapper_build_dir, bundle, &options.formats)?;
    }

    let artifacts = collect_artifacts(
        clap_artifact.as_deref(),
        &wrapper_build_dir,
        &package_dir,
        &options.formats,
    )?;

    if options.install {
        install_artifacts(&artifacts)?;
        println!("installed packaged plugins into ~/Library/Audio/Plug-Ins");
    }

    for artifact in &artifacts {
        println!("built {}: {}", artifact.format.bundle_extension(), artifact.path.display());
    }

    Ok(())
}

fn build_webview(workspace_root: &Path) -> Result<(), String> {
    let webview_dir = workspace_root.join("webview");
    if !webview_dir.join("package.json").exists() {
        return Err(format!(
            "missing webview/package.json at {}",
            webview_dir.display()
        ));
    }

    let mut command = Command::new("pnpm");
    command.current_dir(&webview_dir);
    command.arg("build");
    run_command(command, "pnpm build (webview)")
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

fn stage_clap_artifact(
    clap_binary: &Path,
    compiler_worker_binary: &Path,
    package_dir: &Path,
) -> Result<PathBuf, String> {
    #[cfg(target_os = "macos")]
    {
        let bundle_dir = package_dir.join(format!("{PRODUCT_NAME}.clap"));
        let contents_dir = bundle_dir.join("Contents");
        let macos_dir = contents_dir.join("MacOS");
        let resources_dir = contents_dir.join("Resources");
        let plist_path = contents_dir.join("Info.plist");
        fs::create_dir_all(&macos_dir).map_err(io_error)?;
        fs::create_dir_all(&resources_dir).map_err(io_error)?;

        let entrypoint = macos_dir.join(PRODUCT_SLUG);
        if entrypoint.exists() {
            remove_path_if_exists(&entrypoint)?;
        }
        fs::copy(clap_binary, &entrypoint).map_err(io_error)?;

        let worker_target = resources_dir.join(compiler_worker_binary_name());
        if worker_target.exists() {
            remove_path_if_exists(&worker_target)?;
        }
        fs::copy(compiler_worker_binary, worker_target).map_err(io_error)?;

        fs::write(plist_path, clap_info_plist()).map_err(io_error)?;
        Ok(bundle_dir)
    }

    #[cfg(not(target_os = "macos"))]
    {
        fs::create_dir_all(package_dir).map_err(io_error)?;
        let artifact_path = package_dir.join(format!("{PRODUCT_SLUG}.clap"));
        if artifact_path.exists() {
            remove_path_if_exists(&artifact_path)?;
        }
        fs::copy(clap_binary, &artifact_path).map_err(io_error)?;

        let worker_path = package_dir.join(compiler_worker_binary_name());
        if worker_path.exists() {
            remove_path_if_exists(&worker_path)?;
        }
        fs::copy(compiler_worker_binary, worker_path).map_err(io_error)?;

        Ok(artifact_path)
    }
}

fn stage_clap_bundle(
    clap_binary: &Path,
    compiler_worker_binary: &Path,
    wrapper_stage_dir: &Path,
) -> Result<PathBuf, String> {
    #[cfg(target_os = "macos")]
    {
        let bundle_dir = wrapper_stage_dir.join(format!("{PRODUCT_NAME}.clap"));
        let contents_dir = bundle_dir.join("Contents");
        let macos_dir = contents_dir.join("MacOS");
        let resources_dir = contents_dir.join("Resources");
        let plist_path = contents_dir.join("Info.plist");
        fs::create_dir_all(&macos_dir).map_err(io_error)?;
        fs::create_dir_all(&resources_dir).map_err(io_error)?;

        let entrypoint = macos_dir.join(PRODUCT_SLUG);
        if entrypoint.exists() {
            remove_path_if_exists(&entrypoint)?;
        }
        fs::copy(clap_binary, &entrypoint).map_err(io_error)?;

        let worker_target = resources_dir.join(compiler_worker_binary_name());
        if worker_target.exists() {
            remove_path_if_exists(&worker_target)?;
        }
        fs::copy(compiler_worker_binary, worker_target).map_err(io_error)?;

        fs::write(plist_path, clap_info_plist()).map_err(io_error)?;
        Ok(bundle_dir)
    }

    #[cfg(not(target_os = "macos"))]
    {
        fs::create_dir_all(wrapper_stage_dir).map_err(io_error)?;
        let staged = wrapper_stage_dir.join(format!("{PRODUCT_SLUG}.clap"));
        if staged.exists() {
            remove_path_if_exists(&staged)?;
        }
        fs::copy(clap_binary, &staged).map_err(io_error)?;

        let worker_path = wrapper_stage_dir.join(compiler_worker_binary_name());
        if worker_path.exists() {
            remove_path_if_exists(&worker_path)?;
        }
        fs::copy(compiler_worker_binary, worker_path).map_err(io_error)?;

        Ok(staged)
    }
}

fn build_compiler_worker(workspace_root: &Path, profile: BuildProfile) -> Result<PathBuf, String> {
    let mut command = Command::new("cargo");
    command
        .current_dir(workspace_root)
        .arg("build")
        .arg("-p")
        .arg(COMPILER_WORKER_PACKAGE_NAME);
    if let Some(flag) = profile.cargo_flag() {
        command.arg(flag);
    }
    run_command(command, "cargo build (compiler-worker)")?;

    let worker_path = workspace_root
        .join("target")
        .join(profile.dir_name())
        .join(compiler_worker_binary_name());

    if !worker_path.exists() {
        return Err(format!(
            "missing compiler worker binary at {}",
            worker_path.display()
        ));
    }

    Ok(worker_path)
}

fn compiler_worker_binary_name() -> &'static str {
    if cfg!(target_os = "windows") {
        "mimium-compiler-worker.exe"
    } else {
        "mimium-compiler-worker"
    }
}

fn clap_info_plist() -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n<plist version=\"1.0\">\n<dict>\n  <key>CFBundleDevelopmentRegion</key>\n  <string>en</string>\n  <key>CFBundleExecutable</key>\n  <string>{PRODUCT_SLUG}</string>\n  <key>CFBundleIdentifier</key>\n  <string>{BUNDLE_IDENTIFIER}.clap</string>\n  <key>CFBundleName</key>\n  <string>{PRODUCT_NAME}</string>\n  <key>CFBundlePackageType</key>\n  <string>BNDL</string>\n  <key>CFBundleShortVersionString</key>\n  <string>{}</string>\n  <key>CFBundleVersion</key>\n  <string>{}</string>\n</dict>\n</plist>\n",
        env!("CARGO_PKG_VERSION"),
        env!("CARGO_PKG_VERSION")
    )
}

fn configure_wrapper_project(
    workspace_root: &Path,
    wrapper_build_dir: &Path,
    clap_bundle: &Path,
    formats: &[PackageFormat],
    profile: BuildProfile,
) -> Result<(), String> {
    fs::create_dir_all(wrapper_build_dir).map_err(io_error)?;

    let clap_wrapper_root = clap_wrapper_root(workspace_root)?;
    let packaging_dir = workspace_root.join("packaging").join("clap-wrapper");

    let mut command = Command::new("cmake");
    command
        .arg("-S")
        .arg(&packaging_dir)
        .arg("-B")
        .arg(wrapper_build_dir)
        .arg(format!("-DCLAP_WRAPPER_ROOT={}", clap_wrapper_root.display()))
        .arg(format!("-DCLAP_PLUGIN_BUNDLE={}", clap_bundle.display()))
        .arg(format!("-DCLAP_WRAPPER_OUTPUT_NAME={PRODUCT_NAME}"))
        .arg(format!("-DCLAP_WRAPPER_BUNDLE_IDENTIFIER={BUNDLE_IDENTIFIER}"))
        .arg(format!(
            "-DCLAP_WRAPPER_BUNDLE_VERSION={}",
            env!("CARGO_PKG_VERSION")
        ))
        .arg("-DCLAP_WRAPPER_DOWNLOAD_DEPENDENCIES=TRUE");

    if cfg!(target_os = "macos") && formats.contains(&PackageFormat::Au) {
        command
            .arg("-DCLAP_WRAPPER_BUILD_AUV2=ON")
            .arg(format!(
                "-DCLAP_WRAPPER_AUV2_MANUFACTURER_NAME={}",
                env_or_default("CLAP_WRAPPER_AUV2_MANUFACTURER_NAME", "Mimium")
            ))
            .arg(format!(
                "-DCLAP_WRAPPER_AUV2_MANUFACTURER_CODE={}",
                env_or_default("CLAP_WRAPPER_AUV2_MANUFACTURER_CODE", "Mimi")
            ))
            .arg(format!(
                "-DCLAP_WRAPPER_AUV2_SUBTYPE_CODE={}",
                env_or_default("CLAP_WRAPPER_AUV2_SUBTYPE_CODE", "Mclp")
            ))
            .arg(format!(
                "-DCLAP_WRAPPER_AUV2_INSTRUMENT_TYPE={}",
                env_or_default("CLAP_WRAPPER_AUV2_INSTRUMENT_TYPE", "aumu")
            ));
    }

    if let Some(clap_sdk_root) = env::var_os("CLAP_SDK_ROOT") {
        command.arg(format!(
            "-DCLAP_SDK_ROOT={}",
            PathBuf::from(clap_sdk_root).display()
        ));
    }

    if let Some(vst3_sdk_root) = env::var_os("VST3_SDK_ROOT") {
        command.arg(format!(
            "-DVST3_SDK_ROOT={}",
            PathBuf::from(vst3_sdk_root).display()
        ));
    }

    if cfg!(target_os = "macos") {
        command.arg(format!("-DCMAKE_BUILD_TYPE={}", profile.cmake_name()));
    }

    run_command(command, "cmake configure")
}

fn build_wrapper_targets(
    wrapper_build_dir: &Path,
    formats: &[PackageFormat],
    profile: BuildProfile,
) -> Result<(), String> {
    let mut names = formats.iter().filter_map(|f| f.target_name()).peekable();
    if names.peek().is_none() {
        return Ok(());
    }

    let mut command = Command::new("cmake");
    command.arg("--build").arg(wrapper_build_dir);

    if cfg!(target_os = "windows") {
        command.arg("--config").arg(profile.cmake_name());
    }

    command.arg("--target");
    for name in names {
        command.arg(name);
    }

    run_command(command, "cmake build")
}

fn collect_artifacts(
    clap_artifact: Option<&Path>,
    wrapper_build_dir: &Path,
    package_dir: &Path,
    formats: &[PackageFormat],
) -> Result<Vec<BuiltArtifact>, String> {
    let mut artifacts = Vec::new();

    for format in formats {
        match format {
            PackageFormat::Clap => {
                let path = clap_artifact
                    .ok_or_else(|| "requested clap artifact is missing".to_string())?
                    .to_path_buf();
                artifacts.push(BuiltArtifact {
                    format: *format,
                    path,
                });
            }
            PackageFormat::Vst3 => {
                let path = stage_wrapper_artifact(*format, wrapper_build_dir, package_dir)?;
                artifacts.push(BuiltArtifact {
                    format: *format,
                    path,
                });
            }
            PackageFormat::Au => {
                let path = stage_wrapper_artifact(*format, wrapper_build_dir, package_dir)?;
                artifacts.push(BuiltArtifact {
                    format: *format,
                    path,
                });
            }
        }
    }

    Ok(artifacts)
}

fn sync_embedded_clap_bundles(
    wrapper_build_dir: &Path,
    clap_bundle: &Path,
    formats: &[PackageFormat],
) -> Result<(), String> {
    for format in formats {
        if !format.requires_wrapper() {
            continue;
        }

        let wrapper_bundle =
            wrapper_build_dir.join(format!("{PRODUCT_NAME}.{}", format.bundle_extension()));
        if !wrapper_bundle.exists() {
            continue;
        }

        let embedded_bundle = wrapper_bundle
            .join("Contents")
            .join("PlugIns")
            .join(format!("{PRODUCT_NAME}.clap"));

        if embedded_bundle.exists() {
            remove_path_if_exists(&embedded_bundle)?;
        }
        copy_dir_recursive(clap_bundle, &embedded_bundle)?;
    }

    Ok(())
}

fn find_first_matching_artifact(dir: &Path, extension: &str) -> Result<Option<PathBuf>, String> {
    let mut stack = vec![dir.to_path_buf()];

    while let Some(current) = stack.pop() {
        let entries = match fs::read_dir(&current) {
            Ok(entries) => entries,
            Err(_) => continue,
        };

        for entry in entries {
            let entry = entry.map_err(io_error)?;
            let path = entry.path();
            let file_type = entry.file_type().map_err(io_error)?;

            let has_target_extension = path
                .extension()
                .and_then(|value| value.to_str())
                .is_some_and(|value| value.eq_ignore_ascii_case(extension));

            if has_target_extension {
                return Ok(Some(path));
            }

            if file_type.is_dir() {
                stack.push(path);
                continue;
            }
        }
    }

    Ok(None)
}

fn install_artifacts(artifacts: &[BuiltArtifact]) -> Result<(), String> {
    for artifact in artifacts {
        install_artifact_into(&artifact.path, artifact.format.install_dir())?;
    }
    Ok(())
}

fn install_artifact_into(artifact_path: &Path, install_dir: &str) -> Result<(), String> {
    let install_root = home_dir()?
        .join("Library")
        .join("Audio")
        .join("Plug-Ins")
        .join(install_dir);
    fs::create_dir_all(&install_root).map_err(io_error)?;
    let destination = install_root.join(
        artifact_path
            .file_name()
            .ok_or_else(|| "invalid artifact path".to_string())?,
    );

    if destination.exists() {
        remove_path_if_exists(&destination)?;
    }

    if artifact_path.is_dir() {
        copy_dir_recursive(artifact_path, &destination)?;
    } else {
        fs::copy(artifact_path, &destination).map_err(io_error)?;
    }

    Ok(())
}

fn copy_dir_recursive(src: &Path, dst: &Path) -> Result<(), String> {
    fs::create_dir_all(dst).map_err(io_error)?;

    for entry in fs::read_dir(src).map_err(io_error)? {
        let entry = entry.map_err(io_error)?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());
        let file_type = entry.file_type().map_err(io_error)?;

        if file_type.is_dir() {
            copy_dir_recursive(&src_path, &dst_path)?;
        } else {
            fs::copy(&src_path, &dst_path).map_err(io_error)?;
        }
    }

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

fn clap_wrapper_root(workspace_root: &Path) -> Result<PathBuf, String> {
    if let Some(value) = env::var_os("CLAP_WRAPPER_ROOT") {
        let path = PathBuf::from(value);
        if path.exists() {
            return Ok(path);
        }
        return Err(format!("CLAP_WRAPPER_ROOT does not exist: {}", path.display()));
    }

    let default = workspace_root.join("third_party").join("clap-wrapper");
    if default.exists() {
        Ok(default)
    } else {
        Err(format!(
            "clap-wrapper not found. Set CLAP_WRAPPER_ROOT or place it at {}",
            default.display()
        ))
    }
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

fn push_format(formats: &mut Vec<PackageFormat>, format: PackageFormat) {
    if !formats.contains(&format) {
        formats.push(format);
    }
}

fn stage_wrapper_artifact(
    format: PackageFormat,
    wrapper_build_dir: &Path,
    package_dir: &Path,
) -> Result<PathBuf, String> {
    fs::create_dir_all(package_dir).map_err(io_error)?;
    let expected_path = wrapper_build_dir.join(format!("{PRODUCT_NAME}.{}", format.bundle_extension()));
    let built_path = if expected_path.exists() {
        expected_path
    } else {
        find_first_matching_artifact(wrapper_build_dir, format.bundle_extension())?
            .ok_or_else(|| format!("failed to locate built {} bundle", format.bundle_extension()))?
    };

    let destination = package_dir.join(
        built_path
            .file_name()
            .ok_or_else(|| "invalid wrapper artifact path".to_string())?,
    );

    if destination.exists() {
        remove_path_if_exists(&destination)?;
    }

    if built_path.is_dir() {
        copy_dir_recursive(&built_path, &destination)?;
    } else {
        fs::copy(&built_path, &destination).map_err(io_error)?;
    }

    Ok(destination)
}

fn env_or_default(key: &str, default: &str) -> String {
    env::var(key).ok().filter(|value| !value.trim().is_empty()).unwrap_or_else(|| default.to_string())
}

fn print_usage() {
    println!(
        "Usage:\n  cargo xtask package [--release|--debug] [--format clap|vst3|au] [--all-formats] [--install]\n\nCommands:\n  package     Build and stage plugin artifacts (.clap, .vst3, and on macOS .component)"
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
