use base64::Engine;
use std::io::Write;
use std::process::{Command, Stdio};

pub fn copy_text(text: &str) {
    // 1. Try system clipboard tools
    let commands = [
        ("wl-copy", vec![]),
        ("xclip", vec!["-selection", "clipboard"]),
        ("xsel", vec!["--clipboard", "--input"]),
        ("pbcopy", vec![]),
    ];

    for (cmd, args) in commands {
        if let Ok(mut child) = Command::new(cmd)
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
        {
            if let Some(mut stdin) = child.stdin.take() {
                let _ = stdin.write_all(text.as_bytes());
            }
            let _ = child.wait();
            break;
        }
    }

    // 2. Also emit OSC 52 sequence to stdout
    let b64 = base64::engine::general_purpose::STANDARD.encode(text);
    let osc52 = format!("\x1b]52;c;{}\x07", b64);
    let _ = std::io::stdout().write_all(osc52.as_bytes());
    let _ = std::io::stdout().flush();
}
