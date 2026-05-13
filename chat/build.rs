/*
This is part of WHY2
Copyright (C) 2022-2026 Václav Šmejkal

This program is free software: you can redistribute it and/or modify
it under the terms of the GNU General Public License as published by
the Free Software Foundation, either version 3 of the License, or
(at your option) any later version.

This program is distributed in the hope that it will be useful,
but WITHOUT ANY WARRANTY; without even the implied warranty of
MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
GNU General Public License for more details.

You should have received a copy of the GNU General Public License
along with this program.  If not, see <https://www.gnu.org/licenses/>.
*/

use std::
{
    env,
    process::Command,
};

fn main()
{
    //DO NOT USE WHY2_DEV_BYPASS IN PRODUCTION!!!
    if env::var("WHY2_DEV_BYPASS").is_ok() { return; }

    //ENSURE CORRECT FEATURE USAGE
    let client_feature = env::var("CARGO_FEATURE_CLIENT").is_ok();
    let server_feature = env::var("CARGO_FEATURE_SERVER").is_ok();
    let chat_feature = env::var("CARGO_FEATURE_CHAT").is_ok();

    //DIRECT CHAT FEATURE USE
    if chat_feature && !(client_feature || server_feature)
    {
        panic!("Do not enable `chat` directly - use `client` or `server`.");
    }

    //USE OF SERVER AND CLIENT FEATURES COMBINED
    if client_feature && server_feature
    {
        panic!
        (
            "Error: You are trying to enable both `client` and `server` features at the same time.\n\
             By default, the 'client' feature is enabled.\n\n\
             To install the SERVER, use:\n\
             cargo install why2-chat --no-default-features --features server"
        );
    }

    //CONFIG DIRECTORY
    let config_dir = env::var("WHY2_CONFIG_DIR").unwrap_or_else(|_| "{HOME}/.config/WHY2".to_string());
    println!("cargo:rustc-env=WHY2_CONFIG_DIR={config_dir}");
    println!("cargo:rerun-if-env-changed=WHY2_CONFIG_DIR");

    //HASH
    let git_hash = Command::new("git")
        .args(&["rev-parse", "--short", "HEAD"])
        .output()
        .ok()
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|| String::new());

    println!("cargo:rustc-env=WHY2_GIT_HASH={git_hash}");
    println!("cargo:rerun-if-changed=../.git/HEAD");
    println!("cargo:rerun-if-changed=../.git/index");
}
