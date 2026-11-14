#[test]
fn the_programs_print_set_is_the_last_blocks_flag_and_pref_lines() {
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_profile-trial"))
        .arg("--print-set")
        .output()
        .expect("profile-trial --print-set");
    assert!(out.status.success());
    let printed = String::from_utf8(out.stdout).expect("utf-8");
    let file = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../../docs/r3/trials/browser-profile.md"),
    )
    .expect("the result file");
    let start = file.rfind("```text\n").expect("a block") + "```text\n".len();
    let block = &file[start..];
    let block = &block[..block.find("```").expect("the block's end")];
    let lines: Vec<&str> = block
        .lines()
        .filter(|l| l.starts_with("flag ") || l.starts_with("pref "))
        .collect();
    assert_eq!(printed.lines().collect::<Vec<_>>(), lines);
}
