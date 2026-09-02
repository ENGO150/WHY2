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

use winresource::WindowsResource;

//APPLICATION MANIFEST (DPI AWARENESS, LONG PATHS, NO UAC PROMPT)
const WINDOWS_MANIFEST: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<assembly xmlns="urn:schemas-microsoft-com:asm.v1" manifestVersion="1.0">
    <trustInfo xmlns="urn:schemas-microsoft-com:asm.v3">
        <security>
            <requestedPrivileges>
                <requestedExecutionLevel level="asInvoker" uiAccess="false" />
            </requestedPrivileges>
        </security>
    </trustInfo>
    <compatibility xmlns="urn:schemas-microsoft-com:compatibility.v1">
        <application>
            <supportedOS Id="{8e0f7a12-bfb3-4fe8-b9a5-48fd50a15a9a}" />
            <supportedOS Id="{1f676c76-80e1-4239-95bb-83d0f6d0da78}" />
            <supportedOS Id="{4a2f28e3-53b9-4441-ba9c-d69d4a4a6e38}" />
        </application>
    </compatibility>
    <application xmlns="urn:schemas-microsoft-com:asm.v3">
        <windowsSettings>
            <dpiAware xmlns="http://schemas.microsoft.com/SMI/2005/WindowsSettings">true/pm</dpiAware>
            <dpiAwareness xmlns="http://schemas.microsoft.com/SMI/2016/WindowsSettings">permonitorv2</dpiAwareness>
            <activeCodePage xmlns="http://schemas.microsoft.com/SMI/2019/WindowsSettings">UTF-8</activeCodePage>
            <longPathAware xmlns="http://schemas.microsoft.com/SMI/2016/WindowsSettings">true</longPathAware>
        </windowsSettings>
    </application>
</assembly>
"#;

//EMBED ICON + VERSION INFO INTO THE WINDOWS BINARIES
fn windows_resources()
{
    println!("cargo:rerun-if-changed=assets/why2.ico");

    if env::var("CARGO_FEATURE_WINDOWS_RESOURCES").is_err() { return; }
    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") { return; }

    let server = env::var("CARGO_FEATURE_SERVER").is_ok();

    let (binary, description) = if server
    {
        ("why2-server.exe", "WHY2 chat server")
    } else
    {
        ("why2.exe", "WHY2 chat client")
    };

    let mut resource = WindowsResource::new();

    resource.set_icon("assets/why2.ico");
    resource.set_manifest(WINDOWS_MANIFEST);

    resource.set("ProductName", "WHY2");
    resource.set("FileDescription", description);
    resource.set("InternalName", binary);
    resource.set("OriginalFilename", binary);
    resource.set("CompanyName", "Václav Šmejkal");
    resource.set("LegalCopyright", "Copyright (C) 2022-2026 Václav Šmejkal - GPL-3.0-only");
    resource.set("Comments", "https://why2.satan.red");

    //NON-FATAL: A MISSING RESOURCE COMPILER MUST NOT BREAK THE BUILD
    if let Err(error) = resource.compile()
    {
        println!("cargo:warning=failed to embed windows resources: {error}");
    }
}

fn main()
{
    windows_resources();

    //DO NOT USE WHY2_DEV_BYPASS IN PRODUCTION!!!
    if env::var("WHY2_DEV_BYPASS").is_ok() { return; }

    //ENSURE CORRECT FEATURE USAGE
    let client_feature = env::var("CARGO_FEATURE_CLIENT_BASE").is_ok();
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
             cargo install why2-chat --no-default-features --features server,windows_resources"
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

    //TOFU SKIP (ON LIVE SYSTEM)
    println!("cargo:rustc-env=WHY2_SKIP_TOFU={}", env::var("WHY2_SKIP_TOFU").is_ok());
    println!("cargo::rerun-if-env-changed=WHY2_SKIP_TOFU");
}
