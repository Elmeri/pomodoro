use anyhow::Result;
use notify_rust::Notification;
use std::io::{self, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

pub fn run_timer(project: &str, minutes: u64) -> Result<Option<u64>> {
    // Shared flag between the main thread and the Ctrl+C signal handler
    let running = Arc::new(AtomicBool::new(true));
    let r = Arc::clone(&running);

    ctrlc::set_handler(move || {
        r.store(false, Ordering::SeqCst);
    })?;

    let total_secs = minutes * 60;
    let start = Instant::now();

    println!("Starting {}m session for [{}] (Ctrl+C to abort)", minutes, project);

    while running.load(Ordering::SeqCst) {
        let elapsed = start.elapsed().as_secs();
        if elapsed >= total_secs {
            // Normal completion: trigger ASCII bell + native OS banner
            print!("\x07");
            io::stdout().flush()?;

            let _ = Notification::new()
                .summary("Pomodoro Complete!")
                .body(&format!("Finished {}m session on [{}].", minutes, project))
                .show();

            println!("\r✅ Session complete! ({}m logged to {})   ", minutes, project);
            return Ok(Some(minutes));
        }

        let remaining = total_secs - elapsed;
        let mins = remaining / 60;
        let secs = remaining % 60;

        print!("\r⏳ Time remaining: {:02}:{:02} ", mins, secs);
        io::stdout().flush()?;

        thread::sleep(Duration::from_millis(200));
    }

    // If we exited the loop because running became false, the user pressed Ctrl+C
    let elapsed_mins = start.elapsed().as_secs() / 60;
    println!(); // Move off the countdown line

    if elapsed_mins == 0 {
        println!("Session aborted (less than 1 minute elapsed). Nothing saved.");
        return Ok(None);
    }

    print!("Save partial {}m session? [y/N]: ", elapsed_mins);
    io::stdout().flush()?;

    let mut input = String::new();
    io::stdin().read_line(&mut input)?;

    if input.trim().eq_ignore_ascii_case("y") {
        println!("✅ Saved partial {}m session to [{}].", elapsed_mins, project);
        Ok(Some(elapsed_mins))
    } else {
        println!("Session discarded.");
        Ok(None)
    }
}