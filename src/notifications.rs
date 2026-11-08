use std::process::{Command, Stdio};

pub(crate) fn send(title: impl Into<String>, body: impl Into<String>) {
    let title = title.into();
    let body = body.into();
    let mut child = match Command::new("notify-send")
        .args([
            "--app-name=Filem",
            "--icon=filem",
            "--expire-time=5000",
            "--",
        ])
        .arg(&title)
        .arg(&body)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    {
        Ok(child) => child,
        Err(error) => {
            eprintln!("Could not start desktop notification: {error}");
            return;
        }
    };

    std::thread::spawn(move || match child.wait() {
        Ok(status) if status.success() => {}
        Ok(status) => eprintln!("Desktop notification exited with status {status}."),
        Err(error) => eprintln!("Could not wait for desktop notification: {error}"),
    });
}
