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

use std::io::{ self, Write };

use cpal::
{
    Device,
    traits::{ DeviceTrait, HostTrait },
};

use crate::config;

//PRIVATE
fn prompt_selection(devices: &[String], config: &str) -> usize
{
    //PROMPT
    let mut input: String;
    loop
    {
        print!("\nSelect device: ");
        io::stdout().flush().unwrap();
        input = String::new(); //CLEAR BUFFER
        io::stdin().read_line(&mut input).unwrap(); //READ INPUT

        if let Ok(idx) = input.trim().parse::<usize>() && idx != 0
        {
            if let Some(device) = devices.get(idx - 1)
            {
                config::client_write(config, device);
                println!("Set to: {}", device);
                return idx - 1;
            }
        }

        println!("Invalid selection!");
    }
}

fn load_devices<T>(all_devices: T) -> Vec<String>
where
    T: Iterator<Item = Device>
{
    //COLLECT
    let mut devices = Vec::new();
    for device in all_devices
    {
        //DO NOT PUSH DUPLICATES
        if let Ok(name) = device.description().map(|d| d.to_string()) && !devices.contains(&name)
        {
            devices.push(name);
        }
    }

    //SORT & RETURN
    devices.sort();
    devices
}

fn list_devices(devices: &[String])
{
    for (i, device) in devices.iter().enumerate()
    {
        println!("[{}]: {device}", i + 1);
    }
}

//PUBLIC
pub fn setup_devices() //SELECT AUDIO DEVICES AND STORE IN CLIENT CONFIG
{
    let host = cpal::default_host();

    //COLLECT INPUT DEVICES
    println!("Available input devices\n=======================");
    let input_devices = load_devices(host.input_devices().unwrap());

    //GET INPUT DEVICE
    list_devices(&input_devices); //LIST
    prompt_selection(&input_devices, "input_device"); //PROMPT

    //COLLECT OUTPUT DEVICES
    println!("\nAvailable output devices\n========================");
    let output_devices = load_devices(host.output_devices().unwrap());

    //GET OUTPUT DEVICE
    list_devices(&output_devices); //LIST
    prompt_selection(&output_devices, "output_device"); //PROMPT
}
