use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};

#[test]
fn panic_hook_restores_original_on_return_and_unwind_without_stacking() {
    const CHILD: &str = "LABELDECK_PANIC_HOOK_TEST";
    const VERIFIED: &str = "panic hook lifecycle verified";
    if std::env::var_os(CHILD).is_none() {
        // Hooks are process-global; isolate from concurrent tests and nextest.
        let thread = std::thread::current();
        let mut command =
            std::process::Command::new(std::env::current_exe().unwrap());
        command
            .args(["--exact", thread.name().unwrap(), "--nocapture"])
            .env(CHILD, "1");
        // Crossterm's Windows fallback emits no bracketed-paste ANSI bytes.
        #[cfg(windows)]
        command.env("TERM", "xterm");
        let output = command.output().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(
            stdout.contains(VERIFIED),
            "child did not verify the panic hook: {stdout}"
        );
        assert_eq!(stdout.matches("\u{1b}[?2004l").count(), 2);
        return;
    }
    verify_hook_lifecycle();
    println!("{VERIFIED}");
}

fn verify_hook_lifecycle() {
    let calls = Arc::new(AtomicUsize::new(0));
    let count = Arc::clone(&calls);
    let original: Hook = Box::new(move |_| {
        count.fetch_add(1, Ordering::SeqCst);
    });
    let identity = &*original as *const _;
    panic::set_hook(original);
    for unwind in [false, true, false, true] {
        let result = panic::catch_unwind(|| {
            PanicHookGuard::install().run(|| {
                // Model the extra wrapper installed by Ratatui's try_init.
                let previous = panic::take_hook();
                panic::set_hook(Box::new(move |info| previous(info)));
                if unwind {
                    panic!("TUI unwind");
                }
                42
            })
        });
        assert_eq!(result.is_err(), unwind);
        if !unwind {
            assert_eq!(result.unwrap(), 42);
        }
        let restored = panic::take_hook();
        assert!(std::ptr::eq(&*restored, identity));
        panic::set_hook(restored);
    }
    assert_eq!(calls.load(Ordering::SeqCst), 2);
}
