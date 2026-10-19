//! Run the real addon's existing WoW API smoke fixture under its Lua 5.1 runtime.
#![cfg(feature = "local")]

#[test]
fn addon_guide_smoke() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let previous = std::env::current_dir().unwrap();
    std::env::set_current_dir(&root).unwrap();
    let lua = mlua::Lua::new();
    let result = lua
        .load(std::fs::read_to_string("addon/tests/smoke.lua").unwrap())
        .set_name("addon/tests/smoke.lua")
        .exec();
    std::env::set_current_dir(previous).unwrap();
    result.unwrap();
}
