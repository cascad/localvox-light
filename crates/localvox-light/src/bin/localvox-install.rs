//! Runs from the verified staging bundle, before the destination is modified.
use clap::Parser;
use localvox_light::setup::install;

#[derive(Parser)]
#[command(about = "Установка и проверка LocalVox", version)]
struct Args {
    #[arg(long)]
    install_dir: Option<std::path::PathBuf>,
    #[arg(long)]
    bundle_check: bool,
    /// Compare the packaged release identity after verifying every bundle file.
    #[arg(long, requires = "bundle_check")]
    release_tag: Option<String>,
    #[arg(long)]
    check: bool,
    /// Portable installation: do not change PATH or shell profiles.
    #[arg(long)]
    no_path_update: bool,
    #[arg(long)]
    staging_root: Option<std::path::PathBuf>,
}

fn main() -> anyhow::Result<()> {
    #[cfg(windows)]
    unsafe {
        windows_sys::Win32::System::Diagnostics::Debug::SetErrorMode(3);
    }
    let args = Args::parse();
    let destination = args
        .install_dir
        .unwrap_or_else(install::default_program_dir);
    if args.bundle_check {
        return match localvox_light::setup::bundle::Bundle::read(&destination) {
            Ok(bundle) => {
                use localvox_light::setup::bundle::ReleaseState;
                match args
                    .release_tag
                    .as_deref()
                    .map(|tag| bundle.release_state(tag))
                {
                    None | Some(ReleaseState::Current) => Ok(()),
                    Some(ReleaseState::InstalledNewer) => std::process::exit(3),
                    Some(ReleaseState::Different) => std::process::exit(2),
                }
            }
            Err(error) => {
                eprintln!("{error:#}");
                std::process::exit(2);
            }
        };
    }
    let source = std::env::current_exe()?.parent().unwrap().to_owned();
    let result = install::run(
        &source,
        &destination,
        install::Options {
            check: args.check,
            register_path: !args.no_path_update,
            staging_root: args.staging_root,
            ..Default::default()
        },
    );
    match result {
        Ok(code) => std::process::exit(code),
        Err(error) if args.check => {
            eprintln!("{error:#}");
            std::process::exit(2);
        }
        Err(error) => Err(error),
    }
}
