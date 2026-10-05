use std::{
    process::Command,
    time::{Duration, Instant},
};

pub(super) fn enter_input(adb: &str, device: &str) {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        let output = Command::new(adb)
            .args(["-s", device, "shell", "dumpsys", "input_method"])
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "inspect Android terminal input connection"
        );
        let state = String::from_utf8(output.stdout).unwrap();
        if state.contains("packageName=com.sailry.sailry_mobile.acceptance")
            && state.contains("inputType=0x1 ")
            && state.contains("mInputShown=true")
        {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "ordinary Android terminal keyboard did not open"
        );
        std::thread::sleep(Duration::from_millis(100));
    }
    assert!(
        Command::new(adb)
            .args(["-s", device, "shell", "input", "text", "sailryime"])
            .status()
            .unwrap()
            .success(),
        "enter Android terminal input"
    );
    println!("Android ordinary text keyboard opened; native key input delivered");
}
