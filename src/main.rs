mod backend;
mod batch;
mod config;
mod fs_entry;
mod icons;
mod image_conversion;
mod lifecycle;
mod metadata;
mod natsort;
mod notifications;
mod ops;
mod paths;
mod preview;
mod search;
mod store;
mod sysclip;
mod tags;
mod theme;
mod transfer;
mod transfer_export;
mod volumes;

use std::path::PathBuf;

use qmetaobject::prelude::*;

use backend::Backend;
use theme::{Theme, ThemeData};

fn find_qml() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os("FILEM_QML_DIR") {
        let candidate = PathBuf::from(dir).join("Main.qml");
        if candidate.is_file() {
            return Some(candidate);
        }
    }

    if let Ok(cwd) = std::env::current_dir() {
        let candidate = cwd.join("qml/Main.qml");
        if candidate.is_file() {
            return Some(candidate);
        }
    }

    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            for candidate in [
                dir.join("qml/Main.qml"),
                dir.join("../share/filem/qml/Main.qml"),
                dir.join("../Resources/qml/Main.qml"),
            ] {
                if candidate.is_file() {
                    return Some(candidate);
                }
            }
        }
    }

    None
}

fn print_help() {
    println!(
        "Filem — a quiet, keyboard-first file manager\n\n\
         Usage: filem [path] [options]\n\n\
         Options:\n  \
         -h, --help       Show this help\n  \
         -V, --version    Show version\n  \
         --smoke-test     Load the UI, then quit automatically\n  \
         --screenshot FILE  Save one rendered frame to FILE, then quit\n  \
         --print-theme    Print the resolved theme and exit\n\n\
         Environment:\n  \
         FILEM_QML_DIR    Directory containing Main.qml\n"
    );
}

extern "C" fn log_to_stdout(
    _kind: qmetaobject::log::QtMsgType,
    _context: &qmetaobject::log::QMessageLogContext,
    message: &QString,
) {
    eprintln!("{message}");
}

fn main() {
    if std::env::var_os("QT_FFMPEG_DECODING_HW_DEVICE_TYPES").is_none() {
        std::env::set_var("QT_FFMPEG_DECODING_HW_DEVICE_TYPES", "");
    }
    qmetaobject::log::install_message_handler(Some(log_to_stdout));
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|arg| arg == "--help" || arg == "-h") {
        print_help();
        return;
    }
    if args.iter().any(|arg| arg == "--version" || arg == "-V") {
        println!("filem {}", env!("CARGO_PKG_VERSION"));
        return;
    }
    if args.iter().any(|arg| arg == "--print-theme") {
        let loaded = ThemeData::load();
        println!("source: {}", loaded.source);
        if let Some(warning) = loaded.warning {
            eprintln!("warning: {warning}");
        }
        println!("{}", theme::describe_theme(&loaded.data));
        return;
    }

    let smoke_test = args.iter().any(|arg| arg == "--smoke-test");
    let screenshot = args
        .iter()
        .position(|arg| arg == "--screenshot")
        .and_then(|index| args.get(index + 1))
        .cloned()
        .unwrap_or_default();
    let mut initial = String::new();
    let mut index = 1;
    while index < args.len() {
        let arg = &args[index];
        if arg == "--screenshot" {
            index += 2;
            continue;
        }
        if !arg.starts_with('-') && initial.is_empty() {
            initial = arg.clone();
        }
        index += 1;
    }

    let instance_guard = if smoke_test || !screenshot.is_empty() {
        None
    } else {
        match lifecycle::InstanceGuard::acquire(&initial) {
            Ok(Some(guard)) => Some(guard),
            Ok(None) => return,
            Err(error) => {
                eprintln!("Could not activate Filem: {error}");
                std::process::exit(1);
            }
        }
    };

    qmetaobject::qml_register_type::<Backend>(c"Filem", 1, 0, c"Backend");
    qmetaobject::qml_register_type::<Theme>(c"Filem", 1, 0, c"Theme");
    qmetaobject::qml_register_type::<lifecycle::Lifecycle>(c"Filem", 1, 0, c"Lifecycle");

    let qml = find_qml().unwrap_or_else(|| {
        eprintln!("Could not find qml/Main.qml. Run from the project root or set FILEM_QML_DIR.");
        std::process::exit(1);
    });

    let mut engine = QmlEngine::new();
    engine.set_property(
        QString::from("initialPath"),
        QVariant::from(QString::from(initial)),
    );
    engine.set_property(QString::from("smokeTest"), QVariant::from(smoke_test));
    engine.set_property(
        QString::from("screenshotPath"),
        QVariant::from(QString::from(screenshot)),
    );
    engine.load_file(
        QString::try_from(qml.as_path())
            .unwrap_or_else(|_| QString::from(qml.to_string_lossy().into_owned())),
    );
    engine.exec();
    transfer_export::cleanup();
    drop(instance_guard);
}
