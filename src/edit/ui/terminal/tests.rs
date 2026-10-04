use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};

#[test]
fn panic_hook_restores_original_on_return_and_unwind_without_stacking() {
    const CHILD: &str = "LABELDECK_PANIC_HOOK_TEST";
    if std::env::var_os(CHILD).is_none() {
        // Hooks are process-global; isolate from concurrent tests and nextest.
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "edit::ui::terminal::tests::panic_hook_restores_original_on_return_and_unwind_without_stacking"])
            .env(CHILD, "1")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert_eq!(stdout.matches("\u{1b}[?2004l").count(), 2);
        return;
    }
    verify_hook_lifecycle();
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
