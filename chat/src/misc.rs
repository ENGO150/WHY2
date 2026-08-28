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
    fs,
    env,
    time::Duration,
    path::Path,
};

use ureq::{ Error, Agent };

use serde_json::Value;

use semver::Version;

use crate::consts;

#[cfg(feature = "client_base")]
use tokio::sync::mpsc::Sender;

#[cfg(feature = "client_base")]
use crate::network::client::ClientEvent;

#[cfg(feature = "server")]
use std::path::PathBuf;

//PRIVATE
fn get_dir(dir: &str) -> String
{
    dir.replace("{HOME}", dirs::home_dir().expect("Could not determine home directory").to_str().expect("Invalid home directory"))
}

//PUBLIC
pub fn get_version<'a>() -> &'a str //GET COMPILED PACKAGE VERSION
{
    env!("CARGO_PKG_VERSION")
}

pub fn get_identifier() -> String //GET IDENTIFIER OF PACKAGE VERSION [WHY2/VERSION]
{
    format!("WHY2/{}", get_version())
}

pub fn fetch_data(url: &str) -> Result<String, Error> //FETCH DATA USING REQWEST
{
    //CREATE CUSTOM CLIENT (WITH TIMEOUT)
    let agent: Agent = Agent::config_builder()
        .timeout_global(Some(Duration::from_millis(consts::FETCH_TIMEOUT)))
        .build()
        .into();

    //FETCH DATA
    agent.get(url)
        .header("User-Agent", &get_identifier())
        .call()?
        .body_mut()
        .read_to_string()
}

pub async fn check_version(#[cfg(feature = "client_base")] tx: &Sender<ClientEvent>) //CHECK FOR LATEST WHY2 VERSION
{
    //FETCH METADATA (USE CUSTOM User-Agent, FOR CRATES.IO TO WORK) - BLOCKING HTTP, KEEP IT OFF THE RUNTIME
    let metadata_raw = match tokio::task::spawn_blocking(|| fetch_data(consts::METADATA_URL)).await
        .expect("Fetching versions panicked")
    {
        Ok(m) => m,
        Err(_) =>
        {
            #[cfg(feature = "client_base")]
            {
                tx.send(ClientEvent::VersionFailed).await.unwrap();
            }

            #[cfg(feature = "server")]
            {
                log::warn!("Fetching versions failed, this release could be unsafe!");
            }

            return;
        }
    };

    //PARSE METADATA TO JSON
    let metadata: Value = serde_json::from_str(&metadata_raw).expect("Parsing versions failed"); //PARSE
    let newest_version = metadata.get("crate") //GET LATEST VERSION
        .and_then(|c| c.get("newest_version"))
        .and_then(|v| v.as_str())
        .unwrap();

    //OUTDATED VERSION, CALCULATE HOW MANY NEWER VERSIONS EXIST
    let current_version = get_version();
    if current_version != newest_version
    {
        //GET ARRAY OF VERSIONS
        let versions = metadata.get("versions").and_then(|v| v.as_array()).unwrap();
        let mut newer_versions = 0usize;
        let current_version = Version::parse(current_version).expect("Invalid version");

        //CALCULATE
        for version in versions
        {
            //FOUND NEWER VERSION
            if Version::parse(version.get("num").and_then(|n| n.as_str()).unwrap()).unwrap() > current_version
            {
                //INCREMENT COUNTER
                newer_versions += 1;
            }
        }

        #[cfg(feature = "client_base")]
        {
            tx.send(ClientEvent::UnsafeVersion(newer_versions, current_version, newest_version.to_owned())).await.unwrap();
        }

        #[cfg(feature = "server")]
        {
            log::warn!("This release could be unsafe! You are {newer_versions} versions behind! ({current_version}/{newest_version})");
        }
    }
}

pub fn get_why2_dir() -> String //RETURN PATH TO WHY2 CONFIG DIRECTORY
{
    get_dir(consts::CONFIG_DIR)
}

pub fn check_directory() //CREATE WHY2 CONFIG DIRECTORY
{
    let config = get_why2_dir();

    //CREATE WHY2 CONFIG DIRECTORY
    if !Path::new(&config).is_dir()
    {
        fs::create_dir_all(config).expect("Failed to create WHY2 config directory");
    }
}

#[cfg(feature = "server")]
pub fn get_upload_dir(username: &str) -> PathBuf //GET USER'S TEMP DIR FOR UPLOAD
{
    env::temp_dir().join(consts::UPLOADS_DIR).join(username)
}

#[cfg(feature = "server")]
pub fn restart() -> !
{
    let executable = match env::current_exe()
    {
        Ok(path) => path,
        Err(error) =>
        {
            log::error!("Restart failed (executable not found): {error}");
            std::process::exit(1);
        }
    };

    let arguments: Vec<String> = env::args().skip(1).collect();

    //ON UNIX THE PROCESS IMAGE IS REPLACED, KEEPING THE PID - WHATEVER SUPERVISES THE SERVER (systemd,
    //docker) NEVER SEES IT GO AWAY, AND THE LISTENING SOCKET IS CLOSED BY THE exec ITSELF (CLOSE-ON-EXEC)
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;

        //ONLY EVER RETURNS ON FAILURE
        let error = std::process::Command::new(&executable).args(&arguments).exec();

        log::error!("Restart failed: {error}");
    }

    //EVERYWHERE ELSE THERE IS NO exec, SO THE REPLACEMENT IS STARTED BESIDE US AND WE STAND DOWN - IT CAN
    //REACH ITS bind BEFORE WE ARE GONE, WHICH IS WHAT THE BINARY'S RETRY IS FOR
    #[cfg(not(unix))]
    match std::process::Command::new(&executable).args(&arguments).spawn()
    {
        Ok(_) => std::process::exit(0),
        Err(error) => log::error!("Restart failed: {error}"),
    }

    std::process::exit(1);
}
