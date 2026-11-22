//! Runs from the verified staging bundle, before the destination is modified.
use clap::Parser;
use localvox_light::setup::install;

#[derive(Parser)]
#[command(about = "Установка и проверка LocalVox", version)]
struct Args {
    #[arg(long, hide = true, conflicts_with = "download_file")]
    download_protocol: bool,
    #[arg(long, hide = true, requires = "download_output")]
    download_file: Option<String>,
    #[arg(long, hide = true, requires = "download_file")]
    download_output: Option<std::path::PathBuf>,
    #[arg(long, hide = true, requires = "download_file")]
    download_sha256: Option<String>,
    #[arg(long, hide = true, requires = "download_file")]
    download_size: Option<u64>,
    #[arg(long, hide = true, requires = "download_file")]
    download_fallback: Vec<String>,
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
    if args.download_protocol {
        println!("{}", localvox_setup_ui::progress::PROTOCOL);
        return Ok(());
    }
    if let Some(url) = args.download_file {
        let request = localvox_light::setup::download::Request {
            urls: std::iter::once(url).chain(args.download_fallback).collect(),
            output: args.download_output.expect("clap requires download_output"),
            sha256: args.download_sha256,
            size: args.download_size,
        };
        return if std::env::var("LOCALVOX_DOWNLOAD_PROTOCOL").as_deref() == Ok("1") {
            let mut output = std::io::stdout().lock();
            localvox_light::setup::download::fetch(&request, |event| {
                let _ = event.write(&mut output);
            })
        } else {
            let mut activity = localvox_setup_ui::activity::Activity::start("Скачивание модели");
            let result =
                localvox_light::setup::download::fetch(&request, |event| activity.progress(event));
            activity.finish(result.is_ok());
            result
        };
    }
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
