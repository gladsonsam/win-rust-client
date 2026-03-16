use browser_url::{get_active_browser_url, is_browser_active};
use std::thread;
use std::time::Duration;

fn main() {
    println!("🌐 Browser URL - Basic Usage Demo");
    println!("==================================");

    // Give the user time to switch to a browser window
    println!("\n📋 Instructions:");
    println!("1. Open a browser (Chrome, Firefox, Zen, etc.)");
    println!("2. Navigate to any website");
    println!("3. When it says 'NOW!', quickly click on the browser window");
    println!("\n⏰ Starting in 5 seconds...");

    for i in (1..=5).rev() {
        println!("   {i} seconds...");
        thread::sleep(Duration::from_secs(1));
    }

    println!("\n🚀 NOW! Quickly click on your browser window!");
    thread::sleep(Duration::from_millis(2000));

    // Check if a browser is the active window
    println!("\n🔍 Checking for active browser...");
    if !is_browser_active() {
        println!("❌ No browser window detected as active");
        println!("\n🔄 Retrying for 10 seconds — switch to your browser!");

        let mut found = false;
        for i in (1..=10).rev() {
            thread::sleep(Duration::from_secs(1));
            if is_browser_active() {
                println!("✅ Browser detected!");
                found = true;
                break;
            }
            if i > 1 {
                println!("   Checking... {} seconds left", i - 1);
            }
        }

        if !found {
            println!("❌ Still no browser detected.");
            println!("💡 Try running again while keeping a browser window focused:");
            println!("   cargo run --example basic_usage");
            return;
        }
    } else {
        println!("✅ Browser detected immediately!");
    }

    // Extract browser info including URL
    println!("\n🔗 Extracting browser URL...");
    match get_active_browser_url() {
        Ok(info) => {
            println!("✅ Browser info extracted successfully!\n");
            println!("   🔗 URL:        {}", info.url);
            println!("   📝 Title:      {}", info.title);
            println!("   🌐 Browser:    {} ({:?})", info.browser_name, info.browser_type);
            println!("   🆔 Process ID: {}", info.process_id);
            println!(
                "   📐 Position:   ({:.0}, {:.0})",
                info.window_position.x, info.window_position.y
            );
            println!(
                "   📏 Size:       {:.0}x{:.0}",
                info.window_position.width, info.window_position.height
            );
        }
        Err(e) => {
            println!("❌ Failed to extract browser info: {e}");
        }
    }

    println!("\n🎯 Demo completed!");
}
